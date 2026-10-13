"""Original Lain42 artwork -> multi-size ICO and verified PE resources, in CI only."""
import hashlib
import json
import os
from pathlib import Path
import struct

SIZES = (16, 24, 32, 48, 64, 128, 256)


def prepare_icon(root):
    if os.environ.get('GITHUB_ACTIONS') != 'true':
        raise RuntimeError('Icon conversion only in GitHub Actions')
    from PIL import Image
    root = Path(root)
    folder = root / 'resources'
    pin = json.loads((folder / 'icon-source.json').read_text(encoding='utf-8'))
    source = folder / 'icon-source.png'
    if hashlib.sha256(source.read_bytes()).hexdigest() != pin['sha256']:
        raise ValueError('Original icon source changed without its pin')
    with Image.open(source) as original:
        if original.format != 'PNG' or original.width != original.height or original.width < 512:
            raise ValueError('Need a complete square PNG icon source')
        image = original.convert('RGBA')
        if image.getchannel('A').getextrema() != (0, 255):
            raise ValueError('Icon must retain transparent and visible pixels')
        image.save(folder / 'app.ico', format='ICO', sizes=[(s, s) for s in SIZES], bitmap_format='bmp')
        image.resize((256, 256), Image.Resampling.LANCZOS).save(folder / 'app-icon.png')
    with Image.open(folder / 'app.ico') as icon:
        if icon.ico.sizes() != {(s, s) for s in SIZES}:
            raise ValueError('Incomplete Windows icon sizes')
        for size in SIZES:
            if icon.ico.getimage((size, size)).convert('RGBA').getchannel('A').getextrema() != (0, 255):
                raise ValueError('An icon size lost transparency')
    return {'source_sha256': pin['sha256'], 'sizes': list(SIZES),
            'ico_sha256': hashlib.sha256((folder / 'app.ico').read_bytes()).hexdigest(),
            'png_sha256': hashlib.sha256((folder / 'app-icon.png').read_bytes()).hexdigest(),
            'transparent_source_retained': True}


def ico_frames(path):
    data = Path(path).read_bytes()
    reserved, kind, count = struct.unpack_from('<HHH', data)
    if reserved != 0 or kind != 1 or count != len(SIZES):
        raise ValueError('Incomplete expected icon directory')
    frames = []
    for i in range(count):
        width, height, _, _, _, _, length, offset = struct.unpack_from('<BBBBHHII', data, 6 + 16 * i)
        if offset < 6 + 16 * count or offset + length > len(data):
            raise ValueError('Truncated expected icon frame')
        frames.append(((width or 256, height or 256), data[offset:offset + length]))
    return frames


def verify_embedded_icon(executable, icon):
    if os.environ.get('GITHUB_ACTIONS') != 'true':
        raise RuntimeError('PE icon checks only in GitHub Actions')
    import pefile
    expected = ico_frames(icon)
    if {size for size, _ in expected} != {(s, s) for s in SIZES}:
        raise ValueError('Expected icon has the wrong dimensions')
    pe = pefile.PE(str(executable))
    try:
        entries = pe.DIRECTORY_ENTRY_RESOURCE.entries
        images = {}
        for entry in entries:
            if entry.id == 3:
                for image in entry.directory.entries:
                    payload = image.directory.entries[0].data.struct
                    images[image.id] = pe.get_data(payload.OffsetToData, payload.Size)
        for entry in entries:
            if entry.id != 14:
                continue
            for group in entry.directory.entries:
                for language in group.directory.entries:
                    payload = language.data.struct
                    data = pe.get_data(payload.OffsetToData, payload.Size)
                    _, kind, count = struct.unpack_from('<HHH', data)
                    if kind != 1 or count != len(expected):
                        continue
                    actual = []
                    for i in range(count):
                        width, height, _, _, _, _, size, image_id = struct.unpack_from('<BBBBHHIH', data, 6 + 14 * i)
                        body = images.get(image_id)
                        if body is None or len(body) != size:
                            raise ValueError('Broken embedded icon reference')
                        actual.append(((width or 256, height or 256), body))
                    if set(actual) == set(expected):
                        return {'all_original_icon_frames_embedded': True,
                                'executable_sha256': hashlib.sha256(Path(executable).read_bytes()).hexdigest(),
                                'icon_sha256': hashlib.sha256(Path(icon).read_bytes()).hexdigest()}
        raise ValueError('Executable does not embed the original multi-size icon')
    finally:
        pe.close()
