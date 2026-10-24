"""Convert original Lyco artwork and inspect actual Office bundles in Actions."""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parent
WORKSPACE = ROOT.parent
SOURCE_SHA = '668059834636febc2df2fa467d5009c922bd3b973ef61370a3be9e7cf66198e1'
LICENSE_SHA = 'bf26e5109ef6b295b99a170992247cb06f7fec3168877dbe56fd6775301c2618'
ICON_SHA = '4dd1c08f4c444cc456b64217fa1b40bc30e21199f0ad66ee2d7a9aebdc180b01'


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def helper():
    spec = importlib.util.spec_from_file_location(
        'desktop_icons', WORKSPACE / 'apps/desktop-common/desktop_icons.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def check_source():
    source = ROOT / 'resources/icon-source.png'
    pin = json.loads((ROOT / 'resources/icon-source.json').read_text(encoding='utf-8'))
    if pin['sha256'] != SOURCE_SHA or digest(source) != SOURCE_SHA:
        raise ValueError('Office artwork must retain the reviewed original source')
    # Complete notice bytes, not a short SPDX label. Windows git may use CRLF.
    actual = (ROOT / 'resources/ICON-LICENSE.md').read_text(encoding='utf-8')
    if hashlib.sha256(actual.encode('utf-8')).hexdigest() != LICENSE_SHA:
        raise ValueError('Office must carry the complete unchanged Lyco notice')
    if digest(ROOT / 'frontend/app-icon.png') != SOURCE_SHA:
        raise ValueError('WebView favicon is not the reviewed Lyco image')
    nsis = json.loads((ROOT / 'tauri.conf.json').read_text(encoding='utf-8'))['bundle']['windows']['nsis']
    if nsis.get('installerIcon') != 'icons/icon.ico' or nsis.get('uninstallerIcon') != 'icons/icon.ico':
        raise ValueError('Actual NSIS install/uninstall must use the reviewed multi-size icon')
    return source


def prepare():
    source = check_source()
    # Use the official Tauri generator for all desktop platform formats.
    # No build or conversion is allowed outside the workflow.
    subprocess.run([
        'npx.cmd' if sys.platform == 'win32' else 'npx',
        '--yes', '@tauri-apps/cli@2.12.1', 'icon', str(source),
        '--output', str(ROOT / 'icons'),
    ], check=True, timeout=300)
    receipt = helper().prepare_icon(ROOT)
    if receipt['ico_sha256'] != ICON_SHA:
        raise ValueError('Windows ICO does not match the shared reviewed recipe')
    shutil.copyfile(ROOT / 'resources/app.ico', ROOT / 'icons/icon.ico')
    from PIL import Image
    with Image.open(source) as image:
        rgba = image.convert('RGBA')
        for name, size in [('32x32.png', 32), ('128x128.png', 128),
                           ('128x128@2x.png', 256), ('icon.png', 512)]:
            rgba.resize((size, size), Image.Resampling.LANCZOS).save(ROOT / 'icons' / name)
    receipt['tauri_formats'] = {
        name: digest(ROOT / 'icons' / name)
        for name in ['32x32.png', '128x128.png', '128x128@2x.png',
                     'icon.png', 'icon.ico', 'icon.icns']
    }
    (ROOT / 'resources/icon-build.json').write_text(
        json.dumps(receipt, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(receipt))



def verify_bundle_notices(base):
    for name in ['icon-source.png', 'icon-source.json', 'ICON-LICENSE.md',
                 'FRONTEND-LICENSES.json', 'RUST-LICENSES.json']:
        copies = list(Path(base).rglob(name))
        if len(copies) != 1:
            raise ValueError('Bundle must contain one original resource: ' + name)
        if name == 'ICON-LICENSE.md':
            sha = hashlib.sha256(copies[0].read_text(encoding='utf-8').encode('utf-8')).hexdigest()
            if sha != LICENSE_SHA:
                raise ValueError('Packaged Lyco notice is incomplete')
        elif digest(copies[0]) != digest(ROOT / 'resources' / name):
            raise ValueError('Packaged original resource changed: ' + name)
    return True


def verify_native_window_icon():
    from PIL import Image
    snapshot = ROOT / 'resources/native-window-icon.png'
    subprocess.run([
        'pwsh', '-NoProfile', '-File', str(ROOT / 'native-icon.ps1'),
        '-Binary', str(WORKSPACE / 'target/release/lilyco-office.exe'),
        '-Output', str(snapshot),
    ], check=True, timeout=125)
    with Image.open(snapshot) as captured, Image.open(ROOT / 'icons/icon.ico') as icon:
        actual = captured.convert('RGBA')
        if actual.size not in icon.ico.sizes():
            raise ValueError('Native window returned an unexpected icon size')
        expected = icon.ico.getimage(actual.size).convert('RGBA')
        comparisons = [expected]
        # Tauri may select its PNG rather than the ICO for a window icon.
        for name in ['32x32.png', '128x128.png', '128x128@2x.png', 'icon.png']:
            with Image.open(ROOT / 'icons' / name) as png:
                if png.size == actual.size:
                    comparisons.append(png.convert('RGBA'))
        if actual.getchannel('A').getextrema() != (0, 255):
            raise ValueError('Native icon lost the original alpha silhouette')
        for expected in comparisons:
            pixels = [(e, a) for e, a in zip(expected.getdata(), actual.getdata()) if e[3] == 255]
            if len(pixels) < 50 or any(a[3] != 255 for _, a in pixels):
                continue
            errors = [abs(e[c] - a[c]) for e, a in pixels for c in range(3)]
            # Allow only single-level raster rounding on opaque pixels, never a
            # substituted default logo. Alpha edge pixels are premultiplied by GDI.
            if max(errors) <= 4 and sum(errors) / len(errors) <= 1:
                return {'native_icon_matches_reviewed_pixels': True,
                        'normal_native_close': True, 'size': list(actual.size),
                        'opaque_pixels_compared': len(pixels),
                        'snapshot_sha256': digest(snapshot)}
    raise ValueError('Actual native window icon is not the reviewed Lyco image')

def verify(platform):
    check_source()
    receipt = json.loads((ROOT / 'resources/icon-build.json').read_text(encoding='utf-8'))
    for name, sha in receipt['tauri_formats'].items():
        if digest(ROOT / 'icons' / name) != sha:
            raise ValueError('Prepared platform icon was replaced: ' + name)
    if digest(ROOT / 'icons/icon.ico') != ICON_SHA:
        raise ValueError('Windows resource icon differs from the reviewed icon')
    config = json.loads((ROOT / 'tauri.conf.json').read_text(encoding='utf-8'))
    version = config['version']
    report = {'source_sha256': SOURCE_SHA, 'version': version,
              'platform': platform, 'source_and_notice_verified': True}
    if platform == 'windows':
        binary = WORKSPACE / 'target/release/lilyco-office.exe'
        report['application'] = helper().verify_embedded_icon(binary, ROOT / 'icons/icon.ico')
        report['native_window'] = verify_native_window_icon()
        installers = list((WORKSPACE / 'target').glob('**/bundle/nsis/*-setup.exe'))
        if len(installers) != 1:
            raise ValueError('Expected exactly one real NSIS installer')
        report['installer'] = helper().verify_embedded_icon(installers[0], ROOT / 'icons/icon.ico')
        msis = list((WORKSPACE / 'target').glob('**/bundle/msi/*.msi'))
        if len(msis) != 1:
            raise ValueError('Expected exactly one real MSI installer')
        report['msi_sha256'] = digest(msis[0])
        # MSI presence alone does not establish native install/uninstall acceptance.
        report['msi_installation_acceptance'] = 'pending'
        report['windows_installed_resource_acceptance'] = 'pending'
    elif platform == 'macos':
        icons = list((WORKSPACE / 'target').glob('**/bundle/macos/*.app/Contents/Resources/icon.icns'))
        if len(icons) != 1 or digest(icons[0]) != receipt['tauri_formats']['icon.icns']:
            raise ValueError('Actual macOS app lacks the prepared Lyco ICNS')
        report['macos_icon_sha256'] = digest(icons[0])
        report['bundled_original_resources_verified'] = verify_bundle_notices(icons[0].parent)
    elif platform == 'linux':
        debs = list((WORKSPACE / 'target').glob('**/bundle/deb/*.deb'))
        if len(debs) != 1:
            raise ValueError('Expected exactly one real Debian bundle')
        destination = ROOT / 'icon-deb-inspection'
        subprocess.run(['dpkg-deb', '--extract', str(debs[0]), str(destination)],
                       check=True, timeout=60)
        expected = {receipt['tauri_formats'][name] for name in
                    ['32x32.png', '128x128.png', '128x128@2x.png', 'icon.png']}
        actual = {digest(path) for path in destination.glob('usr/share/icons/**/*.png')}
        if not expected.issubset(actual):
            raise ValueError('Actual Debian bundle lacks the prepared Lyco PNG set')
        report['debian_icon_payload_verified'] = True
        report['bundled_original_resources_verified'] = verify_bundle_notices(destination)
    (ROOT / 'resources/icon-package-evidence.json').write_text(
        json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    if os.environ.get('GITHUB_ACTIONS') != 'true':
        raise SystemExit('Office icon conversion and package checks only run in GitHub Actions')
    parser = argparse.ArgumentParser()
    parser.add_argument('action', choices=['prepare', 'verify'])
    parser.add_argument('--platform', choices=['windows', 'macos', 'linux'])
    args = parser.parse_args()
    if args.action == 'prepare':
        prepare()
    elif not args.platform:
        parser.error('verify requires --platform')
    else:
        verify(args.platform)
