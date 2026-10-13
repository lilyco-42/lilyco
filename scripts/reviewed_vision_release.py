"""Inspect already-built formal CI bytes and publish only a complete reviewed release.

This does not rebuild or run products. All execution is restricted to Actions.
"""
import argparse
import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import re
import subprocess
import urllib.error
import urllib.request
from urllib.parse import urlsplit
import zipfile

REPO = 'lilyco-42/lilyco'
PREFIX = 'Lain42-Vision-Studio/'
MAX_ARCHIVE = 200 * 1024 * 1024


def sha(data):
    return hashlib.sha256(data).hexdigest()


def valid_run(run, commit):
    if (run['head_sha'] != commit or run['head_branch'] != 'main' or run['event'] != 'push'
            or run['name'] != 'Vision Studio Desktop' or run['status'] != 'completed'
            or run['conclusion'] != 'success' or run['head_repository']['full_name'] != REPO):
        raise ValueError('Source must be the successful formal main Vision workflow')


def require_receipt(receipt, commit, flags, unstamped=False):
    if receipt.get('commit', commit if unstamped else None) != commit or any(receipt.get(flag) is not True for flag in flags):
        raise ValueError('Receipt does not prove required source and behavior')


def package_manifest(archive, commit):
    entries = [entry for entry in archive.infolist() if not entry.is_dir()]
    names = {entry.filename for entry in entries}
    if len(names) != len(entries) or sum(entry.file_size for entry in entries) > 250 * 1024 * 1024:
        raise ValueError('Duplicate or excessive package entries')
    for name in names:
        path = PurePosixPath(name)
        if path.is_absolute() or '..' in path.parts or not name.startswith(PREFIX):
            raise ValueError('Unsafe package path')
    manifest = json.loads(archive.read(PREFIX + 'manifest.json'))
    if manifest['commit'] != commit or manifest['engine'] != 'lilyco-vision/lvision --mcp':
        raise ValueError('Package does not contain the intended original engine source')
    if names != {PREFIX + name for name in manifest['files']} | {PREFIX + 'manifest.json'}:
        raise ValueError('Manifest must cover every packaged file exactly')
    for name, digest in manifest['files'].items():
        if sha(archive.read(PREFIX + name)) != digest:
            raise ValueError('Manifest file digest differs: ' + name)
    return manifest


def github(path):
    request = urllib.request.Request('https://api.github.com/repos/' + REPO + '/' + path,
        headers={'Authorization': 'Bearer ' + os.environ['GH_TOKEN'], 'Accept': 'application/vnd.github+json'})
    with urllib.request.urlopen(request, timeout=30) as response:
        return json.load(response)


def artifact_bytes(item):
    class NoRedirect(urllib.request.HTTPRedirectHandler):
        def redirect_request(self, *args):
            return None
    request = urllib.request.Request('https://api.github.com/repos/' + REPO + '/actions/artifacts/' + str(item['id']) + '/zip',
        headers={'Authorization': 'Bearer ' + os.environ['GH_TOKEN'], 'Accept': 'application/vnd.github+json'})
    try:
        urllib.request.build_opener(NoRedirect()).open(request, timeout=30)
        raise ValueError('Expected the official artifact redirect')
    except urllib.error.HTTPError as error:
        if error.code != 302:
            raise ValueError('Artifact retrieval refused') from None
        url = error.headers['Location']
    parsed = urlsplit(url)
    if parsed.scheme != 'https' or not parsed.hostname.endswith('.blob.core.windows.net'):
        raise ValueError('Unexpected artifact origin')
    with urllib.request.urlopen(url, timeout=60) as response:
        data = response.read(MAX_ARCHIVE + 1)
    if len(data) > MAX_ARCHIVE or 'sha256:' + sha(data) != item['digest']:
        raise ValueError('Official outer artifact digest differs')
    return data


def members(data):
    with zipfile.ZipFile(io.BytesIO(data)) as archive:
        entries = [entry for entry in archive.infolist() if not entry.is_dir()]
        if sum(entry.file_size for entry in entries) > MAX_ARCHIVE:
            raise ValueError('Excessive artifact size')
        result = {}
        for entry in entries:
            name = PurePosixPath(entry.filename).name
            if name in result or name != entry.filename:
                raise ValueError('Unexpected artifact member layout')
            result[name] = archive.read(entry)
        return result


def inspect(commit, run_id, destination):
    valid_run(github('actions/runs/' + str(run_id)), commit)
    runs = github('actions/runs?head_sha=' + commit + '&per_page=100')['workflow_runs']
    latest = {}
    for run in sorted(runs, key=lambda item: item['id'], reverse=True):
        latest.setdefault(run['name'], run)
    if 'CI' not in latest or any(run['status'] != 'completed' or run['conclusion'] != 'success'
                                for name, run in latest.items() if name in {'CI', 'Asset Studio', 'Vision Studio Desktop'}):
        raise ValueError('All source-commit gates must be completed successfully')
    # Publisher changes cannot smuggle an older app into a new release.
    subprocess.run(['git', 'diff', '--exit-code', commit, 'HEAD', '--', 'apps/vision-studio',
                    'lilyco-*', 'Cargo.toml', 'Cargo.lock', '.github/workflows/vision-studio.yml'], check=True)
    artifacts = github('actions/runs/' + str(run_id) + '/artifacts?per_page=100')['artifacts']
    selected = {item['name']: item for item in artifacts if not item['expired']}
    pin = json.loads(subprocess.check_output(['git', 'show', commit + ':apps/vision-studio/packaging/webview2-sdk.json']))
    expected_license = subprocess.check_output(['git', 'show', commit + ':apps/vision-studio/MICROSOFT-WEBVIEW2-LICENSE.txt'])
    with urllib.request.urlopen(pin['source'], timeout=30) as response:
        sdk_bytes = response.read(40 * 1024 * 1024 + 1)
    if sha(sdk_bytes) != pin['package_sha256']:
        raise ValueError('Official SDK package differs')
    with zipfile.ZipFile(io.BytesIO(sdk_bytes)) as sdk:
        if sdk.read(pin['license_path']) != expected_license or sha(expected_license) != pin['license_sha256']:
            raise ValueError('Official SDK license differs')
        for file in pin['files']:
            if sha(sdk.read(file['sdk_path'])) != file['sha256']:
                raise ValueError('SDK file pin differs from the official package')
    records, assets = [], []
    for platform in ('Windows', 'Linux'):
        package_item = selected['lain42-vision-studio-' + platform + '-' + commit]
        evidence_item = selected['vision-evidence-' + platform + '-' + commit]
        package = members(artifact_bytes(package_item))
        evidence = members(artifact_bytes(evidence_item))
        receipt = lambda name: json.loads(evidence[name + '.json'])
        filename = 'lain42-vision-studio-' + platform.lower() + '.zip'
        digest = sha(package[filename])
        if digest != package[filename + '.sha256'].decode().strip() or receipt('package')['sha256'] != digest:
            raise ValueError('Portable package digest differs from evidence')
        if receipt('package')['commit'] != commit:
            raise ValueError('Package evidence is not from this source')
        with zipfile.ZipFile(io.BytesIO(package[filename])) as archive:
            manifest = package_manifest(archive, commit)
            version = json.loads(archive.read(PREFIX + 'UI-package-lock.json'))['version']
            if version != '0.1.1':
                raise ValueError('Only reviewed version 0.1.1 is allowed by this release policy')
            notices = json.loads(archive.read(PREFIX + 'THIRD-PARTY-NOTICES.json'))
            if not any(n['name'] == 'vtracer' for n in notices) or not any(n['name'] == 'shadcn/ui' for n in notices):
                raise ValueError('Original engine or UI notices missing')
            if platform == 'Windows':
                if json.loads(archive.read(PREFIX + 'webview2-sdk.json')) != pin or archive.read(PREFIX + 'MICROSOFT-WEBVIEW2-LICENSE.txt') != expected_license:
                    raise ValueError('Independent SDK attribution missing')
                for file in pin['files']:
                    if sha(archive.read(PREFIX + '_internal/webview/lib/' + file['app_path'])) != file['sha256']:
                        raise ValueError('Packaged SDK DLL differs from official pin')
                if not any(n['name'] == pin['name'] and n['version'] == pin['version']
                           and n['notices']['LICENSE.txt'].encode() == expected_license for n in notices):
                    raise ValueError('Full SDK notice missing from the notice inventory')
        eight = receipt('eight-tools')
        if eight['commit'] != commit or len(eight['tools']) != 8 or not all(
                item['state'] == 'done' and item['result']['ok'] for item in eight['tools'].values()):
            raise ValueError('All eight original tools must have successful real evidence')
        require_receipt(receipt('bundle-smoke'), commit, ['frozen_app', 'bundled_original_mcp',
                        'crop_pixels_and_dimensions', 'real_zip_bytes', 'deleted_results'])
        if platform == 'Windows':
            require_receipt(receipt('windows-native'), commit, ['native_webview2', 'real_crop_and_preview',
                            'real_export_zip', 'normal_close', 'workspace_removed', 'port_closed'])
            require_receipt(receipt('installer-acceptance'), commit, ['installed_files_sha256_verified',
                'installed_app_real_engine', 'uninstall_removed_packaged_files', 'unrelated_file_preserved',
                'start_menu_removed', 'real_uninstaller_exit_checked', 'original_uninstaller_removed'], unstamped=True)
            setup = 'lain42-vision-studio-windows-setup.exe'
            if (sha(package[setup]) != package[setup + '.sha256'].decode().strip()
                    or receipt('installer-build')['commit'] != commit
                    or receipt('installer-build')['sha256'] != sha(package[setup])):
                raise ValueError('Installer identity differs')
        else:
            browser = receipt('browser')
            if (browser['commit'] != commit or {item['width'] for item in browser['layouts']} != {1440, 1028, 768, 390, 320}
                    or any(item['pageErrors'] or not all(item[k] for k in ('pointer_crop', 'real_crop_pixels', 'colors',
                          'real_svg', 'comparison', 'html_screenshot', 'clear')) for item in browser['layouts'])):
                raise ValueError('Responsive real tool flow evidence incomplete')
        expected = {filename, filename + '.sha256'}
        if platform == 'Windows':
            expected |= {setup, setup + '.sha256'}
        if set(package) != expected:
            raise ValueError('Unexpected distribution members')
        destination.mkdir(parents=True, exist_ok=True)
        for name, data in package.items():
            target = destination / name
            target.write_bytes(data)
            assets.append({'name': name, 'size': len(data), 'digest': 'sha256:' + sha(data)})
        records.append({'platform': platform, 'source_commit': commit, 'outer_package_digest': package_item['digest'],
                        'outer_evidence_digest': evidence_item['digest'], 'manifest_files': len(manifest['files']),
                        'zip_digest': digest, 'all_behavior_and_sdk_checks': True})
    result = {'source_commit': commit, 'source_run': run_id, 'tag': 'vision-studio-v0.1.1-' + commit[:12],
              'records': records, 'assets': sorted(assets, key=lambda item: item['name'])}
    (destination / 'review.json').write_text(json.dumps(result, indent=2), encoding='utf-8')
    return result


def tag_commit(tag):
    try:
        item = github('git/ref/tags/' + tag)['object']
    except urllib.error.HTTPError as error:
        if error.code == 404:
            return None
        raise
    if item['type'] == 'tag':
        item = github('git/tags/' + item['sha'])['object']
    if item['type'] != 'commit':
        raise ValueError('Release tag does not point to a commit')
    return item['sha']


def release_by_tag(tag):
    """The public tag endpoint omits drafts; authenticated release lists include them."""
    try:
        return github('releases/tags/' + tag)
    except urllib.error.HTTPError as error:
        if error.code != 404:
            raise
    matches = []
    page = 1
    while True:
        releases = github('releases?per_page=100&page=' + str(page))
        matches.extend(item for item in releases if item['tag_name'] == tag)
        if len(releases) < 100:
            break
        page += 1
    if len(matches) > 1:
        raise ValueError('Ambiguous release tag; never modify an arbitrary draft')
    return matches[0] if matches else None


def publish(review, destination):
    tag, commit = review['tag'], review['source_commit']
    release = release_by_tag(tag)
    if release and not release['draft']:
        if tag_commit(tag) != commit:
            raise ValueError('Published tag differs from the reviewed source')
        if sorted([{'name': a['name'], 'size': a['size'], 'digest': a['digest']} for a in release['assets']],
                  key=lambda item: item['name']) != review['assets']:
            raise ValueError('Existing public release differs; never overwrite published bytes')
        return
    if not release:
        body = destination / 'release-notes.md'
        body.write_text('图片工坊 0.1.1：裁剪与缩放、主色、图片对比、纯色背景提取、位图转 SVG、图片信息和可信 HTML 截图。'
            '\n\n修复截图取消后的临时文件清理，保留全部原引擎与 SDK 许可。Windows EXE 可安装，完整 ZIP 可解压使用；Linux 使用浏览器。'
            '没有 macOS 或手机安装包。Windows 安装器未签名，需要 WebView2；HTML 仅用于可信内容且需本机 Chrome / Edge。'
            '\n\n全部构建、实际八工具、冻结后的处理、原生窗口与安装／安全卸载在 GitHub Actions 验收。'
            '\n\n源码：' + commit + '\n正式构建：https://github.com/' + REPO + '/actions/runs/' + str(review['source_run']) + '\n', encoding='utf-8')
        subprocess.run(['gh', 'release', 'create', tag, '-R', REPO, '--target', commit, '--draft',
                        '--title', 'Lain42 图片工坊 0.1.1', '--notes-file', str(body)], check=True)
        release = release_by_tag(tag)
    if release is None or not release['draft']:
        raise ValueError('Expected the one intended draft before uploading')
    release_path = 'releases/' + str(release['id'])
    existing_tag = tag_commit(tag)
    if existing_tag is not None and existing_tag != commit:
        raise ValueError('Existing tag differs; never move someone else\'s tag')
    if github(release_path)['target_commitish'] != commit:
        raise ValueError('Draft release target differs')
    for asset in review['assets']:
        existing = github(release_path)['assets']
        found = next((a for a in existing if a['name'] == asset['name']), None)
        if found:
            if found['size'] != asset['size'] or found['digest'] != asset['digest']:
                raise ValueError('Draft asset differs; do not clobber it')
        else:
            subprocess.run(['gh', 'release', 'upload', tag, str(destination / asset['name']), '-R', REPO], check=True)
    actual = github(release_path)
    if sorted([{'name': a['name'], 'size': a['size'], 'digest': a['digest']} for a in actual['assets']],
              key=lambda item: item['name']) != review['assets']:
        raise ValueError('All six GitHub assets must match before publication')
    subprocess.run(['gh', 'release', 'edit', tag, '-R', REPO, '--draft=false', '--latest=false'], check=True)
    if tag_commit(tag) != commit:
        raise ValueError('Published tag does not match reviewed source')


def main():
    if os.environ.get('GITHUB_ACTIONS') != 'true' or os.environ.get('GITHUB_REPOSITORY') != REPO:
        raise SystemExit('Release review and publication run only in this repository Actions')
    parser = argparse.ArgumentParser()
    parser.add_argument('--commit', required=True)
    parser.add_argument('--run', required=True, type=int)
    parser.add_argument('--publish', action='store_true')
    args = parser.parse_args()
    if not re.fullmatch('[a-f0-9]{40}', args.commit) or args.run < 1:
        raise ValueError('Invalid source identity')
    destination = Path('reviewed-release')
    review = inspect(args.commit, args.run, destination)
    if args.publish:
        if os.environ['GITHUB_EVENT_NAME'] != 'workflow_dispatch' or os.environ['GITHUB_REF'] != 'refs/heads/main':
            raise ValueError('Publication requires explicit dispatch of the reviewed main workflow')
        pushes = github('actions/runs?head_sha=' + os.environ['GITHUB_SHA'] + '&event=push&per_page=100')['workflow_runs']
        latest = {}
        for run in sorted(pushes, key=lambda item: item['id'], reverse=True):
            latest.setdefault(run['name'], run)
        for name in ('CI', 'Reviewed Vision Release'):
            if name not in latest or latest[name]['status'] != 'completed' or latest[name]['conclusion'] != 'success':
                raise ValueError('Publisher main commit must pass its own complete CI gates')
        publish(review, destination)
    print(json.dumps({'reviewed_source': args.commit, 'source_run': args.run,
                      'assets': len(review['assets']), 'published': args.publish}))


if __name__ == '__main__':
    main()
