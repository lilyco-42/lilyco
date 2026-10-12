"""Bounded JSONL transport to the original executor; no domain reimplementation."""
import asyncio
import json
import subprocess
import sys

MAX_LOG = 8 * 1024 * 1024


async def call(binary, tool, arguments, progress):
    process = await asyncio.create_subprocess_exec(
        str(binary), '--mcp', stdin=asyncio.subprocess.PIPE,
        stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE,
        limit=MAX_LOG, **({'creationflags': subprocess.CREATE_NO_WINDOW}
                          if sys.platform == 'win32' else {'start_new_session': True}))

    async def stderr_read():
        size = 0
        tail = b''
        while chunk := await process.stderr.read(65536):
            size += len(chunk)
            if size > MAX_LOG:
                raise ValueError('引擎日志过大')
            tail = (tail + chunk)[-4096:]
        return tail.decode('utf-8', errors='replace')

    reader = asyncio.create_task(stderr_read())
    try:
        messages = [
            {'jsonrpc': '2.0', 'id': 1, 'method': 'initialize', 'params': {
                'protocolVersion': '2024-11-05', 'capabilities': {},
                'clientInfo': {'name': 'vision-studio', 'version': '0.1.0'}}},
            {'jsonrpc': '2.0', 'method': 'notifications/initialized'},
            {'jsonrpc': '2.0', 'id': 2, 'method': 'tools/call', 'params': {
                'name': tool, 'arguments': arguments, '_meta': {'progressToken': 'local-job'}}},
        ]
        for message in messages:
            process.stdin.write((json.dumps(message, ensure_ascii=False) + '\n').encode())
        await process.stdin.drain()
        # The Rust server joins its in-flight executor threads after EOF.
        process.stdin.close()
        result = None
        size = 0
        async with asyncio.timeout(120):
            while line := await process.stdout.readline():
                size += len(line)
                if size > MAX_LOG:
                    raise ValueError('引擎结果过大')
                message = json.loads(line)
                if message.get('method') == 'notifications/progress':
                    progress(message.get('params', {}))
                if message.get('id') != 2:
                    continue
                if 'error' in message:
                    raise ValueError(message['error'].get('message', '引擎拒绝请求'))
                payload = message.get('result', {})
                content = '\n'.join(c.get('text', '') for c in payload.get('content', [])
                                    if c.get('type') == 'text')
                if payload.get('isError'):
                    raise ValueError(content[:1000])
                result = json.loads(content)
            await process.wait()
            stderr = await reader
        if process.returncode != 0 or result is None:
            raise ValueError(stderr[-1000:] or '引擎没有返回结果')
        return result
    finally:
        if process.returncode is None:
            if sys.platform == 'win32':
                killer = await asyncio.create_subprocess_exec(
                    'taskkill', '/PID', str(process.pid), '/T', '/F',
                    stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL,
                    creationflags=subprocess.CREATE_NO_WINDOW)
                await killer.wait()
            else:
                import os
                import signal
                try:
                    os.killpg(process.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
            await process.wait()
        if not reader.done():
            reader.cancel()
        await asyncio.gather(reader, return_exceptions=True)
