"""CI-only attribution collector for the shipped Rust/Python dependencies."""
import importlib.metadata
import hashlib
import io
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import ssl
import urllib.request
import zipfile

if os.environ.get('GITHUB_ACTIONS') != 'true':
    raise SystemExit('Dependency notices are generated with the build in GitHub Actions only')
root = Path(__file__).parents[3]
tree = subprocess.check_output(['cargo','tree','--locked','-p','lilyco-aic','--no-default-features','--features','android','--prefix','none','--format','{p}'],cwd=root,text=True,encoding='utf-8')
selected = set(re.findall(r'^([\w-]+) v([\w.+-]+)', tree, re.M))
metadata = json.loads(subprocess.check_output(['cargo','metadata','--locked','--format-version','1'],cwd=root,text=True,encoding='utf-8'))
notices = []
spine_root = os.environ.get('SPINE_SOURCE_ROOT')
if spine_root:
    spine_root = Path(spine_root).resolve()
    spine_tree = subprocess.check_output(['cargo','tree','--locked','-p','lyco-spine-render','--prefix','none','--format','{p}'],cwd=spine_root,text=True,encoding='utf-8')
    spine_selected = set(re.findall(r'^([\w-]+) v([\w.+-]+)',spine_tree,re.M))
    spine_metadata = json.loads(subprocess.check_output(['cargo','metadata','--locked','--format-version','1'],cwd=spine_root,text=True,encoding='utf-8'))
    for package in spine_metadata['packages']:
        if (package['name'],package['version']) not in spine_selected:
            continue
        directory = Path(package['manifest_path']).parent
        texts = {}
        for parent in [directory, directory.parent, directory.parent.parent]:
            for p in parent.iterdir():
                if p.is_file() and p.name.upper().startswith(('LICENSE','LICENCE','COPYING','NOTICE')) and p.stat().st_size < 256*1024:
                    texts[str(p.relative_to(parent))] = p.read_text(encoding='utf-8',errors='replace')
        notices.append({'ecosystem':'Spine product dependency','name':package['name'],'version':package['version'],'license':package.get('license'),'source':package.get('source'),'notices':texts})
    # Runtime is not MIT: include the current agreement and pinned upstream notice.
    url='https://esotericsoftware.com/spine-runtimes-license'
    with urllib.request.urlopen(url,timeout=30) as response:
        agreement=response.read(256*1024+1).decode('utf-8')
    if 'Spine Runtimes License Agreement' not in agreement or len(agreement)>256*1024:
        raise RuntimeError('Spine runtime agreement could not be included')
    Path('SPINE-RUNTIME-LICENSE.html').write_text(agreement,encoding='utf-8')
    notices.append({'ecosystem':'Spine Runtimes','name':'Spine Runtime 3.8','license':'Spine Runtimes License Agreement (not MIT)','source':url,'notices':{'current-agreement':agreement}})
# Frozen apps also contain CPython and its OpenSSL shared libraries, outside pip metadata.
python_version = '.'.join(str(v) for v in sys.version_info[:3])
openssl_version = ssl.OPENSSL_VERSION.split()[1]
for name, version, url in [
    ('CPython', python_version, f'https://raw.githubusercontent.com/python/cpython/v{python_version}/LICENSE'),
    ('OpenSSL', openssl_version, f'https://raw.githubusercontent.com/openssl/openssl/openssl-{openssl_version}/LICENSE.txt'),
]:
    with urllib.request.urlopen(url,timeout=30) as response:
        text = response.read(256*1024+1).decode('utf-8')
    if len(text) < 100 or len(text) > 256*1024:
        raise RuntimeError('Invalid bundled runtime notice')
    notices.append({'ecosystem':'Bundled native runtime','name':name,'version':version,'source':url,'notices':{'LICENSE':text}})
# pywebview's own BSD notice does not replace the SDK's independent notice.
if os.name == 'nt':
    pin = json.loads(Path('packaging/webview2-sdk.json').read_text(encoding='utf-8'))
    with urllib.request.urlopen(pin['source'], timeout=30) as response:
        package = response.read(40 * 1024 * 1024 + 1)
    assert len(package) <= 40 * 1024 * 1024
    assert hashlib.sha256(package).hexdigest() == pin['package_sha256']
    distribution = importlib.metadata.distribution('pywebview')
    bundled = Path('dist/Lain42-Asset-Studio/_internal/webview/lib')
    with zipfile.ZipFile(io.BytesIO(package)) as sdk:
        license_bytes = sdk.read(pin['license_path'])
        assert hashlib.sha256(license_bytes).hexdigest() == pin['license_sha256']
        assert Path('MICROSOFT-WEBVIEW2-LICENSE.txt').read_bytes() == license_bytes
        for file in pin['files']:
            assert hashlib.sha256(sdk.read(file['sdk_path'])).hexdigest() == file['sha256']
            installed = distribution.locate_file('webview/lib/' + file['app_path'])
            assert hashlib.sha256(installed.read_bytes()).hexdigest() == file['sha256'], file['app_path']
            assert hashlib.sha256((bundled / file['app_path']).read_bytes()).hexdigest() == file['sha256'], file['app_path']
    notices.append({'ecosystem': 'Bundled native SDK', 'name': pin['name'],
                    'version': pin['version'], 'license': 'BSD-3-Clause', 'source': pin['source'],
                    'verified_files': pin['files'], 'notices': {'LICENSE.txt': license_bytes.decode('utf-8')}})
    Path('evidence').mkdir(exist_ok=True)
    Path('evidence/webview2-sdk.json').write_text(json.dumps({
        'commit': os.environ.get('APP_COMMIT', os.environ['GITHUB_SHA']), 'version': pin['version'],
        'package_sha256': pin['package_sha256'], 'license_sha256': pin['license_sha256'],
        'five_bundled_files_match_official_sdk': True}), encoding='utf-8')
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
ui = Path('ui')
# Preserve the exact official shadcn sources; visual customization lives in wrappers.
sources = json.loads((ui/'src/shadcn-sources.json').read_text(encoding='utf-8'))
for item in sources:
    source = ui/'src/components/ui'/item['file']
    assert hashlib.sha256(source.read_bytes()).hexdigest() == item['sha256'], item['file']
notices.append({'ecosystem':'Frontend component source','name':'shadcn/ui','source':'https://github.com/shadcn-ui/ui',
                'license':'MIT','notices':{'LICENSE':(ui/'src/components/ui/LICENSE.md').read_text(encoding='utf-8')}})
npm = 'npm.cmd' if os.name == 'nt' else 'npm'
paths = subprocess.check_output([npm,'ls','--omit=dev','--all','--parseable'],cwd=ui,text=True,encoding='utf-8')
for line in paths.splitlines():
    directory = Path(line)
    if directory.resolve() == ui.resolve():
        continue
    package = json.loads((directory/'package.json').read_text(encoding='utf-8'))
    texts = {}
    for file in directory.iterdir():
        if file.is_file() and file.name.upper().startswith(('LICENSE','LICENCE','COPYING','NOTICE')) and file.stat().st_size < 256*1024:
            texts[file.name] = file.read_text(encoding='utf-8',errors='replace')
    notices.append({'ecosystem':'Bundled frontend JavaScript','name':package['name'],'version':package['version'],
                    'license':package.get('license'),'notices':texts})
target.write_text(json.dumps(notices,ensure_ascii=False,indent=2),encoding='utf-8')
print(f'Collected attribution for {len(notices)} dependencies')
