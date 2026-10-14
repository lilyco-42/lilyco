"""Read the actual locked Office/lbin dependency closure and native SDK in CI."""
import base64
import gzip
import hashlib
import io
import json
import os
from pathlib import Path
import re
import subprocess
import tarfile
import tomllib
import urllib.request
import urllib.parse
import zipfile

ROOT = Path(__file__).resolve().parent
WORKSPACE = ROOT.parent
NOTICE_PREFIXES = ('LICENSE', 'LICENCE', 'COPYING', 'NOTICE', 'COPYRIGHT')


def sha(data):
    return hashlib.sha256(data).hexdigest()


def read_notice(directory, path):
    path = Path(path)
    if not path.resolve().is_relative_to(Path(directory).resolve()):
        raise ValueError('Dependency notice escaped its package')
    if not path.name.upper().startswith(NOTICE_PREFIXES):
        raise ValueError('Explicit dependency notice has an unexpected filename')
    data = path.read_bytes()
    if not data or len(data) > 512 * 1024:
        raise ValueError('Dependency notice is empty or unexpectedly large')
    # Preserve original bytes even when a historical notice is not UTF-8.
    return {'sha256': sha(data), 'bytes': len(data),
            'original_base64': base64.b64encode(data).decode('ascii'),
            'text': data.decode('utf-8', errors='replace')}


def provenance_url(url):
    parsed = urllib.parse.urlsplit(url)
    if (parsed.scheme != 'https' or parsed.hostname not in
            {'raw.githubusercontent.com', 'api.nuget.org'} or parsed.port not in (None, 443)
            or parsed.username or parsed.password or parsed.fragment):
        raise ValueError('Unsupported provenance origin')
    return url


class ProvenanceRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, response, code, message, headers, newurl):
        provenance_url(newurl)
        return super().redirect_request(request, response, code, message, headers, newurl)


def fetch(url, limit):
    provenance_url(url)
    opener = urllib.request.build_opener(ProvenanceRedirect())
    with opener.open(url, timeout=60) as response:
        provenance_url(response.geturl())
        data = response.read(limit + 1)
        encoding = response.headers.get('Content-Encoding', 'identity').lower()
    if len(data) > limit:
        raise ValueError('Provenance resource exceeded the bounded size')
    if encoding == 'gzip':
        with gzip.GzipFile(fileobj=io.BytesIO(data)) as compressed:
            data = compressed.read(limit + 1)
        if len(data) > limit:
            raise ValueError('Decoded provenance resource exceeded the bounded size')
    elif encoding != 'identity':
        raise ValueError('Unsupported provenance content encoding')
    return data


def official_sdk_catalog(version):
    # Resource addresses come from Microsoft's service index. Do not invent a
    # .nupkg.sha512 endpoint: the documented catalog carries SHA512 and size.
    index = json.loads(fetch('https://api.nuget.org/v3/index.json', 512 * 1024))
    def resource(kind):
        addresses = {item['@id'] for item in index['resources'] if item['@type'] == kind}
        if len(addresses) != 1:
            raise ValueError('Official NuGet service resource is not unique: ' + kind)
        address = provenance_url(addresses.pop())
        if urllib.parse.urlsplit(address).hostname != 'api.nuget.org':
            raise ValueError('NuGet metadata left its official host')
        return address.rstrip('/') + '/'

    registration = resource('RegistrationsBaseUrl/3.6.0')
    package_base = resource('PackageBaseAddress/3.0.0')
    package_id = 'microsoft.web.webview2'
    package_url = f'{package_base}{package_id}/{version}/{package_id}.{version}.nupkg'
    registrations = json.loads(fetch(registration + package_id + '/index.json', 8 * 1024 * 1024))
    numeric = lambda value: tuple(int(part) for part in value.split('-')[0].split('.'))
    found = []
    for page in registrations['items']:
        if not numeric(page['lower']) <= numeric(version) <= numeric(page['upper']):
            continue
        leaves = page.get('items')
        if leaves is None:
            leaves = json.loads(fetch(page['@id'], 8 * 1024 * 1024))['items']
        found.extend(leaf for leaf in leaves if leaf['catalogEntry']['version'] == version)
    if len(found) != 1 or found[0]['packageContent'] != package_url:
        raise ValueError('Exact SDK version does not identify one official package')
    catalog_url = provenance_url(found[0]['catalogEntry']['@id'])
    if not catalog_url.startswith('https://api.nuget.org/v3/catalog'):
        raise ValueError('SDK catalog entry left the official NuGet catalog')
    catalog = json.loads(fetch(catalog_url, 1024 * 1024))
    if catalog['id'].lower() != package_id or catalog['version'] != version:
        raise ValueError('SDK catalog identifies another package')
    return package_url, catalog_url, catalog


def verify_sdk_archive(archive, catalog):
    if catalog.get('packageHashAlgorithm') != 'SHA512':
        raise ValueError('SDK catalog does not provide the required SHA512')
    if type(catalog.get('packageSize')) is not int or catalog['packageSize'] != len(archive):
        raise ValueError('SDK archive size differs from its official catalog')
    expected = base64.b64decode(catalog['packageHash'], validate=True)
    if expected != hashlib.sha512(archive).digest():
        raise ValueError('Official SDK archive differs from its NuGet catalog checksum')


def original_sdk_member(sdk, path, limit):
    matches = [item for item in sdk.infolist() if item.filename == path]
    if len(matches) != 1 or matches[0].is_dir() or not 0 < matches[0].file_size <= limit:
        raise ValueError('Original SDK payload is missing, duplicated or oversized: ' + path)
    with sdk.open(matches[0]) as stream:
        data = stream.read(limit + 1)
    if len(data) != matches[0].file_size or len(data) > limit:
        raise ValueError('Original SDK payload size is inconsistent')
    return data


def verify_crate_archive(data, checksum):
    if not re.fullmatch(r'[a-f0-9]{64}', checksum) or sha(data) != checksum:
        raise ValueError('Cached crate archive differs from the exact Cargo.lock checksum')


def sealed_crate(package):
    folder = Path(package['manifest_path']).parent
    lock = tomllib.loads((WORKSPACE / 'Cargo.lock').read_text(encoding='utf-8'))
    matches = [p for p in lock['package'] if p['name'] == package['name']
               and p['version'] == package['version'] and p.get('source') == package['source']]
    if len(matches) != 1:
        raise ValueError('Actual registry dependency has no unique Cargo.lock seal')
    checksum = matches[0]['checksum']
    registry = folder.parents[2]
    archive = registry / 'cache' / folder.parent.name / f"{package['name']}-{package['version']}.crate"
    if not archive.resolve().is_relative_to(registry.resolve()) or not 0 < archive.stat().st_size <= 128 * 1024 * 1024:
        raise ValueError('Actual Cargo registry archive is missing, outside its cache or oversized')
    data = archive.read_bytes()
    verify_crate_archive(data, checksum)
    return data, checksum


def original_crate_member(archive, path, limit):
    with tarfile.open(fileobj=io.BytesIO(archive), mode='r:gz') as crate:
        members = []
        count = 0
        for item in crate:
            count += 1
            if count > 20000:
                raise ValueError('Cached crate has too many archive entries')
            if item.name == path:
                members.append(item)
        if len(members) != 1 or not members[0].isfile() or not 0 < members[0].size <= limit:
            raise ValueError('Original crate member is missing, duplicated or oversized: ' + path)
        with crate.extractfile(members[0]) as stream:
            data = stream.read(limit + 1)
        if len(data) != members[0].size or len(data) > limit:
            raise ValueError('Original crate member size is inconsistent')
        return data


def selected_packages():
    rust = subprocess.check_output(['rustc', '-vV'], text=True, encoding='utf-8')
    host = re.search(r'^host: (.+)$', rust, re.M).group(1)
    tree = subprocess.check_output([
        'cargo', 'tree', '--locked', '-p', 'lilyco-tauri', '-p', 'lilyco-binfmt',
        '--target', host, '--edges', 'normal', '--prefix', 'none', '--format', '{p}',
    ], cwd=WORKSPACE, text=True, encoding='utf-8')
    chosen = set(re.findall(r'^([\w-]+) v([\w.+-]+)', tree, re.M))
    metadata = json.loads(subprocess.check_output([
        'cargo', 'metadata', '--locked', '--format-version', '1', '--filter-platform', host,
    ], cwd=WORKSPACE, text=True, encoding='utf-8'))
    packages = [p for p in metadata['packages'] if (p['name'], p['version']) in chosen]
    if len(packages) != len(chosen):
        raise ValueError('Locked runtime dependency closure is not fully represented')
    return host, rust, packages


def collect_packages(packages):
    notices = []
    missing = []
    for package in packages:
        folder = Path(package['manifest_path']).parent
        candidates = [p for p in folder.iterdir() if p.is_file() and p.name.upper().startswith(NOTICE_PREFIXES)]
        if package.get('license_file'):
            candidates.append(folder / package['license_file'])
        texts = {}
        for path in sorted(set(candidates)):
            texts[str(path.relative_to(folder))] = read_notice(folder, path)
        # Own workspace crates inherit the repository's license choice/texts.
        if not texts and package.get('source') is None and folder.resolve().is_relative_to(WORKSPACE.resolve()):
            for path in WORKSPACE.iterdir():
                if path.is_file() and path.name.upper().startswith(NOTICE_PREFIXES):
                    texts['workspace/' + path.name] = read_notice(WORKSPACE, path)
        if not texts:
            missing.append({'name': package['name'], 'version': package['version'], 'license': package.get('license')})
        archive_checksum = None
        if (package.get('source') or '').startswith('registry+'):
            archive, archive_checksum = sealed_crate(package)
            for path, notice in texts.items():
                original = original_crate_member(archive, f"{package['name']}-{package['version']}/{path}", 512 * 1024)
                if original != base64.b64decode(notice['original_base64']):
                    raise ValueError('Extracted dependency notice differs from the locked original crate')
        notices.append({'name': package['name'], 'version': package['version'],
                        'license': package.get('license'), 'source': package.get('source'),
                        'repository': package.get('repository'),
                        'cargo_archive_sha256': archive_checksum,
                        'notices': texts})
    return notices, missing


def collect_webview_sdk(packages, host):
    if 'windows' not in host:
        return {'applicable': False}
    matches = [p for p in packages if p['name'] == 'webview2-com-sys']
    if len(matches) != 1:
        raise ValueError('Expected one actual Windows WebView2 bindings package')
    package = matches[0]
    if package.get('repository', '').rstrip('/') != 'https://github.com/wravery/webview2-rs':
        raise ValueError('Unexpected WebView2 provenance repository')
    folder = Path(package['manifest_path']).parent
    vcs = json.loads((folder / '.cargo_vcs_info.json').read_text(encoding='utf-8'))
    if vcs['git'].get('dirty'):
        raise ValueError('Packaged WebView2 source is marked dirty')
    commit = vcs['git']['sha1']
    if not re.fullmatch(r'[a-f0-9]{40}', commit):
        raise ValueError('Invalid actual packaged source commit')
    source_url = f'https://raw.githubusercontent.com/wravery/webview2-rs/{commit}/crates/update-bindings/src/main.rs'
    update_source = fetch(source_url, 128 * 1024).decode('utf-8')
    versions = re.findall(r'const WEBVIEW2_VERSION: &str = "([0-9]+\.[0-9]+\.[0-9]+\.[0-9]+)";', update_source)
    if len(versions) != 1:
        raise ValueError('Exact packaged source does not identify a unique SDK version')
    version = versions[0]
    url, catalog_url, catalog = official_sdk_catalog(version)
    archive = fetch(url, 128 * 1024 * 1024)
    verify_sdk_archive(archive, catalog)
    arch = 'x64' if host.startswith('x86_64-') else 'arm64' if host.startswith('aarch64-') else 'x86' if host.startswith('i686-') else None
    if not arch or not host.endswith('-msvc'):
        raise ValueError('Windows SDK static-library audit requires a known MSVC target')
    actual = folder / arch / 'WebView2LoaderStatic.lib'
    actual_bytes = actual.read_bytes()
    # .cargo-checksum.json is a vendor-directory artifact, not a guaranteed file
    # in Cargo's registry cache. Audit the original .crate against Cargo.lock.
    crate_archive, crate_checksum = sealed_crate(package)
    crate_path = f"{package['name']}-{package['version']}/{arch}/WebView2LoaderStatic.lib"
    if original_crate_member(crate_archive, crate_path, 32 * 1024 * 1024) != actual_bytes:
        raise ValueError('Actual Cargo SDK library differs from its sealed package')
    with zipfile.ZipFile(io.BytesIO(archive)) as sdk:
        path = f'build/native/{arch}/WebView2LoaderStatic.lib'
        if original_sdk_member(sdk, path, 32 * 1024 * 1024) != actual_bytes:
            raise ValueError('Actual linked SDK library does not match the identified official SDK')
        license_bytes = original_sdk_member(sdk, 'LICENSE.txt', 256 * 1024)
        if len(license_bytes) < 100 or len(license_bytes) > 256 * 1024:
            raise ValueError('Incomplete original Microsoft SDK license')
    (ROOT / 'resources/MICROSOFT-WEBVIEW2-SDK-LICENSE.txt').write_bytes(license_bytes)
    return {'applicable': True, 'bindings_version': package['version'], 'packaged_source_commit': commit,
            'sdk_version': version, 'sdk_source': url, 'sdk_catalog_source': catalog_url,
            'sdk_archive_sha256': sha(archive), 'sdk_archive_sha512': catalog['packageHash'],
            'bindings_crate_archive_sha256': crate_checksum,
            'actual_static_library_sha256': sha(actual_bytes), 'actual_library_matches_official_sdk': True,
            'complete_sdk_license_sha256': sha(license_bytes)}


def run():
    host, rust, packages = selected_packages()
    notices, missing = collect_packages(packages)
    sdk = collect_webview_sdk(packages, host)
    report = {'host': host, 'rust_toolchain': rust, 'locked_runtime_packages': notices,
              'missing_complete_package_notices': missing, 'webview2_sdk': sdk,
              'redistribution_audit_complete': False,
              'remaining_scope': ['Rust standard-library/compiler-builtins original notices',
                                  'actual native bundle payloads and any bundled system-library source obligations',
                                  'license expression review and actual installed notice byte verification']}
    target = ROOT / 'resources/RUST-LICENSES.json'
    target.write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
    print(json.dumps({'host': host, 'runtime_packages': len(notices), 'missing': missing,
                      'webview2_sdk': sdk, 'redistribution_audit_complete': False}))


if __name__ == '__main__':
    if os.environ.get('GITHUB_ACTIONS') != 'true':
        raise SystemExit('Office Rust/native notice collection only runs in GitHub Actions')
    run()
