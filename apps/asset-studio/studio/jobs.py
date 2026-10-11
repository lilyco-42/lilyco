"""One bounded local process, with cancellation and downloadable results."""
import asyncio
import json
import zipfile
from pathlib import Path

from .paths import MAX_FILES, MAX_OUTPUT

MAX_LOG = 8 * 1024 * 1024


async def bounded_read(stream):
    result = bytearray()
    while chunk := await stream.read(65536):
        result.extend(chunk)
        if len(result) > MAX_LOG:
            raise ValueError('引擎输出过大，请缩小处理范围')
    return bytes(result).decode('utf-8', errors='replace')


class Jobs:
    def __init__(self):
        self.items = {}

    def busy(self):
        return any(j['state'] in {'queued', 'running'} for j in self.items.values())

    def start(self, identifier, session, root, adapter, request):
        if self.busy():
            raise ValueError('已有任务正在处理，请等待或取消后再试')
        if len(self.items) >= 50:
            raise ValueError('任务记录已满，请删除不用的工作区')
        out = root / 'output' / identifier
        command = adapter.command(root / 'input', out, request)
        out.mkdir()
        job = {'id': identifier, 'session': session, 'state': 'queued',
               'files': [], 'error': None, 'result': None, 'out': out}
        self.items[identifier] = job
        job['task'] = asyncio.create_task(self.run(job, command, adapter))
        return self.public(job)

    def public(self, job):
        return {k: v for k, v in job.items() if k not in {'task', 'out', 'process'}}

    async def run(self, job, command, adapter):
        process = None
        readers = []
        try:
            job['state'] = 'running'
            import subprocess
            process = await asyncio.create_subprocess_exec(
                *command, stdin=asyncio.subprocess.DEVNULL,
                stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE,
                cwd=job['out'], **({'creationflags': subprocess.CREATE_NO_WINDOW} if __import__('sys').platform == 'win32' else {}))
            readers = [asyncio.create_task(bounded_read(s)) for s in (process.stdout, process.stderr)]
            async with asyncio.timeout(120):
                stdout, stderr = await asyncio.gather(*readers)
                await process.wait()
            if process.returncode != 0:
                raise ValueError(stderr.strip()[-1000:] or '引擎无法读取这些文件，请核对格式和配套贴图')
            job['result'] = adapter.result(stdout)
            report = job['out'] / 'result.json'
            report.write_text(json.dumps(job['result'], ensure_ascii=False, indent=2), encoding='utf-8')
            paths = sorted(p for p in job['out'].rglob('*') if p.is_file())
            if len(paths) > MAX_FILES or sum(p.stat().st_size for p in paths) > MAX_OUTPUT:
                raise ValueError('导出结果太大，请只选一个姿势后重试')
            for p in paths:
                if p.is_symlink() or not p.resolve().is_relative_to(job['out'].resolve()):
                    raise ValueError('引擎产生了无效输出路径')
            with zipfile.ZipFile(job['out'] / 'export.zip', 'w', zipfile.ZIP_DEFLATED) as archive:
                for p in paths:
                    archive.write(p, p.relative_to(job['out']).as_posix())
            job['files'] = [{'name': p.relative_to(job['out']).as_posix(), 'size': p.stat().st_size}
                            for p in paths]
            job['state'] = 'done'
        except asyncio.CancelledError:
            job['state'] = 'cancelled'
        except TimeoutError:
            job.update(state='failed', error='处理超过 2 分钟，请减少文件或缩小动画范围')
        except Exception as error:
            job.update(state='failed', error=str(error)[:1000])
        finally:
            if process and process.returncode is None:
                process.kill()
                await process.wait()
            for reader in readers:
                if not reader.done():
                    reader.cancel()
            if readers:
                await asyncio.gather(*readers, return_exceptions=True)

    async def cancel(self, identifier):
        job = self.items[identifier]
        if job['state'] in {'queued', 'running'}:
            job['task'].cancel()
            await asyncio.gather(job['task'], return_exceptions=True)
            job['state'] = 'cancelled'
        return self.public(job)

    async def delete_session(self, session):
        for identifier, job in list(self.items.items()):
            if job['session'] == session:
                await self.cancel(identifier)
                del self.items[identifier]

    async def close(self):
        for identifier in list(self.items):
            await self.cancel(identifier)
