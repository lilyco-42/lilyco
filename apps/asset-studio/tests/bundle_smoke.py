"""Exercise the complete distributable, including its bundled real Rust engine."""
import http.cookiejar
import io
import json
import os
import re
import queue
import subprocess
import sys
import threading
import time
import urllib.request
from pathlib import Path
from PIL import Image
from fixture import fixture, spine_fixture

binary = Path(sys.argv[1]).resolve()
env = dict(os.environ)
env.pop('LAIN42_LAIC', None)
env.pop('LAIN42_LYCO_SPINE_RENDER', None)
# Opening an interactive desktop is unwanted in CI; browser BROWSER honors this on Linux.
env['BROWSER'] = 'true'
process = subprocess.Popen([str(binary)], stdout=subprocess.PIPE, stderr=subprocess.STDOUT, env=env)
try:
    # Frozen console output must be unbuffered by launcher for deterministic startup.
    lines = queue.Queue()
    def reader():
        for raw in process.stdout:
            lines.put(raw.decode('utf-8',errors='replace'))
        lines.put(None)
    threading.Thread(target=reader,daemon=True).start()
    url = None
    output = []
    deadline = time.monotonic()+20
    while time.monotonic()<deadline:
        try:
            line = lines.get(timeout=.5)
        except queue.Empty:
            continue
        if line is None:
            break
        output.append(line)
        match = re.search(r'http://127\.0\.0\.1:\d+/', line)
        if match:
            url = match.group(); break
    assert url, 'bundle did not emit loopback URL: '+''.join(output)[-3000:]
    client = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()), urllib.request.ProxyHandler({}))
    for _ in range(50):
        try:
            client.open(url,timeout=2).read(); break
        except OSError:
            time.sleep(.2)
    def get(path):
        return json.loads(client.open(url+path,timeout=10).read())
    assert get('api/capabilities')['tools']['aic']['available']
    data = fixture()
    boundary = 'Lain42CI'
    body = (f'--{boundary}\r\nContent-Disposition: form-data; name="files"; filename="demo.pxls"\r\nContent-Type: application/octet-stream\r\n\r\n'.encode()+data+f'\r\n--{boundary}--\r\n'.encode())
    req = urllib.request.Request(url+'api/workspaces', data=body, headers={'Content-Type':f'multipart/form-data; boundary={boundary}'})
    session = json.loads(client.open(req,timeout=10).read())['id']
    for action in ['inspect','render']:
        req = urllib.request.Request(url+'api/jobs', data=json.dumps({'session':session,'tool':'aic','entry':'demo.pxls','action':action,'motion':'wave'}).encode(),headers={'Content-Type':'application/json'})
        identifier = json.loads(client.open(req,timeout=10).read())['id']
        for _ in range(100):
            job=get('api/jobs/'+identifier)
            if job['state'] in {'done','failed'}: break
            time.sleep(.1)
        assert job['state']=='done', job
        if action=='render': assert job['result']['frames_written']==2
    if os.environ.get('EXPECT_SPINE') == 'true':
        assert get('api/capabilities')['tools']['spine']['available'], 'Spine engine must ship in licensed full package'
        body = b''
        for name, data in spine_fixture().items():
            body += (f'--{boundary}\r\nContent-Disposition: form-data; name="files"; filename="{name}"\r\nContent-Type: application/octet-stream\r\n\r\n'.encode()+data+b'\r\n')
        body += f'--{boundary}--\r\n'.encode()
        req = urllib.request.Request(url+'api/workspaces',data=body,headers={'Content-Type':f'multipart/form-data; boundary={boundary}'})
        session = json.loads(client.open(req,timeout=10).read())['id']
        for action in ['inspect','render']:
            req = urllib.request.Request(url+'api/jobs',data=json.dumps({'session':session,'tool':'spine','entry':'demo.json','action':action,'motion':'wave'}).encode(),headers={'Content-Type':'application/json'})
            identifier = json.loads(client.open(req,timeout=10).read())['id']
            for _ in range(200):
                job = get('api/jobs/'+identifier)
                if job['state'] in {'done','failed'}: break
                time.sleep(.1)
            assert job['state']=='done', job
            if action=='render':
                image = Image.open(io.BytesIO(client.open(url+f'api/jobs/{identifier}/files/animation.gif',timeout=10).read()))
                assert image.size==(512,512) and image.n_frames>1
                image.seek(0); first=image.convert('RGBA').tobytes()
                image.seek(image.n_frames//2)
                assert first!=image.convert('RGBA').tobytes(), 'actual animation must move'
    print(json.dumps({'bundle':binary.name,'real_engine_render':True,'aic_frames':2,'spine_verified':os.environ.get('EXPECT_SPINE')=='true'}))
finally:
    process.terminate()
    process.wait(timeout=10)
