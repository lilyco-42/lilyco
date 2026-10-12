"""Task lifecycle around the original MCP transport and its real output files."""
import asyncio
import json
import shutil
import zipfile

from PIL import Image

from . import adapters, mcp
from .paths import MAX_FILES, MAX_OUTPUT


class Jobs:
    def __init__(self):
        self.items = {}

    def start(self, identifier, session, root, tool, params):
        if any(j['state'] in {'queued', 'running'} for j in self.items.values()):
            raise ValueError('已有任务正在处理，请等待或取消')
        if len(self.items) >= 50:
            raise ValueError('任务记录已满，请清空工作区')
        binary = adapters.executable()
        out = root / 'output' / identifier
        arguments = adapters.arguments(tool, params, root, out)
        out.mkdir()
        job = {'id': identifier, 'session': session, 'tool': tool, 'state': 'queued',
               'progress': None, 'files': [], 'error': None, 'result': None, 'out': out}
        self.items[identifier] = job
        job['task'] = asyncio.create_task(self.run(job, binary, arguments))
        return self.public(job)

    def public(self, job):
        return {k: v for k, v in job.items() if k not in {'task', 'out'}}

    async def run(self, job, binary, arguments):
        try:
            job['state'] = 'running'
            result = await mcp.call(binary, job['tool'], arguments,
                                    lambda p: job.update(progress=p))
            if not isinstance(result, dict) or result.get('ok') is False:
                raise ValueError('引擎未完成处理')
            job['result'] = result
            (job['out'] / 'result.json').write_text(
                json.dumps(result, ensure_ascii=False, indent=2), encoding='utf-8')
            paths = sorted(p for p in job['out'].rglob('*') if p.is_file())
            if len(paths) > MAX_FILES or sum(p.stat().st_size for p in paths) > MAX_OUTPUT:
                raise ValueError('导出结果过大')
            for path in paths:
                if path.is_symlink() or not path.resolve().is_relative_to(job['out'].resolve()):
                    raise ValueError('无效输出路径')
                if path.suffix == '.png':
                    with Image.open(path) as image:
                        if image.width * image.height > adapters.MAX_PIXELS:
                            raise ValueError('输出超过 1600 万像素')
                        image.verify()
                if path.suffix == '.svg':
                    text = path.read_text(encoding='utf-8').lower()
                    if '<svg' not in text or any(x in text for x in
                                                ['<script', '<foreignobject', 'href=', 'onload=']):
                        raise ValueError('SVG 输出包含不支持的活动内容')
            with zipfile.ZipFile(job['out'] / 'export.zip', 'w', zipfile.ZIP_DEFLATED) as archive:
                for path in paths:
                    archive.write(path, path.relative_to(job['out']).as_posix())
            job['files'] = [{'name': p.relative_to(job['out']).as_posix(), 'size': p.stat().st_size}
                            for p in paths]
            job['state'] = 'done'
        except asyncio.CancelledError:
            job['state'] = 'cancelled'
        except TimeoutError:
            job.update(state='failed', error='处理超过 2 分钟，请缩小文件后重试')
        except Exception as error:
            job.update(state='failed', error=str(error)[:1000])
        finally:
            if job['state'] in {'failed', 'cancelled'}:
                shutil.rmtree(job['out'], ignore_errors=True)

    async def cancel(self, identifier):
        job = self.items[identifier]
        if job['state'] in {'queued', 'running'}:
            job['task'].cancel()
            await asyncio.gather(job['task'], return_exceptions=True)
            if job['state'] != 'failed':
                job['state'] = 'cancelled'
            # A queued coroutine may be cancelled before its finally block runs.
            shutil.rmtree(job['out'], ignore_errors=True)
        return self.public(job)

    async def delete_session(self, session):
        for identifier, job in list(self.items.items()):
            if job['session'] == session:
                await self.cancel(identifier)
                del self.items[identifier]

    async def close(self):
        for identifier in list(self.items):
            await self.cancel(identifier)
