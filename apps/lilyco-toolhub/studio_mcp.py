"""Client-only Lilyco task orchestrator: skill discovery and Blender reference staging.

A precise "three views to finished 3D" pipeline requires a real reconstruction
provider; this module provides an honest auditable baseline, not fake geometry.
Never fetches arbitrary scripts, downloads models or executes discovered skills.
"""
from __future__ import annotations

import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import time
from typing import Any

HERE = Path(__file__).resolve().parent
CATALOG_PATH = HERE / 'catalog' / 'skills.json'
BLENDER_SCRIPT = HERE / 'workflows' / 'blender_reference_stage.py'
MAX_IMAGE_BYTES = 25 * 1024 * 1024
MAX_RUN_SECONDS = 210


class WorkflowError(Exception):
    pass


def skill_search(query: str) -> dict[str, Any]:
    """Read a bundled, reviewed lyco-skill market seed and optional CLI-Hub catalog."""
    query = query.strip().lower()
    if len(query) > 128:
        raise WorkflowError('query exceeds 128 characters')
    catalog = json.loads(CATALOG_PATH.read_text(encoding='utf-8'))
    skills = catalog['skills']
    # "3d" returns the full Blender pipeline: lyco research + harness + expert.
    normalized = '3d' if any(w in query for w in ('三视图', '建模', 'blender', '3d', '三维')) else query
    matches = [
        {"id": entry['id'], "name": entry['name'], "source": entry['source'],
         "trust": entry['trust'], "execution": entry['execution'],
         "capabilities": entry['capabilities']}
        for entry in skills
        if not normalized or normalized == '3d' and '3d' in entry['capabilities']
        or any(normalized in text.lower() for text in
               [entry['id'], entry['name'], entry['description'], *entry['capabilities']])
    ]
    # CLI-Hub is consulted read-only. Discovery NEVER implies permission to install.
    hub = shutil.which('cli-hub')
    hub_info: dict[str, Any] = {"status": 'unavailable'}
    if hub:
        try:
            proc = subprocess.run([hub, 'search', 'blender', '--json'], capture_output=True,
                                  text=True, encoding='utf-8', errors='replace',
                                  timeout=14, stdin=subprocess.DEVNULL, shell=False)
            data = json.loads(proc.stdout)
            hub_info = {'status': 'ok' if proc.returncode == 0 else 'failed',
                        'results': [{"name": item.get('name'),
                                     "description": item.get('description'),
                                     "version": item.get('version')}
                                    for item in data[:8]] if isinstance(data, list) else []}
        except (OSError, subprocess.TimeoutExpired, json.JSONDecodeError):
            hub_info = {'status': 'unavailable'}
    return {'ok': True, 'catalog_version': catalog['schema_version'],
            'matched': matches, 'cli_anything': hub_info,
            'installation_policy': 'allowlist + first-run device approval + pinned source'}


def blender_executable() -> str | None:
    configured = os.environ.get('LILYCO_BLENDER_BIN')
    if configured:
        path = Path(configured)
        if not path.is_absolute() or not path.is_file():
            raise WorkflowError('LILYCO_BLENDER_BIN must be an existing absolute local path')
        return str(path)
    return shutil.which('blender')


def preflight() -> dict[str, Any]:
    blender = blender_executable()
    harness = shutil.which('cli-anything-blender')
    harness_installed = bool(harness)
    if not harness_installed and shutil.which('cli-hub'):
        try:
            result = subprocess.run(['cli-hub','info','blender'], stdout=subprocess.PIPE,
                                    stderr=subprocess.DEVNULL, text=True, encoding='utf-8',
                                    errors='replace', timeout=8, shell=False)
            harness_installed = result.returncode == 0 and bool(
                re.search(r'Status:\s+installed',result.stdout,flags=re.IGNORECASE))
        except (OSError, subprocess.TimeoutExpired):
            pass
    return {
        'ok': True,
        'blender': {
            'installed': bool(blender),
            'path': blender,
            'required': 'Blender >= 4.2',
        },
        'cli_anything': {
            'cli_hub': bool(shutil.which('cli-hub')),
            'blender_harness_installed': harness_installed,
            'status': 'optional; Blender native CLI works independently',
        },
        'trusted_workflow': 'lyco.3d.reference-stage',
        'expert_workflow': 'lyco.blender.pipeline (discovery only, execution pending review)',
        'full_reconstruction_available': False,
        'next': 'ready_for_reference_stage' if blender else 'requires_approved_blender_install',
        'approval': 'User device approval required before any new software installation',
    }


def _input_image(raw: str) -> Path:
    if not isinstance(raw, str) or not raw or '\x00' in raw:
        raise WorkflowError('image must be a local file path')
    try:
        image = Path(raw).resolve(strict=True)
    except OSError as exc:
        raise WorkflowError("input image missing or inaccessible") from exc
    if not image.is_file() or image.stat().st_size > MAX_IMAGE_BYTES or image.stat().st_size == 0:
        raise WorkflowError('image must be a non-empty local PNG or JPEG <=25 MiB')
    if image.suffix.lower() not in ('.png', '.jpg', '.jpeg'):
        raise WorkflowError('three-view images must be PNG or JPEG')
    with image.open('rb') as f:
        header = f.read(12)
    if not (header.startswith(b'\x89PNG\r\n\x1a\n') or header[:3] == b'\xff\xd8\xff'):
        raise WorkflowError('image bytes do not match PNG/JPEG signature')
    return image


def _job_workspace() -> Path:
    raw = os.environ.get('LILYCO_AUTOMATION_ROOT')
    if not raw:
        raise WorkflowError('LILYCO_AUTOMATION_ROOT required for local project output')
    folder = Path(raw).resolve(strict=True)
    if not folder.is_dir():
        raise WorkflowError('automation root must be an existing local directory')
    return folder


def _safe_job_name(name: str) -> str:
    if not isinstance(name, str) or not re.fullmatch(r'[\w\-]{1,48}', name, re.UNICODE):
        raise WorkflowError('project name must be 1-48 letters, digits, _ or -')
    return name


def create_reference_stage(args: dict[str, Any]) -> dict[str, Any]:
    """Generate .blend and preview. This is a prep step, NOT reconstructed 3D."""
    project = _safe_job_name(args.get('project', 'three-view-project'))
    root = _job_workspace()
    views = {name: _input_image(args[name]) for name in ('front', 'side', 'back')}
    blender = blender_executable()
    if not blender:
        raise WorkflowError('Blender not installed: request user approval for trusted installer')
    output = (root / project).resolve()
    if not output.is_relative_to(root):
        raise WorkflowError('project must be created inside local workspace')
    if output.exists():
        raise WorkflowError('project already exists; refusing overwrite')
    if not BLENDER_SCRIPT.is_file():
        raise WorkflowError('trusted Blender workflow script was not bundled')
    start = time.monotonic()
    output.mkdir(parents=False, exist_ok=False)
    stages = [
        {'name': 'lyco-skill-discovery', 'status': 'completed',
         'source': 'curated bundled market catalog'},
        {'name': 'environment-check', 'status': 'completed',
         'provider': str(blender)},
        {'name': 'blender-reference-stage', 'status': 'running'},
        {'name': 'geometry-reconstruction', 'status': 'not_configured'},
        {'name': 'expert-topology-qa', 'status': 'not_started'},
    ]
    manifest = {
        'schema': 1, 'output': str(output),
        'views': {k: str(v) for k,v in views.items()},
        'stages': stages, 'source_skill': 'lyco.3d.reference-stage',
        'quality_gate': 'draft_reference_only',
    }
    manifest_path = output / 'job.json'
    manifest_path.write_text(json.dumps(manifest, indent=2, ensure_ascii=False),encoding='utf-8')
    try:
        command = [blender, '--background', '--factory-startup', '--python',
                   str(BLENDER_SCRIPT), '--', str(manifest_path)]
        proc = subprocess.run(command, stdin=subprocess.DEVNULL,
                              capture_output=True, text=True, encoding='utf-8',
                              errors='replace', shell=False, timeout=MAX_RUN_SECONDS)
        if proc.returncode:
            raise WorkflowError('Blender failed: ' + (proc.stderr or proc.stdout)[-700:])
        report = output / 'stage_result.json'
        if not report.is_file():
            raise WorkflowError('Blender finished without stage_result.json')
        data = json.loads(report.read_text(encoding='utf-8'))
        for key in ('blend', 'preview'):
            if not Path(data[key]).is_file() or Path(data[key]).stat().st_size < 100:
                raise WorkflowError('missing/empty Blender artifact: ' + key)
        stages[2]['status'] = 'completed'
        manifest['status'] = 'reference_stage_ready'
        manifest['stages'] = stages
        manifest_path.write_text(json.dumps(manifest, ensure_ascii=False,indent=2),encoding='utf-8')
        return {'ok': True, 'status': 'reference_stage_ready',
                'out': data['preview'],
                'blend': data['blend'],
                'workflow': str(manifest_path),
                'quality': 'reference_and_proxy_only',
                'real_3d_model_finished': False,
                'elapsed_ms': round((time.monotonic() - start)*1000),
                'next_required_skill': 'approved 3D reconstruction model + expert topology QA'}
    except (subprocess.TimeoutExpired, WorkflowError, OSError, ValueError) as e:
        stages[2]['status'] = 'failed'
        manifest['status'] = 'failed'
        manifest['reason'] = str(e)[:600]
        manifest_path.write_text(json.dumps(manifest,ensure_ascii=False,indent=2),encoding='utf-8')
        raise WorkflowError('reference-stage failed; diagnostic kept in job.json: ' + str(e)) from e


TOOLS = [
    {'name': 'lyco_find_skills',
     'description': 'Search bundled trusted Lyco skill market, Blender expert pipelines and CLI-Anything Blender harness; discovery only, never install arbitrary code.',
     'inputSchema': {'type':'object','properties':{'query':{'type':'string','description':'e.g. 三视图 Blender 3D'}}}},
    {'name':'lyco_environment_preflight',
     'description':'Inspect this client for Blender and CLI-Anything. Never installs anything without device approval.',
     'inputSchema':{'type':'object','properties':{}}},
    {'name':'lyco_create_3d_reference',
     'description':'Given front, side and back image paths, create editable Blender .blend reference stage and preview on client. This is NOT a high-fidelity reconstructed 3D model. Input files remain local. Requires Blender installed and LILYCO_AUTOMATION_ROOT output folder.',
     'inputSchema': {'type':'object',
                     'required':['front','side','back'],
                     'properties':{
                         'front':{'type':'string','description':'Local front-view PNG/JPEG'},
                         'side':{'type':'string','description':'Local side-view PNG/JPEG'},
                         'back':{'type':'string','description':'Local back-view PNG/JPEG'},
                         'project':{'type':'string','description':'Optional safe project directory name; default three-view-project'}
                     }}},
]


def mcp_message(value: dict[str,Any]) -> dict[str,Any] | None:
    if 'id' not in value:
        return None
    mid = value['id']
    method = value.get('method')
    if method == 'initialize':
        result: dict[str,Any] = {'protocolVersion':'2024-11-05','capabilities':{'tools':{}},
                                  'serverInfo':{'name':'lilyco-studio','version':'0.1.0'}}
    elif method == 'tools/list':
        result = {'tools': TOOLS}
    elif method == 'tools/call':
        request=value.get('params',{})
        name=request.get('name')
        args=request.get('arguments',{})
        try:
            if not isinstance(args,dict):
                raise WorkflowError('tool arguments must be object')
            if name=='lyco_find_skills':
                out=skill_search(args.get('query',''))
            elif name=='lyco_environment_preflight':
                out=preflight()
            elif name=='lyco_create_3d_reference':
                out=create_reference_stage(args)
            else:
                raise WorkflowError('unknown studio tool')
            result={'content':[{'type':'text','text':json.dumps(out,ensure_ascii=False)}],'isError':False}
        except (WorkflowError,KeyError,OSError,ValueError) as exc:
            result={'content':[{'type':'text','text':str(exc)}],'isError':True}
    elif method == 'ping':
        result = {}
    else:
        return {'jsonrpc':'2.0','id':mid,'error':{'code':-32601,'message':'unknown method'}}
    return {'jsonrpc':'2.0','id':mid,'result':result}


def main() -> None:
    if hasattr(sys.stdin, "reconfigure"): sys.stdin.reconfigure(encoding="utf-8", errors="replace")
    if hasattr(sys.stdout, "reconfigure"): sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    for line in sys.stdin:
        if not line.strip(): continue
        try:
            req=json.loads(line)
            response=mcp_message(req)
        except (json.JSONDecodeError,TypeError):
            response={'jsonrpc':'2.0','id':None,'error':{'code':-32700,'message':'Invalid JSON'}}
        if response is not None:
            print(json.dumps(response,ensure_ascii=False),flush=True)


if __name__ == '__main__':
    main()
