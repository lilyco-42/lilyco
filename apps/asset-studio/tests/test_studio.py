import asyncio
import io
import os
import sys
import time
import unittest
import zipfile
from pathlib import Path

from fastapi.testclient import TestClient
from PIL import Image

sys.path.insert(0, str(Path(__file__).parents[1]))
from studio.app import create_app
from studio.paths import child
from studio.jobs import Jobs
from studio.adapters import SpineAdapter
from fixture import fixture


class LocalStudioTests(unittest.TestCase):
    def setUp(self):
        self.app = create_app()
        self.client = TestClient(self.app, base_url='http://127.0.0.1:8043')
        self.client.__enter__()
        self.client.get('/')

    def tearDown(self):
        self.client.__exit__(None, None, None)

    def upload(self, name='demo.pxls', data=None):
        return self.client.post('/api/workspaces', files={'files': (name, fixture() if data is None else data)})

    def run_job(self, session, action, **options):
        response = self.client.post('/api/jobs', json={'session': session, 'tool': 'aic',
                                   'action': action, 'entry': 'demo.pxls', 'motion': 'wave', **options})
        self.assertEqual(response.status_code, 200, response.text)
        identifier = response.json()['id']
        for _ in range(100):
            job = self.client.get('/api/jobs/' + identifier).json()
            if job['state'] in {'done', 'failed', 'cancelled'}:
                return job
            time.sleep(.05)
        self.fail('real engine did not terminate')

    def test_real_engine_inspect_render_export_and_delete(self):
        self.assertTrue(os.environ.get('LAIN42_LAIC'), 'CI must supply the real built engine')
        session = self.upload().json()['id']
        inspect = self.run_job(session, 'inspect')
        self.assertEqual(inspect['state'], 'done', inspect)
        self.assertEqual(inspect['result']['files'][0]['poses'][0]['title'], 'wave')
        render = self.run_job(session, 'render', scale=2)
        self.assertEqual(render['state'], 'done', render)
        self.assertEqual(render['result']['frames_written'], 2)
        self.assertEqual(render['result']['gifs_written'], 1)
        payload = self.client.get(f"/api/jobs/{render['id']}/files/export.zip").content
        with zipfile.ZipFile(io.BytesIO(payload)) as archive:
            gif = next(n for n in archive.namelist() if n.endswith('.gif'))
            image = Image.open(io.BytesIO(archive.read(gif)))
            self.assertEqual(image.size, (128, 96))
            self.assertEqual(image.n_frames, 2)
            frames = []
            for index in range(image.n_frames):
                image.seek(index)
                rgba = image.convert('RGBA')
                self.assertGreater(sum(1 for p in rgba.getdata() if p[3]), 100)
                frames.append(rgba.tobytes())
            self.assertNotEqual(frames[0], frames[1])
        self.assertEqual(self.client.delete('/api/workspaces/' + session).status_code, 200)
        self.assertEqual(self.client.get('/api/jobs/' + render['id']).status_code, 404)
        self.assertEqual(self.client.post('/api/jobs', json={'session':session,'tool':'aic','action':'inspect','entry':'demo.pxls'}).status_code, 400)

    def test_real_invalid_file_is_failure_not_empty_success(self):
        session = self.upload(data=b'not a pxls').json()['id']
        job = self.run_job(session, 'inspect')
        self.assertEqual(job['state'], 'failed', job)
        self.assertTrue(job['error'])
        self.assertFalse(job['files'])

    def test_real_sprite_export_retains_transparent_pixels(self):
        session = self.upload().json()['id']
        job = self.run_job(session, 'sprites')
        self.assertEqual(job['state'], 'done', job)
        payload = self.client.get(f"/api/jobs/{job['id']}/files/export.zip").content
        with zipfile.ZipFile(io.BytesIO(payload)) as archive:
            names = [n for n in archive.namelist() if n.endswith('.png')]
            self.assertTrue(names, 'real sprite export must contain images')
            image = Image.open(io.BytesIO(archive.read(names[0]))).convert('RGBA')
            self.assertGreater(image.width, 0)
            self.assertGreater(sum(1 for p in image.getdata() if p[3]), 0)

    def test_host_origin_cookie_isolation_and_paths(self):
        stranger = TestClient(self.app, base_url='http://127.0.0.1:8043')
        self.assertEqual(stranger.get('/api/capabilities').status_code, 403)
        self.assertEqual(self.client.get('/', headers={'Host':'attacker.example'}).status_code, 403)
        self.assertEqual(self.client.get('/api/capabilities', headers={'Origin':'https://attacker.example'}).status_code, 403)
        self.assertEqual(self.upload(name='../escape.pxls').status_code, 400)
        self.assertEqual(self.upload(name='CON.pxls').status_code, 400)
        self.assertEqual(len(self.app.state.workspaces.items), 0)
        self.assertEqual(self.client.post('/api/workspaces', files={'files':('run.exe', b'x')}).status_code, 400)
        self.assertEqual(self.client.post('/api/workspaces', files={'files':('demo.pxls',fixture())}, data={'paths':'{}'}).status_code, 400)

    def test_strict_parameters_and_read_protection(self):
        session = self.upload().json()['id']
        for updates in [{'entry':'../../secret.pxls'}, {'entry':'demo.pxls','action':'repack'}, {'scale':5}, {'direction':-1}, {'scale':True}, {'tool':'shell'}, {'command':'echo hello'}]:
            body = {'session':session,'tool':'aic','action':'inspect','entry':'demo.pxls',**updates}
            self.assertIn(self.client.post('/api/jobs', json=body).status_code, (400,422))
        job = self.run_job(session, 'inspect')
        self.assertEqual(self.client.get(f"/api/jobs/{job['id']}/files/other.txt").status_code, 404)

    def test_spine_missing_runtime_and_atlas_escape(self):
        import tempfile
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root/'model.json').write_text('{}')
            (root/'model.atlas').write_text('../secret.png\nsize: 8,8\n')
            with self.assertRaises(ValueError):
                SpineAdapter().atlas(root, root/'model.json')
        capabilities = self.client.get('/api/capabilities').json()
        self.assertFalse(capabilities['tools']['spine']['available'], 'do not bundle licensed runtime')

    def test_cancellation_and_single_process(self):
        # Cancellation uses an actual child process, not a status-only mock.
        async def scenario():
            import tempfile
            with tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                (root/'input').mkdir(); (root/'output').mkdir()
                class Slow:
                    def command(self, *args):
                        return [sys.executable,'-c','import time; time.sleep(30)']
                    def result(self, text):
                        return {}
                jobs = Jobs()
                jobs.start('one','session',root,Slow(),None)
                with self.assertRaises(ValueError):
                    jobs.start('two','session',root,Slow(),None)
                await asyncio.sleep(.1)
                await jobs.cancel('one')
                self.assertEqual(jobs.items['one']['state'], 'cancelled')
                self.assertFalse(jobs.busy())
        asyncio.run(scenario())


if __name__ == '__main__':
    unittest.main()
