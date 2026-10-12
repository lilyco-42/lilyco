"""Real Rust MCP processing, byte-level outputs and local HTTP boundaries."""
import asyncio
import io
import json
import os
from pathlib import Path
import sys
import time
import unittest
from unittest.mock import patch
import zipfile

from fastapi.testclient import TestClient
from PIL import Image

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from studio.app import create_app
from studio.jobs import Jobs
from studio.paths import Workspaces, child
from fixture import HTML, picture

if os.environ.get('GITHUB_ACTIONS') != 'true':
    raise SystemExit('Product tests only in GitHub Actions')


class VisionApi(unittest.TestCase):
    def setUp(self):
        self.app = create_app()
        self.client = TestClient(self.app, base_url='http://127.0.0.1')
        self.client.__enter__()
        self.assertEqual(self.client.get('/').status_code, 200)

    def tearDown(self):
        root = self.app.state.workspaces.root
        self.client.__exit__(None, None, None)
        self.assertFalse(root.exists())

    def workspace(self, second=False, html=False):
        files = {'original': ('page.html', HTML, 'text/html') if html else ('photo.png', picture(), 'image/png')}
        if second:
            files['rebuilt'] = ('second.png', picture(True), 'image/png')
        response = self.client.post('/api/workspaces', files=files)
        self.assertEqual(response.status_code, 200, response.text)
        return response.json()['id']

    def run_tool(self, session, tool, params=None, success=True):
        response = self.client.post('/api/jobs', json={'session': session, 'tool': tool, 'params': params or {}})
        self.assertEqual(response.status_code, 200, response.text)
        job = response.json()
        for _ in range(600):
            job = self.client.get('/api/jobs/' + job['id']).json()
            if job['state'] not in {'queued', 'running'}:
                break
            time.sleep(.1)
        self.assertEqual(job['state'], 'done' if success else 'failed', job)
        return job

    def image(self, job, name):
        response = self.client.get(f'/api/jobs/{job["id"]}/files/{name}')
        self.assertEqual(response.status_code, 200, response.text[:200])
        return Image.open(io.BytesIO(response.content)).convert('RGBA')

    def test_all_eight_tools_real_engine(self):
        session = self.workspace(second=True)
        results = {}
        info = self.run_tool(session, 'image-info')
        self.assertEqual((info['result']['width'], info['result']['height']), (96, 80))
        results['image-info'] = info
        cropped = self.run_tool(session, 'crop', {'region': '24,20,72,60', 'scale': 1})
        image = self.image(cropped, 'result.png')
        self.assertEqual(image.size, (48, 40))
        self.assertEqual(image.getpixel((0, 0)), (255, 0, 0, 255))
        self.assertEqual(image.getpixel((47, 39)), (255, 0, 0, 255))
        results['crop'] = cropped
        resized = self.run_tool(session, 'resize', {'width': 48, 'height': 0})
        self.assertEqual(self.image(resized, 'result.png').size, (48, 40))
        results['resize'] = resized
        colors = self.run_tool(session, 'dominant-colors', {'top': 5, 'tolerance': 8})
        self.assertIn('#FF0000', [x['hex'].upper() for x in colors['result']['colors']])
        self.assertIn('#FFFFFF', [x['hex'].upper() for x in colors['result']['colors']])
        results['dominant-colors'] = colors
        difference = self.run_tool(session, 'pixel-diff', {'grid': 4, 'top': 3})
        self.assertGreater(difference['result']['mean_diff'], 0)
        self.assertEqual(self.image(difference, 'heatmap.png').size, (96, 80))
        results['pixel-diff'] = difference
        foreground = self.run_tool(session, 'extract-foreground', {'mode': 'color', 'exclude_tol': 35})
        image = self.image(foreground, 'foreground.png')
        self.assertEqual(image.getpixel((0, 0))[3], 0)
        self.assertEqual(image.getpixel((48, 40)), (255, 0, 0, 255))
        results['extract-foreground'] = foreground
        traced = self.run_tool(session, 'trace', {'scale': 1, 'color': True, 'polygon': False})
        svg = self.client.get(f'/api/jobs/{traced["id"]}/files/traced.svg').text
        self.assertIn('<svg', svg)
        self.assertIn('<path', svg)
        self.assertGreater(traced['result']['paths'], 0)
        results['trace'] = traced
        page = self.workspace(html=True)
        screenshot = self.run_tool(page, 'html-screenshot', {'width': 640, 'height': 480})
        image = self.image(screenshot, 'screenshot.png')
        self.assertEqual(image.size, (640, 480))
        self.assertEqual(image.getpixel((300, 300))[:3], (12, 34, 56))
        results['html-screenshot'] = screenshot
        for job in results.values():
            data = self.client.get(f'/api/jobs/{job["id"]}/files/export.zip').content
            with zipfile.ZipFile(io.BytesIO(data)) as archive:
                self.assertEqual(set(archive.namelist()), {f['name'] for f in job['files']})
                self.assertEqual(json.loads(archive.read('result.json')), job['result'])
        Path('evidence').mkdir(exist_ok=True)
        Path('evidence/eight-tools.json').write_text(json.dumps({
            'commit': os.environ['GITHUB_SHA'], 'original_mcp_binary': True,
            'tools': {tool: {'state': job['state'], 'result': job['result']} for tool, job in results.items()},
            'pixel_dimensions_alpha_svg_and_html_asserted': True}), encoding='utf-8')

    def test_boundaries_and_failed_core_validation(self):
        session = self.workspace()
        self.assertEqual(self.client.get('/api/capabilities', headers={'Origin': 'https://evil.invalid'}).status_code, 403)
        self.assertEqual(self.client.get('/api/capabilities', headers={'Host': 'evil.invalid'}).status_code, 403)
        other = TestClient(create_app(), base_url='http://127.0.0.1')
        with other:
            other.get('/')
            self.assertEqual(other.get('/api/jobs/00000000-0000-0000-0000-000000000000').status_code, 404)
            self.assertEqual(other.post('/api/jobs', json={'session': session, 'tool': 'image-info'}).status_code, 400)
            other.cookies.clear()
            self.assertEqual(other.get('/api/capabilities').status_code, 403)
        for tool, params in [('exec', {}), ('crop', {'out': '/tmp/stolen'}), ('resize', {'width': 16384, 'height': 16384}), ('pixel-diff', {})]:
            self.assertEqual(self.client.post('/api/jobs', json={'session': session, 'tool': tool, 'params': params}).status_code, 400)
        invalid = self.run_tool(session, 'crop', {'region': '0,0,64,64', 'scale': 9}, success=False)
        self.assertEqual(self.client.get(f'/api/jobs/{invalid["id"]}/files/export.zip').status_code, 404)
        bad = self.client.post('/api/workspaces', files={'original': ('bad.png', b'not an image')})
        self.assertEqual(bad.status_code, 400)
        self.assertEqual(len(self.app.state.workspaces.items), 1)
        self.assertEqual(self.client.delete('/api/workspaces/' + session).status_code, 200)
        self.assertEqual(self.client.get('/api/jobs/' + invalid['id']).status_code, 404)
        for path in ['../escape.png', '/abs.png', 'C:/abs.png', 'NUL.png']:
            with self.assertRaises(ValueError):
                child(Path('/owned'), path)

    def test_queued_cancel_does_not_run_or_export(self):
        async def run():
            workspaces = Workspaces()
            jobs = Jobs()
            try:
                root = workspaces.new('cancel-ci')
                (root / 'input' / 'original.png').write_bytes(picture())
                job = jobs.start('cancel-job', 'cancel-ci', root, 'crop', {'region': '0,0,64,64'})
                await jobs.cancel(job['id'])
                self.assertEqual(jobs.items[job['id']]['state'], 'cancelled')
                self.assertFalse((root / 'output' / 'cancel-job' / 'export.zip').exists())
            finally:
                await jobs.close()
                workspaces.close()
        asyncio.run(run())

    def test_running_mcp_cancel_terminates_original_process(self):
        from studio import mcp, adapters

        async def run():
            import tempfile
            started = asyncio.Event()
            processes = []
            original_spawn = asyncio.create_subprocess_exec

            async def capture(*args, **kwargs):
                process = await original_spawn(*args, **kwargs)
                if str(args[0]) == str(adapters.executable()):
                    processes.append(process)
                return process

            with tempfile.TemporaryDirectory() as directory:
                page = Path(directory) / 'slow.html'
                page.write_text('<!doctype html><script>while(true){}</script>', encoding='utf-8')
                with patch('studio.mcp.asyncio.create_subprocess_exec', side_effect=capture):
                    task = asyncio.create_task(mcp.call(adapters.executable(), 'html-screenshot', {
                        'source': str(page), 'out': str(Path(directory) / 'output.png'),
                        'width': 640, 'height': 480}, lambda p: started.set()))
                    try:
                        await asyncio.wait_for(started.wait(), timeout=20)
                        await asyncio.sleep(.5)
                        self.assertFalse(task.done(), 'Real long-running engine must still be running')
                        task.cancel()
                        await asyncio.gather(task, return_exceptions=True)
                        self.assertEqual(len(processes), 1)
                        self.assertIsNotNone(processes[0].returncode)
                        self.assertFalse((Path(directory) / 'output.png').exists())
                    finally:
                        if not task.done():
                            task.cancel()
                        await asyncio.gather(task, return_exceptions=True)
        asyncio.run(run())

    def test_schema_names_are_discovered_without_domain_revalidation(self):
        from studio.mcp import resolve_tool
        schema = [{'name': 'PixelDiff', 'inputSchema': {'properties': {
            'original': {}, 'rebuilt': {}, 'out-heatmap': {}}}}]
        name, arguments = resolve_tool(schema, 'pixel-diff', {'original': 'owned-a', 'rebuilt': 'owned-b', 'out_heatmap': 'owned-out'})
        self.assertEqual(name, 'PixelDiff')
        self.assertEqual(arguments, {'original': 'owned-a', 'rebuilt': 'owned-b', 'out-heatmap': 'owned-out'})
        with self.assertRaises(ValueError):
            resolve_tool(schema + [{'name': 'pixel_diff', 'inputSchema': schema[0]['inputSchema']}], 'pixel-diff', {})
        with self.assertRaises(ValueError):
            resolve_tool(schema, 'pixel-diff', {'shell': 'forbidden'})
