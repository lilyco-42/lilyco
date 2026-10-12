"""Loopback-only HTTP boundary; uploads are owned, temporary and bounded."""
from contextlib import asynccontextmanager
from pathlib import Path
import secrets
import sys
from uuid import UUID, uuid4

from fastapi import FastAPI, File, HTTPException, UploadFile
from fastapi.responses import FileResponse, JSONResponse
from fastapi.staticfiles import StaticFiles
from PIL import Image, UnidentifiedImageError
from pydantic import BaseModel, ConfigDict, Field

from . import adapters
from .jobs import Jobs
from .paths import MAX_FILE, MAX_UPLOAD, Workspaces, child


class JobInput(BaseModel):
    model_config = ConfigDict(extra='forbid')
    session: UUID
    tool: str = Field(max_length=64)
    params: dict = Field(default_factory=dict, max_length=16)


def create_app():
    workspaces, jobs = Workspaces(), Jobs()

    @asynccontextmanager
    async def lifespan(app):
        try:
            yield
        finally:
            await jobs.close()
            workspaces.close()

    app = FastAPI(title='Lain42 Vision Studio', lifespan=lifespan, docs_url=None,
                  redoc_url=None, openapi_url=None)
    app.state.workspaces = workspaces
    app.state.jobs = jobs
    token = secrets.token_urlsafe(32)
    base = Path(getattr(sys, '_MEIPASS', Path(__file__).resolve().parents[1]))
    frontend = base / 'frontend'

    @app.middleware('http')
    async def local_only(request, call_next):
        if request.url.hostname not in {'127.0.0.1', 'localhost', '::1'}:
            return JSONResponse({'detail': '只允许本机访问'}, status_code=403)
        origin = request.headers.get('origin')
        expected = f'{request.url.scheme}://{request.headers.get("host")}'
        if origin and origin != expected:
            return JSONResponse({'detail': '跨站请求被拒绝'}, status_code=403)
        try:
            length = int(request.headers.get('content-length', '0'))
        except ValueError:
            return JSONResponse({'detail': '无效请求大小'}, status_code=400)
        if length > MAX_UPLOAD + 1024 * 1024:
            return JSONResponse({'detail': '文件总量超过 32 MB'}, status_code=413)
        cookie = f'vision_session_{request.url.port or 80}'
        if request.url.path.startswith('/api/') and not secrets.compare_digest(
                request.cookies.get(cookie, ''), token):
            return JSONResponse({'detail': '请从本机应用窗口访问'}, status_code=403)
        response = await call_next(request)
        if request.url.path == '/':
            response.set_cookie(cookie, token, httponly=True, samesite='strict')
        response.headers['Content-Security-Policy'] = (
            "default-src 'self'; img-src 'self' blob: data:; style-src 'self'; "
            "script-src 'self'; object-src 'none'; frame-ancestors 'none'; base-uri 'none'")
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
            installed = True
        except ValueError:
            installed = False
        return {'engine': installed, 'tools': list(adapters.TOOLS),
                'html_browser': adapters.browser_available(), 'max_pixels': adapters.MAX_PIXELS}

    @app.post('/api/workspaces')
    async def upload(original: UploadFile = File(...), rebuilt: UploadFile | None = File(None)):
        identifier = str(uuid4())
        root = workspaces.new(identifier)
        total, files = 0, []
        try:
            for role, item in [('original', original), ('rebuilt', rebuilt)]:
                if item is None:
                    continue
                suffix = Path(item.filename or '').suffix.lower()
                allowed = {'.png', '.jpg', '.jpeg', '.webp'} | ({'.html', '.htm'} if role == 'original' else set())
                if suffix not in allowed:
                    raise ValueError('仅支持 PNG、JPEG、WebP 或本地 HTML 文件')
                path = root / 'input' / (role + suffix)
                size = 0
                with path.open('wb') as stream:
                    while chunk := await item.read(65536):
                        size += len(chunk)
                        total += len(chunk)
                        if size > MAX_FILE or total > MAX_UPLOAD:
                            raise ValueError('单文件最多 16 MB，总量最多 32 MB')
                        stream.write(chunk)
                dimensions = None
                if suffix in {'.html', '.htm'}:
                    if size > 2 * 1024 * 1024:
                        raise ValueError('HTML 最多 2 MB')
                    path.read_text(encoding='utf-8-sig')
                else:
                    with Image.open(path) as image:
                        if image.format not in {'PNG', 'JPEG', 'WEBP'} or image.width * image.height > adapters.MAX_PIXELS:
                            raise ValueError('图片格式不符或超过 1600 万像素')
                        dimensions = [image.width, image.height]
                        image.verify()
                files.append({'role': role, 'size': size, 'dimensions': dimensions})
            return {'id': identifier, 'files': files}
        except (OSError, UnidentifiedImageError, Image.DecompressionBombError) as error:
            workspaces.delete(identifier)
            raise ValueError('图片无法读取，请核对格式') from error
        except BaseException:
            workspaces.delete(identifier)
            raise
        finally:
            await original.close()
            if rebuilt:
                await rebuilt.close()

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
            raise HTTPException(404, '任务不存在或已清空')
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
        if job['state'] != 'done' or name not in {'export.zip'} | {p['name'] for p in job['files']}:
            raise HTTPException(404, '结果尚未完成或文件不存在')
        path = child(job['out'], name)
        return FileResponse(path, filename=path.name, content_disposition_type=(
            'inline' if path.suffix in {'.png', '.svg'} else 'attachment'))

    if frontend.is_dir():
        app.mount('/assets', StaticFiles(directory=frontend), name='assets')
    return app
