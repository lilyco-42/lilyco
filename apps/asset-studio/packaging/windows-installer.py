"""Generate a per-user NSIS installer from this CI's complete, verified bundle."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

if os.environ.get('GITHUB_ACTIONS') != 'true' or os.name != 'nt':
    raise SystemExit('Windows installers are built only in GitHub Actions')
root = Path('dist/Lain42-Asset-Studio').resolve()
assert (root / 'manifest.json').is_file()
sys.path.insert(0, str(Path(__file__).resolve().parents[2] / 'desktop-common'))
from desktop_icons import verify_embedded_icon
icon = root / '_internal/resources/app.ico'
assert icon.is_file()
files = sorted(p for p in root.rglob('*') if p.is_file())
assert len(files) < 10000

def quote(value):
    # NSIS interpolates dollar signs; package names are trusted build output, but
    # still must not turn a filename into an instruction or variable reference.
    return str(value).replace('$', '$$').replace('"', '$\\"')

deletes = []
directories = set()
for path in files:
    relative = path.relative_to(root)
    deletes.append(f'  Delete "$INSTDIR\\{quote(relative)}"')
    for parent in relative.parents:
        if str(parent) != '.':
            directories.add(parent)
deletes += [f'  RMDir "$INSTDIR\\{quote(p)}"' for p in sorted(directories, key=lambda p: len(p.parts), reverse=True)]
deletes += ['  Delete "$INSTDIR\\Uninstall.exe"', '  RMDir "$INSTDIR"']
target = Path('dist/lain42-asset-studio-windows-setup.exe').resolve()
script = Path('dist/asset-studio-installer.nsi')
script.write_text('''Unicode true
!include "MUI2.nsh"
Name "Lain42 素材工坊"
OutFile "''' + quote(target) + '''"
InstallDir "$LOCALAPPDATA\\Programs\\Lain42-Asset-Studio"
RequestExecutionLevel user
SetCompressor /SOLID lzma
!define MUI_ICON "''' + quote(icon) + '''"
!define MUI_UNICON "''' + quote(icon) + '''"
!define MUI_ABORTWARNING
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!define MUI_FINISHPAGE_RUN "$INSTDIR\\Lain42-Asset-Studio.exe"
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "SimpChinese"
!insertmacro MUI_LANGUAGE "English"
Section "素材工坊"
  SetShellVarContext current
  SetOutPath "$INSTDIR"
  File /r "''' + quote(root) + '''\\*"
  WriteUninstaller "$INSTDIR\\Uninstall.exe"
  CreateDirectory "$SMPROGRAMS\\Lain42 素材工坊"
  CreateShortCut "$SMPROGRAMS\\Lain42 素材工坊\\素材工坊.lnk" "$INSTDIR\\Lain42-Asset-Studio.exe" "" "$INSTDIR\\_internal\\resources\\app.ico" 0
  CreateShortCut "$SMPROGRAMS\\Lain42 素材工坊\\卸载.lnk" "$INSTDIR\\Uninstall.exe" "" "$INSTDIR\\_internal\\resources\\app.ico" 0
  WriteRegStr HKCU "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Lain42AssetStudio" "DisplayName" "Lain42 素材工坊"
  WriteRegStr HKCU "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Lain42AssetStudio" "DisplayIcon" "$INSTDIR\\Lain42-Asset-Studio.exe,0"
  WriteRegStr HKCU "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Lain42AssetStudio" "UninstallString" '$\\"$INSTDIR\\Uninstall.exe$\\"'
  WriteRegStr HKCU "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Lain42AssetStudio" "InstallLocation" "$INSTDIR"
  WriteRegDWORD HKCU "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Lain42AssetStudio" "NoModify" 1
  WriteRegDWORD HKCU "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Lain42AssetStudio" "NoRepair" 1
SectionEnd
Section "Uninstall"
  SetShellVarContext current
''' + '\n'.join(deletes) + '''
  Delete "$SMPROGRAMS\\Lain42 素材工坊\\素材工坊.lnk"
  Delete "$SMPROGRAMS\\Lain42 素材工坊\\卸载.lnk"
  RMDir "$SMPROGRAMS\\Lain42 素材工坊"
  DeleteRegKey HKCU "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Lain42AssetStudio"
SectionEnd
''', encoding='utf-8-sig')
compiler = shutil.which('makensis') or r'C:\Program Files (x86)\NSIS\makensis.exe'
subprocess.run([compiler, str(script.resolve())], check=True)
icon_evidence = verify_embedded_icon(target, icon)
Path('evidence/installer-icon.json').write_text(json.dumps(icon_evidence, indent=2), encoding='utf-8')
digest = hashlib.sha256(target.read_bytes()).hexdigest()
target.with_suffix('.exe.sha256').write_text(digest + '\n', encoding='ascii')
Path('evidence').mkdir(exist_ok=True)
Path('evidence/installer-build.json').write_text(json.dumps({'commit': os.environ.get('APP_COMMIT', os.environ['GITHUB_SHA']), 'spine_source_commit': os.environ['GITHUB_SHA'] if os.environ.get('SPINE_SOURCE_ROOT') else None, 'sha256': digest, 'size': target.stat().st_size}), encoding='utf-8')
