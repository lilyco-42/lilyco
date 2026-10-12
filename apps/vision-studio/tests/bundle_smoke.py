"""CI-only acceptance of actual frozen HTTP app and bundled original lvision."""
import io
import json
import os
from pathlib import Path
import queue
import re
import subprocess
import sys
import threading
import time
import zipfile

import httpx
from PIL import Image
from fixture import picture

if os.environ.get('GITHUB_ACTIONS') != 'true':
    raise SystemExit('Frozen app acceptance only in GitHub Actions')
env = dict(os.environ)
env.pop('LAIN42_VISION_CLI', None)
process = subprocess.Popen([str(Path(sys.argv[1]).resolve())], stdout=subprocess.PIPE,
                           stderr=subprocess.STDOUT, env=env)
lines = queue.Queue()


def collect():
    for line in process.stdout:
        lines.put(line.decode('utf-8', errors='replace'))


threading.Thread(target=collect, daemon=True).start()
try:
    url = None
    deadline = time.time() + 45
    while time.time() < deadline:
        if process.poll() is not None:
            raise RuntimeError('Frozen app exited before startup')
        try:
            line = lines.get(timeout=.2)
            found = re.search(r'http://127\.0\.0\.1:\d+/', line)
            if found:
                url = found[0]
                break
        except queue.Empty:
            pass
    assert url, 'Missing startup URL'
    with httpx.Client(base_url=url, timeout=20, trust_env=False) as client:
        for _ in range(80):
            try:
                response = client.get('/')
                response.raise_for_status()
                break
            except Exception:
                time.sleep(.2)
        capabilities = client.get('api/capabilities').json()
        assert capabilities['engine'] and len(capabilities['tools']) == 8, capabilities
        response = client.post('api/workspaces', files={'original': ('original.png', picture(), 'image/png')})
        response.raise_for_status()
        workspace = response.json()
        response = client.post('api/jobs', json={'session': workspace['id'], 'tool': 'crop',
                                               'params': {'region': '24,20,72,60', 'scale': 1}})
        response.raise_for_status()
        job = response.json()
        for _ in range(200):
            job = client.get('api/jobs/' + job['id']).json()
            if job['state'] not in {'queued', 'running'}:
                break
            time.sleep(.1)
        assert job['state'] == 'done', job
        data = client.get(f'api/jobs/{job["id"]}/files/result.png').content
        image = Image.open(io.BytesIO(data)).convert('RGBA')
        assert image.size == (48, 40) and image.getpixel((0, 0)) == (255, 0, 0, 255)
        exported = client.get(f'api/jobs/{job["id"]}/files/export.zip').content
        with zipfile.ZipFile(io.BytesIO(exported)) as archive:
            assert archive.read('result.png') == data
            assert json.loads(archive.read('result.json')) == job['result']
        response = client.delete('api/workspaces/' + workspace['id'])
        response.raise_for_status()
        assert client.get('api/jobs/' + job['id']).status_code == 404
        Path('evidence').mkdir(exist_ok=True)
        Path('evidence/bundle-smoke.json').write_text(json.dumps({
            'commit': os.environ['GITHUB_SHA'], 'frozen_app': True, 'bundled_original_mcp': True,
            'crop_pixels_and_dimensions': True, 'real_zip_bytes': True, 'deleted_results': True}), encoding='utf-8')
finally:
    process.terminate()
    process.wait(timeout=15)
