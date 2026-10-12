"""Fixed MCP tools and owned paths; semantic validation stays in CommandSchema."""
import os
from pathlib import Path
import shutil
import sys

from PIL import Image

MAX_PIXELS = 16_000_000
TOOLS = {
    'image-info': set(),
    'crop': {'region', 'scale'},
    'resize': {'width', 'height'},
    'dominant-colors': {'top', 'tolerance', 'region', 'candidates'},
    'pixel-diff': {'grid', 'top'},
    'extract-foreground': {'mode', 'region', 'exclude_color', 'exclude_tol'},
    'trace': {'scale', 'color', 'polygon'},
    'html-screenshot': {'width', 'height'},
}


def executable():
    configured = os.environ.get('LAIN42_VISION_CLI')
    root = Path(getattr(sys, '_MEIPASS', Path(__file__).resolve().parents[1]))
    path = Path(configured) if configured else root / 'bin' / ('lvision.exe' if sys.platform == 'win32' else 'lvision')
    if not path.is_file():
        raise ValueError('图片引擎未安装，请下载完整桌面包')
    return path.resolve()


def browser_available():
    configured = os.environ.get('CHROME_PATH')
    if configured:
        return Path(configured).is_file()
    if sys.platform == 'win32':
        return any((Path(os.environ.get(variable, '')) / suffix).is_file()
                   for variable, suffix in [
                       ('PROGRAMFILES', 'Google/Chrome/Application/chrome.exe'),
                       ('PROGRAMFILES(X86)', 'Microsoft/Edge/Application/msedge.exe'),
                       ('PROGRAMFILES', 'Microsoft/Edge/Application/msedge.exe')])
    return any(shutil.which(name) for name in ['chromium', 'chromium-browser', 'google-chrome'])


def arguments(tool, params, root, out):
    if tool not in TOOLS or set(params) - TOOLS[tool]:
        raise ValueError('不支持的工具或参数；不能指定输出路径或执行命令')
    values = {k: v for k, v in params.items() if v is not None and v != ''}
    originals = list((root / 'input').glob('original.*'))
    if len(originals) != 1:
        raise ValueError('请先选择原始文件')
    image = originals[0]
    if tool == 'html-screenshot':
        if image.suffix not in {'.html', '.htm'}:
            raise ValueError('网页截图需要一个本地 HTML 文件')
        if not browser_available():
            raise ValueError('网页截图需要本机安装 Chrome 或 Edge')
        if int(values.get('width', 1280)) * int(values.get('height', 800)) > MAX_PIXELS:
            raise ValueError('截图超过 1600 万像素')
        return {**values, 'source': str(image), 'out': str(out / 'screenshot.png')}
    if image.suffix not in {'.png', '.jpg', '.jpeg', '.webp'}:
        raise ValueError('此工具需要 PNG、JPEG 或 WebP 图片')
    with Image.open(image) as opened:
        width, height = opened.size
    if tool == 'resize':
        w, h = int(values.get('width', 0)), int(values.get('height', 0))
        area = w * h if w and h else ((w * w * height / width) if w else (h * h * width / height))
        if area > MAX_PIXELS:
            raise ValueError('输出超过 1600 万像素')
    if tool in {'crop', 'trace'}:
        scale = int(values.get('scale', 1 if tool == 'crop' else 0))
        if not scale and tool == 'trace':
            shortest = min(width, height)
            scale = min(16, max(1, (256 + shortest - 1) // shortest))
        if width * height * scale * scale > MAX_PIXELS:
            raise ValueError('放大后超过 1600 万像素，请缩小输入或倍率')
    if tool == 'pixel-diff':
        rebuilt = list((root / 'input').glob('rebuilt.*'))
        if len(rebuilt) != 1:
            raise ValueError('图片对比需要第二张图片')
        return {**values, 'original': str(image), 'rebuilt': str(rebuilt[0]),
                'out_heatmap': str(out / 'heatmap.png')}
    if tool == 'extract-foreground':
        return {**values, 'images': [str(image)], 'out': str(out / 'foreground.png')}
    values['image'] = str(image)
    if tool in {'crop', 'resize', 'trace'}:
        values['out'] = str(out / ('traced.svg' if tool == 'trace' else 'result.png'))
    return values
