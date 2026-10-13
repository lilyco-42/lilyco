"""Single freezing recipe used by public and licensed-product GitHub Actions."""
import json
import os
from pathlib import Path
import subprocess
import sys

if os.environ.get('GITHUB_ACTIONS') != 'true':
    raise SystemExit('Build only in GitHub Actions')
sys.path.insert(0, str(Path(__file__).resolve().parents[2] / 'desktop-common'))
from desktop_icons import prepare_icon, verify_embedded_icon
icon_receipt = prepare_icon(Path.cwd())

args=[sys.executable,'-m','PyInstaller','--noconfirm','--name','Lain42-Asset-Studio',
      '--add-data','frontend:frontend','--add-data','bin:bin','--add-data','resources:resources','--collect-submodules','uvicorn']
if sys.platform=='win32':
    args += ['--icon','resources/app.ico']
    args += ['--collect-all','webview','--collect-all','pythonnet','--collect-all','clr_loader',
             '--exclude-module','PyQt6','--exclude-module','PySide6','--exclude-module','gi']
else:
    args += ['--exclude-module','webview']
subprocess.run(args+['launcher.py'],check=True)

if sys.platform == 'win32':
    icon_receipt['executable'] = verify_embedded_icon(Path('dist/Lain42-Asset-Studio/Lain42-Asset-Studio.exe'), Path('resources/app.ico'))
Path('evidence').mkdir(exist_ok=True)
Path('evidence/icon-build.json').write_text(json.dumps(icon_receipt, indent=2), encoding='utf-8')
