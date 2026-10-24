"""Retain original shadcn sources and the installed runtime license texts in CI."""
import hashlib
import json
import os
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parent
if os.environ.get('GITHUB_ACTIONS') != 'true':
    raise SystemExit('Office frontend notice collection only runs in Actions')
ui = ROOT / 'ui'
sources = json.loads((ui / 'src/shadcn-sources.json').read_text(encoding='utf-8'))
for item in sources:
    source = ui / 'src/components/ui' / item['file']
    if hashlib.sha256(source.read_bytes()).hexdigest() != item['sha256']:
        raise ValueError('Original shadcn component modified: ' + item['file'])
notices = [{'name': 'shadcn/ui', 'source': 'https://github.com/shadcn-ui/ui', 'license': 'MIT',
            'notices': {'LICENSE': (ui / 'src/components/ui/LICENSE.md').read_text(encoding='utf-8')}}]
npm = 'npm.cmd' if os.name == 'nt' else 'npm'
paths = subprocess.check_output([npm, 'ls', '--omit=dev', '--all', '--parseable'],
                                cwd=ui, text=True, encoding='utf-8')
for line in paths.splitlines():
    directory = Path(line)
    if directory.resolve() == ui.resolve():
        continue
    if not directory.resolve().is_relative_to((ui / 'node_modules').resolve()):
        raise ValueError('Dependency attribution escaped the installed UI dependencies')
    package = json.loads((directory / 'package.json').read_text(encoding='utf-8'))
    texts = {}
    for file in directory.iterdir():
        if file.is_file() and file.name.upper().startswith(('LICENSE', 'LICENCE', 'COPYING', 'NOTICE', 'OFL')):
            if file.stat().st_size > 256 * 1024:
                raise ValueError('Unexpected oversized dependency notice')
            texts[file.name] = file.read_text(encoding='utf-8', errors='strict')
    if not texts:
        raise ValueError('Missing complete installed frontend notice: ' + package['name'])
    notices.append({'name': package['name'], 'version': package['version'],
                    'license': package.get('license'), 'notices': texts})
target = ROOT / 'resources/FRONTEND-LICENSES.json'
target.write_text(json.dumps(notices, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
print(f'Retained complete notices for {len(notices)} installed frontend dependencies')
