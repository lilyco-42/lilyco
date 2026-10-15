"""Loopback UI, bounded owned uploads, and explicit per-instance session isolation."""
from contextlib import asynccontextmanager
import json
from pathlib import Path
import secrets
import sys
from uuid import UUID, uuid4

from fastapi import FastAPI, File, HTTPException, UploadFile
from fastapi.responses import FileResponse, JSONResponse
from fastapi.staticfiles import StaticFiles
from pydantic import BaseModel, ConfigDict, Field

from . import adapters
from .jobs import Jobs
from .paths import MAX_FILE, Workspaces, child


class JobInput(BaseModel):
    model_config = ConfigDict(extra='forbid')
    session: UUID
    tool: str = Field(max_length=32)
    params: dict = Field(default_factory=dict, max_length=8)


def create_app():
    workspaces, jobs = Workspaces(), Jobs()

    @asynccontextmanager
    async def lifespan(app):
        try:
            yield
        finally:
            await jobs.close()
            workspaces.close()

    app = FastAPI(title='Lain42 Media Studio', lifespan=lifespan, docs_url=None,
                  redoc_url=None, openapi_url=None)
    app.state.workspaces, app.state.jobs = workspaces, jobs
    frontend = adapters.bundle_root() / 'frontend'
    token = secrets.token_urlsafe(32)

    @app.middleware('http')
    async def local_only(request, call_next):
        if request.url.hostname not in {'127.0.0.1', 'localhost', '::1'}:
            return JSONResponse({'detail': '只允许本机访问'}, status_code=403)
        expected = f'{request.url.scheme}://{request.headers.get("host")}'
        if request.headers.get('origin') and request.headers['origin'] != expected:
            return JSONResponse({'detail': '跨站请求被拒绝'}, status_code=403)
        try:
            length = int(request.headers.get('content-length', '0'))
        except ValueError:
            return JSONResponse({'detail': '无效请求大小'}, status_code=400)
        if length < 0 or length > MAX_FILE + 1024 * 1024:
            return JSONResponse({'detail': '单个视频最多 256 MB'}, status_code=413)
        cookie = f'media_session_{request.url.port or 80}'
        if request.url.path.startswith('/api/') and not secrets.compare_digest(
                request.cookies.get(cookie, ''), token):
            return JSONResponse({'detail': '请从本机应用窗口访问'}, status_code=403)
        response = await call_next(request)
        if request.url.path == '/':
            response.set_cookie(cookie, token, httponly=True, samesite='strict')
        response.headers['Content-Security-Policy'] = (
            "default-src 'self'; img-src 'self' data:; media-src 'self' blob:; "
            "style-src 'self'; script-src 'self'; object-src 'none'; "
            "frame-ancestors 'none'; base-uri 'none'")
        response.headers['X-Content-Type-Options'] = 'nosniff'
        response.headers['Cache-Control'] = 'no-store'
        return response

    @app.exception_handler(ValueError)
    async def invalid(request, error):
        return JSONResponse({'detail': str(error)[:1000]}, status_code=400)

    @app.get('/')
    def index():
        return FileResponse(frontend / 'index.html')

    @app.get('/api/capabilities')
    def capabilities():
        try:
            adapters.executable()
            adapters.runtime('ffmpeg')
            adapters.runtime('ffprobe')
            installed = True
        except ValueError:
            installed = False
        return {'engine': installed, 'tools': sorted(adapters.PRESETS), 'max_file': MAX_FILE}

    @app.post('/api/workspaces')
    async def upload(original: UploadFile = File(...)):
        identifier = str(uuid4())
        root = workspaces.new(identifier)
        try:
            suffix = Path(original.filename or '').suffix.lower()
            if suffix not in adapters.INPUT_SUFFIXES:
                raise ValueError('请选择 MP4、MOV、M4V、WebM 或 MKV 视频')
            path = root / 'input' / ('original' + suffix)
            size = 0
            with path.open('wb') as stream:
                while chunk := await original.read(65536):
                    size += len(chunk)
                    if size > MAX_FILE:
                        raise ValueError('单个视频最多 256 MB')
                    stream.write(chunk)
            metadata = await adapters.probe(path)
            (root / 'media.json').write_text(json.dumps(metadata), encoding='utf-8')
            return {'id': identifier, 'name': Path(original.filename or '视频').name,
                    'media': metadata, 'preview': f'/api/workspaces/{identifier}/preview'}
        except BaseException:
            workspaces.delete(identifier)
            raise
        finally:
            await original.close()

    @app.get('/api/workspaces/{identifier}/preview')
    def preview(identifier: UUID):
        root = workspaces.get(str(identifier))
        paths = list((root / 'input').glob('original.*'))
        if len(paths) != 1:
            raise HTTPException(404, '视频已删除')
        return FileResponse(paths[0], content_disposition_type='inline')

    @app.delete('/api/workspaces/{identifier}')
    async def delete(identifier: UUID):
        identifier = str(identifier)
        await jobs.delete_session(identifier)
        workspaces.delete(identifier)
        return {'deleted': True}

    @app.post('/api/jobs')
    async def start(request: JobInput):
        session = str(request.session)
        return jobs.start(str(uuid4()), session, workspaces.get(session), request.tool, request.params)

    def get_job(identifier):
        job = jobs.items.get(str(identifier))
        if not job:
            raise HTTPException(404, '任务不存在或已删除')
        return job

    @app.get('/api/jobs/{identifier}')
    def status(identifier: UUID):
        return jobs.public(get_job(identifier))

    @app.post('/api/jobs/{identifier}/cancel')
    async def cancel(identifier: UUID):
        get_job(identifier)
        return await jobs.cancel(str(identifier))

    @app.get('/api/jobs/{identifier}/files/{name:path}')
    def download(identifier: UUID, name: str):
        job = get_job(identifier)
        allowed = {'export.zip'} | {p['name'] for p in job['files']}
        if job['state'] != 'done' or name not in allowed:
            raise HTTPException(404, '结果尚未完成或文件不存在')
        path = child(job['out'], name)
        return FileResponse(path, filename=path.name, content_disposition_type=(
            'inline' if path.suffix in {'.mp4', '.webm'} else 'attachment'))

    if frontend.is_dir():
        app.mount('/assets', StaticFiles(directory=frontend), name='assets')
    return app
