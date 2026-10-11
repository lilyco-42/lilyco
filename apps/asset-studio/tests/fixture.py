"""Original tiny colour-block character, no third-party game resources."""
import struct
import json
import zlib
from pathlib import Path


def u32(n):
    return struct.pack('>I', n)


def string(s):
    data = s.encode()
    return struct.pack('>H', len(data)) + data


def extract(data):
    return u32(len(data)) + data


def png():
    def chunk(kind, data):
        return u32(len(data)) + kind + data + u32(zlib.crc32(kind + data) & 0xffffffff)
    # A fully original orange/blue 8x8 sprite.
    raw = b''.join(b'\0' + bytes([230, 100, 45, 255] if y < 4 else [50, 110, 230, 255]) * 8 for y in range(8))
    return b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', 8, 8, 8, 6, 0, 0, 0)) + chunk(b'IDAT', zlib.compress(raw)) + chunk(b'IEND', b'')


def fixture():
    image = png()
    pack = u32(1) + bytes([22, 0, 0]) + u32(1) + u32(1) + struct.pack('>dIIII', 1.0, 0, 0, 8, 8) + extract(image)
    header = bytes([1, 0, 0]) + string('wave') + struct.pack('>HHH', 64, 48, 0) + string('') + struct.pack('>H', 0)
    sequence = bytes([0]) + struct.pack('>HHhhhhhH', 64, 48, 0, 0, 0, 0, 0, 0)
    pose = u32(1) + extract(header) + bytes([10]) + extract(sequence) + struct.pack('>H', 2)
    for i, x in enumerate([-80, 80]):
        pose += extract(bytes([1]) + struct.pack('>h', 12) + string('frame' + str(i)))
        pose += struct.pack('>hIdB', 1, 1, 1.0, 0) + string('body')
        pose += struct.pack('>hhhdddHIBB', 10000, x, 0, 1.0, 1.0, 0.0, 0, 0, 0, 0)
    pose += b'\0'
    return bytes.fromhex('7741a8ff') + b'PXLS' + b'%PACK_SECTION%' + extract(pack) + b'%POSE_SECTION%' + extract(pose)


def spine_fixture():
    """Own colour block, Spine 3.8 JSON/atlas; no Editor or game assets."""
    skeleton = {
        'skeleton': {'spine': '3.8.99', 'width': 8, 'height': 8},
        'bones': [{'name': 'root'}, {'name': 'body', 'parent': 'root'}],
        'slots': [{'name': 'body', 'bone': 'body', 'attachment': 'body'}],
        'skins': [{'name': 'default', 'attachments': {'body': {'body': {
            'type': 'region', 'path': 'body', 'width': 8, 'height': 8}}}}],
        'animations': {'wave': {'bones': {'body': {'rotate': [
            {'time': 0, 'angle': -30}, {'time': 1, 'angle': 30},
            {'time': 2, 'angle': -30}]}}}}
    }
    atlas = ('demo.png\nsize: 8,8\nformat: RGBA8888\nfilter: Nearest,Nearest\n'
             'repeat: none\nbody\n  rotate: false\n  xy: 0,0\n  size: 8,8\n'
             '  orig: 8,8\n  offset: 0,0\n  index: -1\n')
    return {'demo.json': json.dumps(skeleton).encode(), 'demo.atlas': atlas.encode(), 'demo.png': png()}


if __name__ == '__main__':
    import sys
    path = Path(sys.argv[1])
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(fixture())
    for name, data in spine_fixture().items():
        (path.parent / name).write_bytes(data)
