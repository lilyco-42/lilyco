"""CI-only attribution collector for the shipped Rust/Python dependencies."""
import importlib.metadata
import json
import os
from pathlib import Path
import re
import subprocess

if os.environ.get('GITHUB_ACTIONS') != 'true':
    raise SystemExit('Dependency notices are generated with the build in GitHub Actions only')
root = Path(__file__).parents[3]
tree = subprocess.check_output(['cargo','tree','--locked','-p','lilyco-aic','--no-default-features','--features','android','--prefix','none','--format','{p}'],cwd=root,text=True)
selected = set(re.findall(r'^([\w-]+) v([\w.+-]+)', tree, re.M))
metadata = json.loads(subprocess.check_output(['cargo','metadata','--locked','--format-version','1'],cwd=root,text=True))
notices = []
for package in metadata['packages']:
    if (package['name'],package['version']) not in selected:
        continue
    directory = Path(package['manifest_path']).parent
    texts = {}
    for p in directory.iterdir():
        if p.is_file() and p.name.upper().startswith(('LICENSE','LICENCE','COPYING','NOTICE')) and p.stat().st_size < 256*1024:
            texts[p.name] = p.read_text(encoding='utf-8',errors='replace')
    notices.append({'ecosystem':'Rust','name':package['name'],'version':package['version'],'license':package.get('license'),'notices':texts})
for distribution in importlib.metadata.distributions():
    texts = {}
    for file in distribution.files or []:
        if '.dist-info/' in str(file).replace('\\','/') and re.search(r'(?i)(license|licence|copying|notice)', str(file)):
            p = distribution.locate_file(file)
            if p.is_file() and p.stat().st_size < 256*1024:
                texts[str(file)] = p.read_text(encoding='utf-8',errors='replace')
    notices.append({'ecosystem':'Python (includes CI packaging dependencies)','name':distribution.metadata['Name'],'version':distribution.version,'license':distribution.metadata.get('License-Expression') or distribution.metadata.get('License'),'notices':texts})
target = Path('THIRD-PARTY-NOTICES.json')
target.write_text(json.dumps(notices,ensure_ascii=False,indent=2),encoding='utf-8')
print(f'Collected attribution for {len(notices)} dependencies')
