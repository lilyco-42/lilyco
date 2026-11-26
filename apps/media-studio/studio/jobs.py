"""HTTP job lifecycle around the original MCP engine; no media reimplementation."""
import asyncio
import json
import shutil
import zipfile

from . import adapters, mcp
from .paths import MAX_OUTPUT


class Jobs:
    def __init__(self):
        self.items = {}

    def public(self, job):
        return {k: v for k, v in job.items() if k not in {'task', 'out'}}

    def start(self, identifier, session, root, tool, params):
        if any(j['state'] in {'queued', 'running'} for j in self.items.values()):
            raise ValueError('已有视频正在处理，请等待或取消')
        if len(self.items) >= 20:
            raise ValueError('任务记录已满，请删除不用的文件')
        binary = adapters.executable()
        adapters.runtime('ffmpeg')
        out = root / 'output' / identifier
        values = adapters.arguments(tool, params, root, out)
        out.mkdir()
        job = {'id': identifier, 'session': session, 'tool': tool, 'state': 'queued',
               'progress': None, 'files': [], 'error': None, 'result': None, 'out': out}
        self.items[identifier] = job
        job['task'] = asyncio.create_task(self.run(job, binary, values))
        return self.public(job)

    async def run(self, job, binary, values):
        engine = guard = None
        try:
            job['state'] = 'running'
            engine = asyncio.create_task(mcp.call(
                binary, 'Ffmpeg', values, lambda p: job.update(progress=p), adapters.environment()))

            async def output_limit():
                while True:
                    await asyncio.sleep(0.2)
                    if sum(p.stat().st_size for p in job['out'].glob('*') if p.is_file()) > MAX_OUTPUT:
                        raise ValueError('输出超过 512 MB，请缩短片段或提高压缩程度')

            guard = asyncio.create_task(output_limit())
            done, _ = await asyncio.wait({engine, guard}, return_when=asyncio.FIRST_COMPLETED)
            if guard in done:
                await guard
            result = await engine
            guard.cancel()
            await asyncio.gather(guard, return_exceptions=True)
            if not isinstance(result, dict) or result.get('success') is not True:
                raise ValueError('视频引擎未完成处理')
            video = job['out'] / ('result.mp4' if values['codec'] == 'h264' else 'result.webm')
            if not video.is_file() or video.is_symlink() or video.stat().st_size > MAX_OUTPUT:
                raise ValueError('输出文件缺失或超过 512 MB')
            metadata = await adapters.probe(video)
            result['media'] = metadata
            job['result'] = result
            (job['out'] / 'result.json').write_text(
                json.dumps(result, ensure_ascii=False, indent=2), encoding='utf-8')
            paths = [video, job['out'] / 'result.json']
            with zipfile.ZipFile(job['out'] / 'export.zip', 'w', zipfile.ZIP_STORED) as archive:
                for path in paths:
                    archive.write(path, path.name)
            job['files'] = [{'name': p.name, 'size': p.stat().st_size} for p in paths]
            job['state'] = 'done'
        except asyncio.CancelledError:
            job['state'] = 'cancelled'
        except TimeoutError:
            job.update(state='failed', error='处理超时，请缩短视频或选择较小尺寸')
        except Exception as error:
            job.update(state='failed', error=str(error)[:1000])
        finally:
            for task in [engine, guard]:
                if task and not task.done():
                    task.cancel()
            await asyncio.gather(*(t for t in [engine, guard] if t), return_exceptions=True)
            if job['state'] in {'failed', 'cancelled'}:
                shutil.rmtree(job['out'], ignore_errors=True)

    async def cancel(self, identifier):
        job = self.items[identifier]
        if job['state'] in {'queued', 'running'}:
            job['task'].cancel()
            await asyncio.gather(job['task'], return_exceptions=True)
            if job['state'] != 'failed':
                job['state'] = 'cancelled'
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
