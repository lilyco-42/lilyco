"""CI-only real installation, packaged processing and safe uninstall acceptance."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time

if os.environ.get('GITHUB_ACTIONS') != 'true' or os.name != 'nt':
    raise SystemExit('Installation acceptance runs only on Windows GitHub Actions')
package = Path('dist/Lain42-Asset-Studio').resolve()
manifest = json.loads((package/'manifest.json').read_text(encoding='utf-8'))
with tempfile.TemporaryDirectory(prefix='lain42-installer-acceptance-') as temp:
    target = Path(temp)/'installed'
    target.mkdir()
    sentinel = target/'user-owned-file.txt'
    sentinel.write_text('Never remove files outside the packaged file list', encoding='utf-8')
    setup = Path('dist/lain42-asset-studio-windows-setup.exe').resolve()
    subprocess.run([str(setup), '/S', '/D='+str(target)], check=True, timeout=120)
    for name, digest in manifest['files'].items():
        assert hashlib.sha256((target/name).read_bytes()).hexdigest()==digest, name
    subprocess.run([sys.executable, 'tests/bundle_smoke.py', str(target/'Lain42-Asset-Studio.exe')], check=True, timeout=90)
    subprocess.run([str(target/'Uninstall.exe'), '/S'], check=True, timeout=120)
    for _ in range(100):
        if not (target/'Lain42-Asset-Studio.exe').exists():
            break
        time.sleep(.2)
    assert not (target/'Lain42-Asset-Studio.exe').exists()
    assert sentinel.read_text(encoding='utf-8')=='Never remove files outside the packaged file list'
    for name in manifest['files']:
        assert not (target/name).exists(), 'leftover packaged file: '+name
    startmenu=Path(os.environ['APPDATA'])/'Microsoft/Windows/Start Menu/Programs/Lain42 素材工坊'
    assert not startmenu.exists(), 'Start menu links were not removed'
    Path('evidence/installer-acceptance.json').write_text(json.dumps({'installed_files_sha256_verified':True,'installed_app_real_engine':True,'uninstall_removed_packaged_files':True,'unrelated_file_preserved':True,'start_menu_removed':True}),encoding='utf-8')
