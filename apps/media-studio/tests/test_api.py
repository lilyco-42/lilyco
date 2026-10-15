"""Actual HTTP -> MCP -> original lffmpeg -> FFmpeg -> decoded output; no stubs."""
import hashlib
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import time
import unittest
import zipfile

from fastapi.testclient import TestClient

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from studio.app import create_app

if os.environ.get('GITHUB_ACTIONS') != 'true':
    raise RuntimeError('Product tests only run in GitHub Actions')


class ActualMediaTests(unittest.TestCase):
    def setUp(self):
        self.app = create_app()
        self.client = TestClient(self.app, base_url='http://127.0.0.1')
        self.client.__enter__()
        self.assertEqual(self.client.get('/').status_code, 200)
        self.source = Path('tests/source.mp4')
        self.before = hashlib.sha256(self.source.read_bytes()).hexdigest()

    def tearDown(self):
        directory = self.app.state.workspaces.root
        self.client.__exit__(None, None, None)
        self.assertFalse(directory.exists())
        self.assertEqual(hashlib.sha256(self.source.read_bytes()).hexdigest(), self.before)

    def upload(self, source=None):
        source = source or self.source
        with source.open('rb') as data:
            response = self.client.post('/api/workspaces', files={'original': (source.name, data, 'video/mp4')})
        self.assertEqual(response.status_code, 200, response.text)
        return response.json()['id']

    def start(self, session, tool, params):
        response = self.client.post('/api/jobs', json={'session': session, 'tool': tool, 'params': params})
        self.assertEqual(response.status_code, 200, response.text)
        return response.json()['id']

    def finished(self, identifier):
        deadline = time.monotonic() + 90
        while time.monotonic() < deadline:
            job = self.client.get('/api/jobs/' + identifier).json()
            if job['state'] not in {'queued', 'running'}:
                self.assertEqual(job['state'], 'done', job)
                return job
            time.sleep(0.03)
        self.fail('Actual media engine did not complete')

    def test_all_four_presets_produce_real_decodable_media(self):
        session = self.upload()
        results = []
        for tool, params in [
            ('compress', {'format': 'mp4', 'crf': 28}),
            ('convert', {'format': 'webm'}),
            ('resize', {'width': 160}),
            ('trim', {'start': 0.5, 'duration': 0.75}),
        ]:
            with self.subTest(tool=tool):
                job = self.finished(self.start(session, tool, params))
                filename = 'result.webm' if tool == 'convert' else 'result.mp4'
                result = self.client.get(f'/api/jobs/{job["id"]}/files/{filename}')
                self.assertEqual(result.status_code, 200)
                owned = self.app.state.jobs.items[job['id']]['out'] / filename
                metadata = json.loads(subprocess.check_output([
                    'ffprobe', '-v', 'error', '-show_streams', '-show_format', '-of', 'json', str(owned)]))
                video = next(s for s in metadata['streams'] if s['codec_type'] == 'video')
                audio = next(s for s in metadata['streams'] if s['codec_type'] == 'audio')
                self.assertEqual(video['codec_name'], 'vp9' if tool == 'convert' else 'h264')
                self.assertEqual(audio['codec_name'], 'opus' if tool == 'convert' else 'aac')
                self.assertEqual(video['width'], 160 if tool == 'resize' else 320)
                self.assertEqual(video['height'], 90 if tool == 'resize' else 180)
                duration = float(metadata['format']['duration'])
                self.assertLess(abs(duration - (0.75 if tool == 'trim' else 2)), 0.2)
                # Decode frames and audio; container headers alone are not success.
                subprocess.run(['ffmpeg', '-v', 'error', '-i', str(owned), '-f', 'null', '-'],
                               check=True, timeout=30)
                archive = self.client.get(f'/api/jobs/{job["id"]}/files/export.zip')
                with zipfile.ZipFile(io.BytesIO(archive.content)) as bundle:
                    self.assertEqual(set(bundle.namelist()), {filename, 'result.json'})
                    self.assertEqual(bundle.read(filename), result.content)
                    self.assertTrue(json.loads(bundle.read('result.json'))['success'])
                results.append({'tool': tool, 'video': video['codec_name'], 'audio': audio['codec_name'],
                                'width': video['width'], 'height': video['height'],
                                'duration': duration, 'decoded': True})
        Path('evidence').mkdir(exist_ok=True)
        Path('evidence/media-roundtrip.json').write_text(json.dumps(
            {'commit': os.environ['GITHUB_SHA'], 'input_unchanged': True, 'cases': results}, indent=2))

    def test_session_origin_host_path_and_payload_boundaries(self):
        with TestClient(create_app(), base_url='http://127.0.0.1') as other:
            self.assertEqual(other.get('/api/capabilities').status_code, 403)
            self.assertEqual(other.get('/api/capabilities', cookies=self.client.cookies).status_code, 403)
        self.assertEqual(self.client.get('/api/capabilities', headers={'Host': 'evil.example'}).status_code, 403)
        self.assertEqual(self.client.post('/api/workspaces', headers={'Origin': 'https://evil.example'}).status_code, 403)
        self.assertEqual(self.client.post('/api/jobs', headers={'Content-Length': str(300 * 1024 * 1024)}).status_code, 413)
        session = self.upload()
        for tool, params in [('shell', {}), ('convert', {'output': '/tmp/escape'}),
                             ('resize', {'width': 3000, 'height': 3000}), ('trim', {'start': 0, 'duration': 0})]:
            response = self.client.post('/api/jobs', json={'session': session, 'tool': tool, 'params': params})
            self.assertEqual(response.status_code, 400, response.text)
        self.assertEqual(len(self.app.state.jobs.items), 0)
        self.assertEqual(self.client.delete('/api/workspaces/' + session).status_code, 200)
        self.assertEqual(self.client.get('/api/workspaces/' + session + '/preview').status_code, 400)

    def test_renamed_playlist_or_invalid_file_is_not_accepted_as_video(self):
        for payload in [b'not a video', b'#EXTM3U\n#EXTINF:1,\nhttps://example.invalid/video.ts\n']:
            response = self.client.post('/api/workspaces', files={'original': ('renamed.mp4', payload, 'video/mp4')})
            self.assertEqual(response.status_code, 400, response.text)
            self.assertEqual(len(self.app.state.workspaces.items), 0)

    @unittest.skipUnless(sys.platform == 'linux', 'Actual Linux process tree acceptance')
    def test_cancel_stops_the_actual_encoder_and_removes_partial_output(self):
        session = self.upload(Path('tests/cancel-source.mp4'))
        identifier = self.start(session, 'compress', {'format': 'webm', 'crf': 20})
        output = self.app.state.jobs.items[identifier]['out']
        deadline = time.monotonic() + 15
        encoder_pids = []
        while time.monotonic() < deadline:
            lines = subprocess.check_output(['ps', '-eo', 'pid=,args='], text=True).splitlines()
            encoder_pids = [int(line.split(None, 1)[0]) for line in lines
                            if str(output) in line and 'ffmpeg' in line]
            if encoder_pids and (output / 'result.webm').exists():
                break
            time.sleep(0.02)
        self.assertTrue(encoder_pids, 'Must observe an actual encoder, not only a queued task')
        response = self.client.post(f'/api/jobs/{identifier}/cancel')
        self.assertEqual(response.status_code, 200, response.text)
        self.assertEqual(response.json()['state'], 'cancelled')
        self.assertFalse(output.exists())
        for pid in encoder_pids:
            state = subprocess.run(['ps', '-p', str(pid), '-o', 'stat='], capture_output=True, text=True)
            self.assertTrue(state.returncode != 0 or state.stdout.strip().startswith('Z'), state.stdout)
        self.assertEqual(self.client.get(f'/api/jobs/{identifier}/files/result.webm').status_code, 404)


if __name__ == '__main__':
    unittest.main()
