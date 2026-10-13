"""CI-only real installation, packaged processing and safe uninstall acceptance."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

if os.environ.get('GITHUB_ACTIONS') != 'true' or os.name != 'nt':
    raise SystemExit('Installation acceptance runs only on Windows GitHub Actions')
package = Path('dist/Lain42-Vision-Studio').resolve()
manifest = json.loads((package/'manifest.json').read_text(encoding='utf-8'))
with tempfile.TemporaryDirectory(prefix='lain42-installer-acceptance-') as temp:
    target = Path(temp)/'installed'
    target.mkdir()
    sentinel = target/'user-owned-file.txt'
    sentinel.write_text('Never remove files outside the packaged file list', encoding='utf-8')
    setup = Path('dist/lain42-vision-studio-windows-setup.exe').resolve()
    subprocess.run([str(setup), '/S', '/D='+str(target)], check=True, timeout=120)
    for name, digest in manifest['files'].items():
        assert hashlib.sha256((target/name).read_bytes()).hexdigest()==digest, name
    subprocess.run([sys.executable, 'tests/bundle_smoke.py', str(target/'Lain42-Vision-Studio.exe')], check=True, timeout=90)
    # NSIS launches a temporary child by default. Its parent exit is not proof
    # that cleanup finished; wait on an explicitly copied uninstaller instead.
    uninstaller = Path(temp)/'CI-Uninstall.exe'
    shutil.copyfile(target/'Uninstall.exe', uninstaller)
    subprocess.run([str(uninstaller), '/S', '_?='+str(target)], check=True, timeout=120)
    assert not (target/'Uninstall.exe').exists(), 'Original uninstaller was not removed'
    assert not (target/'Lain42-Vision-Studio.exe').exists()
    assert sentinel.read_text(encoding='utf-8')=='Never remove files outside the packaged file list'
    for name in manifest['files']:
        assert not (target/name).exists(), 'leftover packaged file: '+name
    startmenu=Path(os.environ['APPDATA'])/'Microsoft/Windows/Start Menu/Programs/Lain42 图片工坊'
    assert not startmenu.exists(), 'Start menu links were not removed'
    Path('evidence/installer-acceptance.json').write_text(json.dumps({'installed_files_sha256_verified':True,'installed_app_real_engine':True,'uninstall_removed_packaged_files':True,'unrelated_file_preserved':True,'start_menu_removed':True,'real_uninstaller_exit_checked':True,'original_uninstaller_removed':True}),encoding='utf-8')
