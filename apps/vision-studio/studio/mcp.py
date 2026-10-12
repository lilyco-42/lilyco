"""Bounded JSONL transport to the original executor; no domain reimplementation."""
import asyncio
import json
import subprocess
import sys

from .profile_cleanup import ScreenshotProfiles

MAX_LOG = 8 * 1024 * 1024


async def call(binary, tool, arguments, progress):
    profiles = ScreenshotProfiles(tool, arguments)
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
    containment = None
    try:
        if sys.platform == 'win32':
            from .windows_process_job import WindowsProcessJob
            containment = WindowsProcessJob(process.pid)
        messages = [
            {'jsonrpc': '2.0', 'id': 1, 'method': 'initialize', 'params': {
                'protocolVersion': '2024-11-05', 'capabilities': {},
                'clientInfo': {'name': 'vision-studio', 'version': '0.1.1'}}},
            {'jsonrpc': '2.0', 'method': 'notifications/initialized'},
            {'jsonrpc': '2.0', 'id': 0, 'method': 'tools/list', 'params': {}},
        ]
        for message in messages:
            process.stdin.write((json.dumps(message, ensure_ascii=False) + '\n').encode())
        await process.stdin.drain()
        result = None
        size = 0
        async with asyncio.timeout(120):
            # Ask the original server for canonical names/field spelling. Its
            # App derive currently uses PascalCase tools and kebab-case fields.
            # No copied schema or second domain validator lives in this wrapper.
            while line := await process.stdout.readline():
                size += len(line)
                if size > MAX_LOG:
                    raise ValueError('引擎工具清单过大')
                listing = json.loads(line)
                if listing.get('id') != 0:
                    continue
                if 'error' in listing:
                    raise ValueError('无法读取引擎工具清单')
                canonical, wire_args = resolve_tool(listing['result']['tools'], tool, arguments)
                request = {'jsonrpc': '2.0', 'id': 2, 'method': 'tools/call', 'params': {
                    'name': canonical, 'arguments': wire_args, '_meta': {'progressToken': 'local-job'}}}
                process.stdin.write((json.dumps(request, ensure_ascii=False) + '\n').encode())
                await process.stdin.drain()
                # EOF joins the original Rust in-flight executor threads.
                process.stdin.close()
                break
            else:
                raise ValueError('引擎未返回工具清单')
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
        try:
            if sys.platform == 'win32':
                if containment:
                    await containment.stop()
                elif process.returncode is None:
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
        finally:
            if not reader.done():
                reader.cancel()
            await asyncio.gather(reader, return_exceptions=True)
            await profiles.close()


def resolve_tool(tools, requested, arguments):
    def normalized(name):
        return name.replace('-', '').replace('_', '').lower()

    exact = [item for item in tools if item['name'] == requested]
    matches = exact or [item for item in tools if normalized(item['name']) == normalized(requested)]
    if len(matches) != 1:
        raise ValueError('工具版本不匹配，请下载完整的同版本桌面包')
    schema = matches[0]
    properties = schema['inputSchema']['properties']
    result = {}
    for key, value in arguments.items():
        canonical = key if key in properties else key.replace('_', '-')
        if canonical not in properties or canonical in result:
            raise ValueError('工具参数版本不匹配，请下载完整的同版本桌面包')
        result[canonical] = value
    return schema['name'], result
