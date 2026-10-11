import asyncio
import json
import secrets
import uuid
from contextlib import asynccontextmanager
from pathlib import Path
from urllib.parse import urlsplit

from fastapi import FastAPI, File, Form, HTTPException, Request, UploadFile
from fastapi.responses import FileResponse, HTMLResponse, JSONResponse
from pydantic import BaseModel, ConfigDict, Field
from starlette.staticfiles import StaticFiles

from .adapters import ADAPTERS
from .jobs import Jobs
from .paths import MAX_FILE, MAX_FILES, MAX_UPLOAD, Workspaces, child

WEB = Path(__file__).parents[1] / 'frontend'


class JobInput(BaseModel):
    model_config = ConfigDict(extra='forbid', strict=True)
    session: str = Field(pattern=r'^[a-f0-9]{32}$')
    tool: str = Field(pattern=r'^(aic|spine)$')
    action: str = Field(pattern=r'^(inspect|render|sprites|textures)$')
    entry: str = Field(min_length=1, max_length=240)
    motion: str = Field(default='*', min_length=1, max_length=100)
    direction: int = Field(default=0, ge=0, le=7)
    scale: int = Field(default=1, ge=1, le=4)


def create_app():
    workspaces, jobs, token = Workspaces(), Jobs(), secrets.token_urlsafe(32)

    @asynccontextmanager
    async def lifespan(app):
        yield
        await jobs.close()
        workspaces.close()

    app = FastAPI(title='Lain42 Asset Studio', lifespan=lifespan, docs_url=None, redoc_url=None, openapi_url=None)
    app.state.workspaces, app.state.jobs, app.state.token = workspaces, jobs, token

    @app.middleware('http')
    async def local_only(request: Request, call_next):
        host = request.headers.get('host', '')
        try:
            parsed = urlsplit('http://' + host)
            valid = parsed.hostname in {'127.0.0.1', 'localhost', '::1'} and parsed.username is None and not parsed.path
            origin = request.headers.get('origin')
            if origin:
                valid = valid and origin in {'http://' + host, 'https://' + host}
            if request.url.path.startswith('/api/'):
                supplied = request.cookies.get('asset_session', '')
                valid = valid and secrets.compare_digest(supplied, token)
            if not valid:
                return JSONResponse({'detail': '请从本机工具窗口打开'}, status_code=403)
            length = int(request.headers.get('content-length', '0'))
            if length > MAX_UPLOAD + 1024 * 1024:
                return JSONResponse({'detail': '一次最多选择 512 MB 文件'}, status_code=413)
        except (ValueError, TypeError):
            return JSONResponse({'detail': '无效请求'}, status_code=400)
        response = await call_next(request)
        response.headers.update({'Cache-Control': 'no-store', 'X-Content-Type-Options': 'nosniff',
                                 'Content-Security-Policy': "default-src 'self'; img-src 'self' data:; style-src 'self'; script-src 'self'; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'"})
        return response

    @app.exception_handler(ValueError)
    async def validation_error(request, error):
        return JSONResponse({'detail': str(error)[:1000]}, status_code=400)

    @app.get('/', response_class=HTMLResponse)
    def index():
        response = HTMLResponse((WEB / 'index.html').read_text(encoding='utf-8'))
        response.set_cookie('asset_session', token, httponly=True, samesite='strict')
        return response

    @app.get('/api/capabilities')
    def capabilities():
        return {'tools': {k: {'available': adapter.available()} for k, adapter in ADAPTERS.items()},
                'max_files': MAX_FILES, 'max_mb': MAX_UPLOAD // 1024 // 1024}

    @app.post('/api/workspaces')
    async def upload(files: list[UploadFile] = File(...), paths: str = Form('[]')):
        if not 1 <= len(files) <= MAX_FILES:
            raise HTTPException(413, '请一次选择 1–2000 个文件')
        try:
            names = json.loads(paths)
        except json.JSONDecodeError as error:
            raise ValueError('文件目录信息无效') from error
        if not isinstance(names, list) or (names and len(names) != len(files)):
            raise ValueError('文件目录信息无效')
        names = names or [f.filename for f in files]
        if any(not isinstance(n, str) for n in names) or len({n.casefold() for n in names}) != len(names):
            raise ValueError('文件路径不能为空或重复')
        identifier = uuid.uuid4().hex
        root = workspaces.new(identifier)
        total = 0
        try:
            for file, name in zip(files, names):
                path = child(root / 'input', name)
                if path.suffix.lower() not in {'.pxls', '.dat', '.bytes', '.png', '.atlas', '.json', '.skel', '.ress'}:
                    # Folder selection may contain game executables etc. Never execute uploaded files.
                    raise ValueError('仅选择角色表、贴图、图集与骨架；不需要游戏程序或脚本')
                path.parent.mkdir(parents=True, exist_ok=True)
                size = 0
                with path.open('xb') as target:
                    while chunk := await file.read(1024 * 1024):
                        size += len(chunk)
                        total += len(chunk)
                        if size > MAX_FILE or total > MAX_UPLOAD:
                            raise HTTPException(413, '单文件最多 128 MB，一次最多 512 MB')
                        target.write(chunk)
                await file.close()
            entries = {k: a.entries(root / 'input') for k, a in ADAPTERS.items()}
            if not any(entries.values()):
                raise ValueError('没有找到角色文件；AIC 需要 .pxls，Spine 需要 .json 或 .skel')
            return {'id': identifier, 'entries': entries, 'bytes': total, 'count': len(files)}
        except BaseException:
            workspaces.delete(identifier)
            raise
        finally:
            for file in files:
                await file.close()

    @app.delete('/api/workspaces/{identifier}')
    async def delete(identifier: str):
        workspaces.get(identifier)
        await jobs.delete_session(identifier)
        workspaces.delete(identifier)
        return {'ok': True}

    @app.post('/api/jobs')
    async def start(body: JobInput):
        root = workspaces.get(body.session)
        return jobs.start(uuid.uuid4().hex, body.session, root, ADAPTERS[body.tool], body)

    def get_job(identifier):
        if identifier not in jobs.items:
            raise HTTPException(404, '任务记录已删除')
        return jobs.items[identifier]

    @app.get('/api/jobs/{identifier}')
    def status(identifier: str):
        return jobs.public(get_job(identifier))

    @app.post('/api/jobs/{identifier}/cancel')
    async def cancel(identifier: str):
        get_job(identifier)
        return await jobs.cancel(identifier)

    @app.get('/api/jobs/{identifier}/files/{name:path}')
    def download(identifier: str, name: str):
        job = get_job(identifier)
        if job['state'] != 'done':
            raise HTTPException(409, '任务尚未完成')
        allowed = {f['name'] for f in job['files']} | {'export.zip'}
        if name not in allowed:
            raise HTTPException(404, '输出文件不存在')
        path = child(job['out'], name)
        return FileResponse(path, filename=path.name, content_disposition_type='inline' if path.suffix in {'.png', '.gif'} else 'attachment')

    app.mount('/assets', StaticFiles(directory=WEB), name='assets')
    return app
