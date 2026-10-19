"""Fixed local files and presets; the original lffmpeg remains the only encoder."""
import asyncio
import json
import math
import os
from pathlib import Path
import shutil
import subprocess
import sys

MAX_PIXELS = 3840 * 2160
MAX_DURATION = 7200
INPUT_SUFFIXES = {'.mp4', '.mov', '.m4v', '.webm', '.mkv'}
PRESETS = {'compress', 'convert', 'resize', 'trim'}


def bundle_root():
    return Path(getattr(sys, '_MEIPASS', Path(__file__).resolve().parents[1]))


def executable():
    configured = os.environ.get('LAIN42_MEDIA_CLI') if os.environ.get('GITHUB_ACTIONS') == 'true' else None
    path = Path(configured) if configured else bundle_root() / 'bin' / (
        'lffmpeg.exe' if sys.platform == 'win32' else 'lffmpeg')
    if not path.is_file():
        raise ValueError('视频引擎未安装，请使用完整桌面包')
    return path.resolve()


def environment():
    values = dict(os.environ)
    bundled = bundle_root() / 'bin'
    values['PATH'] = str(bundled) + os.pathsep + values.get('PATH', '')
    return values


def runtime(name):
    binary = shutil.which(name, path=environment()['PATH'])
    if not binary:
        raise ValueError('媒体运行时未安装，请使用完整桌面包')
    return binary


async def probe(path):
    process = await asyncio.create_subprocess_exec(
        runtime('ffprobe'), '-v', 'error', '-protocol_whitelist', 'file,pipe',
        '-select_streams', 'v:0', '-show_entries',
        'stream=width,height,codec_name:format=duration,format_name,size',
        '-of', 'json', str(path), stdout=asyncio.subprocess.PIPE,
        stderr=asyncio.subprocess.PIPE,
        **({'creationflags': subprocess.CREATE_NO_WINDOW} if sys.platform == 'win32' else {}))
    try:
        async with asyncio.timeout(20):
            stdout, _ = await process.communicate()
        if process.returncode or len(stdout) > 65536:
            raise ValueError('无法读取视频，请核对文件格式')
        data = json.loads(stdout)
        stream = data.get('streams', [])[0]
        container = data['format']
        allowed = {'mov', 'mp4', 'm4a', '3gp', '3g2', 'mj2', 'matroska', 'webm'}
        if not set(container['format_name'].split(',')) <= allowed:
            raise ValueError('只接受 MP4、MOV、M4V、WebM 或 MKV 容器，不接受播放列表')
        width, height = int(stream['width']), int(stream['height'])
        duration = float(container['duration'])
        if not (width > 0 and height > 0 and width * height <= MAX_PIXELS
                and math.isfinite(duration) and 0 < duration <= MAX_DURATION):
            raise ValueError('支持不超过 4K 像素总量、2 小时的视频')
        return {'width': width, 'height': height, 'duration': duration,
                'codec': stream['codec_name'], 'bytes': path.stat().st_size}
    except (KeyError, IndexError, TypeError, json.JSONDecodeError) as error:
        raise ValueError('未找到可读取的视频画面和时长') from error
    finally:
        if process.returncode is None:
            process.kill()
        await process.wait()


def arguments(tool, params, root, out):
    if tool not in PRESETS or set(params) - {'format', 'crf', 'width', 'height', 'start', 'duration'}:
        raise ValueError('不支持的操作或参数；不能指定命令、输入路径或输出路径')
    sources = list((root / 'input').glob('original.*'))
    if len(sources) != 1:
        raise ValueError('请先选择视频')
    output_format = params.get('format', 'mp4')
    if output_format not in {'mp4', 'webm'}:
        raise ValueError('请选择 MP4 或 WebM')
    values = {'input': str(sources[0]), 'output': str(out / ('result.' + output_format)),
              'codec': 'h264' if output_format == 'mp4' else 'vp9',
              'audio': 'aac' if output_format == 'mp4' else 'opus',
              'preset': 'fast', 'crf': 23, 'overwrite': False}
    if tool == 'compress':
        quality = params.get('crf', 28)
        if type(quality) is not int or not 20 <= quality <= 40:
            raise ValueError('压缩质量请选择 20–40')
        values['crf'] = quality
    elif tool == 'resize':
        width, height = params.get('width'), params.get('height')
        if width is None and height is None:
            raise ValueError('至少填写宽度或高度')
        for name, size in [('width', width), ('height', height)]:
            if size is not None:
                if type(size) is not int or not 2 <= size <= 3840 or size % 2:
                    raise ValueError('尺寸须为 2–3840 之间的偶数；另一边可以留空')
                values[name] = size
        if width and height and width * height > MAX_PIXELS:
            raise ValueError('输出超过 4K 像素总量')
        metadata = json.loads((root / 'media.json').read_text(encoding='utf-8'))
        implied_width = width or math.ceil(height * metadata['width'] / metadata['height'] / 2) * 2
        implied_height = height or math.ceil(width * metadata['height'] / metadata['width'] / 2) * 2
        if implied_width * implied_height > MAX_PIXELS:
            raise ValueError('等比缩放后的输出超过 4K 像素总量')
    elif tool == 'trim':
        for name in ['start', 'duration']:
            value = params.get(name)
            if type(value) not in {int, float} or not math.isfinite(value):
                raise ValueError('请填写有效的开始秒数和片段时长')
            if not (0 <= value <= MAX_DURATION) or (name == 'duration' and value <= 0):
                raise ValueError('开始位置须非负，片段时长须大于零且不超过 2 小时')
            values[name] = value
    return values
