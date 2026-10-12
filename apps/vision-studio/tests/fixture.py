"""Owned synthetic input, only generated in GitHub Actions."""
import io
import os
from pathlib import Path

from PIL import Image, ImageDraw


def picture(changed=False):
    image = Image.new('RGBA', (96, 80), (255, 255, 255, 255))
    draw = ImageDraw.Draw(image)
    draw.rectangle((24, 20, 71, 59), fill=(255, 0, 0, 255))
    if changed:
        draw.rectangle((24, 20, 47, 39), fill=(0, 0, 255, 255))
    buffer = io.BytesIO()
    image.save(buffer, format='PNG')
    return buffer.getvalue()


HTML = b'<!doctype html><html><head><meta charset="utf-8"><style>html,body{margin:0;background:rgb(12,34,56)}</style></head><body>Owned CI page</body></html>'


if __name__ == '__main__':
    if os.environ.get('GITHUB_ACTIONS') != 'true':
        raise SystemExit('Fixtures only in GitHub Actions')
    Path('tests/original.png').write_bytes(picture())
    Path('tests/rebuilt.png').write_bytes(picture(True))
    Path('tests/page.html').write_bytes(HTML)
