"""Fixed CLI adapters. Core parsers/executor stay in their original projects."""
import json
import os
import shutil
import sys
from pathlib import Path

from .paths import child

TABLE_ENDINGS = ('.pxls', '.pxls.dat', '.pxls.bytes')


def executable(name):
    suffix = '.exe' if sys.platform == 'win32' else ''
    # Operators can configure a trusted binary, never through a web request.
    configured = os.environ.get('LAIN42_' + name.upper().replace('-', '_'))
    if configured:
        p = Path(configured).resolve()
        return str(p) if p.is_file() else None
    packaged = Path(getattr(sys, '_MEIPASS', Path(__file__).parents[1])) / 'bin' / (name + suffix)
    if packaged.is_file():
        return str(packaged)
    return shutil.which(name)


class LaicAdapter:
    name = 'aic'

    def available(self):
        return bool(executable('laic'))

    def entries(self, root):
        return sorted(str(p.relative_to(root).as_posix()) for p in root.rglob('*')
                      if p.is_file() and p.name.lower().endswith(TABLE_ENDINGS))

    def command(self, root, out, request):
        exe = executable('laic')
        if not exe:
            raise ValueError('AIC 引擎未安装。请下载包含引擎的桌面完整包')
        source = child(root, request.entry)
        if not source.is_file() or not source.name.lower().endswith(TABLE_ENDINGS):
            raise ValueError('请选择 .pxls / .pxls.dat / .pxls.bytes 角色文件')
        args = [exe]
        if request.action == 'inspect':
            return args + ['poses', '--root', str(source), '--json']
        if request.action == 'sprites':
            return args + ['sprites', '--root', str(source), '--out', str(out), '--pose', request.motion, '--atlas', '--json']
        if request.action == 'textures':
            return args + ['tex', '--root', str(source.parent), '--out', str(out), '--limit', '20', '--json']
        if request.action == 'render':
            return args + ['render', '--root', str(source), '--out', str(out),
                           '--pose', request.motion, '--dir', str(request.direction),
                           '--scale', str(request.scale), '--anim', '--sheet', '--json']
        raise ValueError('不支持的 AIC 操作')

    def result(self, text):
        data = json.loads(text)
        if not isinstance(data, dict):
            raise ValueError('引擎返回了无效结果')
        errors = data.get('errors', [])
        if errors and not any(data.get(k, 0) for k in ('files_parsed', 'frames_written', 'sprites_written', 'textures_decoded')):
            raise ValueError(str(errors[0].get('error', errors[0]))[:1000])
        if data.get('files_parsed') == 0:
            raise ValueError('没有读取到角色表，请核对文件格式')
        if 'frames_written' in data and data['frames_written'] == 0:
            raise ValueError('当前动作或朝向没有帧，请选择动画列表中的动作或换一个朝向')
        return data


class SpineAdapter:
    name = 'spine'

    def available(self):
        return bool(executable('lyco-spine-render'))

    def entries(self, root):
        return sorted(str(p.relative_to(root).as_posix()) for p in root.rglob('*')
                      if p.is_file() and p.suffix.lower() in {'.json', '.skel'})

    def atlas(self, root, source):
        atlas = source.with_suffix('.atlas')
        if not atlas.is_file():
            choices = list(source.parent.glob('*.atlas'))
            if len(choices) != 1:
                raise ValueError('请同时选择同名 .atlas 文件和图集 PNG')
            atlas = choices[0]
        # The renderer follows atlas page names: constrain them to this input folder.
        lines = atlas.read_text(encoding='utf-8-sig').splitlines()
        for i, line in enumerate(lines):
            if line.strip() and (i == 0 or not lines[i - 1].strip()):
                page = child(atlas.parent, line.strip())
                if not page.is_file() or page.suffix.lower() != '.png':
                    raise ValueError('图集引用的 PNG 缺失或路径无效，请把配套 PNG 一起选择')
        return atlas

    def command(self, root, out, request):
        exe = executable('lyco-spine-render')
        if not exe:
            raise ValueError('播放需要你有权使用的 Spine 3.8 引擎。本安装包不包含该运行时')
        source = child(root, request.entry)
        if not source.is_file() or source.suffix.lower() not in {'.skel', '.json'}:
            raise ValueError('请选择 Spine .json 或 .skel 骨架文件')
        atlas = self.atlas(root, source)
        if request.action == 'inspect':
            return [exe, 'rig', str(atlas), str(source)]
        if request.action != 'render':
            raise ValueError('Spine 支持读取动画列表和播放导出')
        return [exe, 'animate', str(atlas), str(source), str(out / 'animation.gif'),
                '--animation', request.motion, '--frames', '24', '--fps', '12', '--size', '512x512']

    def result(self, text):
        motions = []
        if '--animations--' in text:
            for line in text.split('--animations--', 1)[1].splitlines():
                fields = line.split('\t')
                if len(fields) == 2:
                    motions.append({'title': fields[0], 'seconds': fields[1]})
        return {'motions': motions, 'message': text[:4000]}


ADAPTERS = {'aic': LaicAdapter(), 'spine': SpineAdapter()}
