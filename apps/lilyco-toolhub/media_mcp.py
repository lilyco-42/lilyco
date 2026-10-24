"""Two locally executed MCP media plugins for Lilyco Tool Hub.

AI gets a high-level callable API. Manim and Chromium/FFmpeg run only on the
device where this MCP server is installed. No remote rendering or cloud API.
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

VERSION = "0.1.0"
TOOLS = [
    {
        "name": "manim_render",
        "description": "Render a TRUSTED local Manim Community Python Scene to a local MP4 on the client computer. The script executes local Python code; user must enable LILYCO_TRUST_LOCAL_MEDIA=1 and constrain LILYCO_MEDIA_ROOT to a trusted project folder. Output must not already exist. Manim Community must be installed on the client.",
        "inputSchema": {
            "type": "object", "required": ["scene_file", "scene", "output"],
            "properties": {
                "scene_file": {"type": "string", "description": "Trusted local .py Manim scene inside configured workspace"},
                "scene": {"type": "string", "description": "Name of a Manim Scene class within script"},
                "output": {"type": "string", "description": "New MP4 path inside the workspace"},
                "quality": {"type": "string", "enum": ["l", "m", "h"], "description": "low/medium/high Manim render quality"},
            },
        },
    },
    {
        "name": "html_to_video",
        "description": "Render a TRUSTED local HTML animation to MP4 using Chrome/Playwright + FFmpeg ON THE CLIENT. For deterministic results, define window.seek(t) to set animation state for time t in seconds. Client blocks all external network requests. Requires LILYCO_TRUST_LOCAL_MEDIA=1 and a trusted LILYCO_MEDIA_ROOT folder.",
        "inputSchema": {
            "type": "object", "required": ["html_file", "output", "duration"],
            "properties": {
                "html_file": {"type": "string", "description": "Local HTML file within trusted workspace"},
                "output": {"type": "string", "description": "New MP4 output in trusted workspace"},
                "duration": {"type": "number", "description": "Duration in seconds 0.2..60"},
                "fps": {"type": "number", "description": "Frames per second 1..30, default 24"},
                "width": {"type": "number", "description": "Viewport width 320..1920, default 1280"},
                "height": {"type": "number", "description": "Viewport height 240..1080, default 720"},
            },
        },
    },
]


class MediaError(Exception):
    pass


def root_path() -> Path:
    root = os.environ.get("LILYCO_MEDIA_ROOT")
    if not root:
        raise MediaError("LILYCO_MEDIA_ROOT is required: set it to your trusted local media project directory")
    path = Path(root).resolve(strict=True)
    if not path.is_dir():
        raise MediaError("LILYCO_MEDIA_ROOT must be a directory")
    return path


def trusted_file(raw: str, suffix: str, root: Path, must_exist: bool = True) -> Path:
    if not isinstance(raw, str) or not raw or "\x00" in raw:
        raise MediaError("path must be a non-empty local path")
    path = Path(raw)
    if not path.is_absolute():
        path = root / path
    # Reject NTFS symlink/junction escapes by canonicalizing the target.
    try:
        path = path.resolve(strict=must_exist)
    except OSError as e:
        raise MediaError("input file cannot be resolved inside trusted workspace") from e
    if not path.is_relative_to(root) or path.suffix.lower() != suffix:
        raise MediaError("path must stay within LILYCO_MEDIA_ROOT and use " + suffix)
    if must_exist and (not path.is_file() or path.stat().st_size > 10 * 1024 * 1024):
        raise MediaError("missing input file or input file exceeds 10 MiB")
    if not must_exist:
        if not path.parent.is_dir() or path.exists():
            raise MediaError("output directory missing or output already exists")
    return path


def permission() -> Path:
    if os.environ.get("LILYCO_TRUST_LOCAL_MEDIA") != "1":
        raise MediaError("local HTML/Python execution requires human opt-in: LILYCO_TRUST_LOCAL_MEDIA=1")
    return root_path()


def process(argv: list[str], timeout: int, cwd: Path) -> None:
    try:
        result = subprocess.run(argv, cwd=cwd, stdin=subprocess.DEVNULL, capture_output=True,
                                text=True, encoding="utf-8", errors="replace",
                                timeout=timeout, check=False, shell=False)
    except subprocess.TimeoutExpired as e:
        raise MediaError(f"client media render timed out after {timeout}s") from e
    except OSError as e:
        raise MediaError(f"missing local renderer: {e}") from e
    if result.returncode:
        raise MediaError(f"client renderer failed ({result.returncode}): " +
                         (result.stderr or result.stdout)[-550:])


def manim_render(args: dict[str, Any]) -> dict[str, Any]:
    root = permission()
    input_file = trusted_file(args["scene_file"], ".py", root)
    output = trusted_file(args["output"], ".mp4", root, must_exist=False)
    scene = args["scene"]
    if not isinstance(scene, str) or not re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]{0,63}", scene):
        raise MediaError("Manim scene must be a Python class name")
    quality = args.get("quality", "l")
    if quality not in ("l", "m", "h"):
        raise MediaError("quality must be l, m, or h")
    manim = os.environ.get("LILYCO_MANIM_BIN") or shutil.which("manim")
    if not manim:
        raise MediaError("manim executable not found; install Manim Community on this client")
    start = time.monotonic()
    with tempfile.TemporaryDirectory(prefix="lilyco-manim-", dir=root) as folder:
        media_dir = Path(folder)
        # Manim --media_dir, --output_file and --format are documented by Manim Community.
        process([manim, "render", "-q", quality, "--format", "mp4",
                 "--media_dir", str(media_dir), "-o", "result.mp4", str(input_file), scene],
                timeout=300, cwd=root)
        renders = list(media_dir.rglob("result.mp4"))
        if len(renders) != 1 or renders[0].stat().st_size < 100:
            raise MediaError("Manim completed without producing a valid MP4")
        # Exclusive create blocks races/accidental overwrite.
        with output.open("xb") as target, renders[0].open("rb") as source:
            shutil.copyfileobj(source, target)
    return {"ok": True, "out": str(output), "bytes": output.stat().st_size,
            "backend": "manim-local", "duration_ms": round((time.monotonic()-start)*1000)}


def html_to_video(args: dict[str, Any]) -> dict[str, Any]:
    root = permission()
    source = trusted_file(args["html_file"], ".html", root)
    out = trusted_file(args["output"], ".mp4", root, must_exist=False)
    try:
        seconds = float(args["duration"])
        fps = int(args.get("fps", 24))
        width = int(args.get("width", 1280))
        height = int(args.get("height", 720))
    except (TypeError, ValueError, OverflowError) as e:
        raise MediaError("invalid video dimensions, fps or duration") from e
    if not (0.2 <= seconds <= 60 and 1 <= fps <= 30 and
            320 <= width <= 1920 and 240 <= height <= 1080 and
            fps*seconds <= 900):
        raise MediaError("video exceeds allowed duration, fps, viewport or 900-frame budget")
    ffmpeg = shutil.which("ffmpeg")
    if not ffmpeg:
        raise MediaError("ffmpeg not installed on client")
    capture = Path(__file__).with_name("html_capture.mjs")
    node = shutil.which("node")
    start = time.monotonic()
    frames = max(1, round(fps*seconds))
    with tempfile.TemporaryDirectory(prefix="lilyco-frames-", dir=root) as folder:
        work = Path(folder)
        if node and capture.is_file():
            process([node, str(capture), str(source), str(work), str(frames),
                     str(fps), str(width), str(height)], timeout=300, cwd=root)
        else:
            try:
                from playwright.sync_api import sync_playwright
            except ImportError as e:
                raise MediaError("install Node.js + playwright-core (preferred) or Python playwright on client") from e
            with sync_playwright() as pw:
                chrome = os.environ.get("LILYCO_CHROME_BIN")
                browser = pw.chromium.launch(executable_path=chrome if chrome else None,
                                             channel=None if chrome else "chrome", headless=True,
                                             args=["--disable-background-networking"])
                try:
                    context = browser.new_context(viewport={"width":width,"height":height},
                                                  device_scale_factor=1, service_workers="block")
                    context.route("**/*", lambda route: route.continue_()
                                  if route.request.url.startswith(("file:", "data:", "blob:"))
                                  else route.abort())
                    page = context.new_page()
                    page.goto(source.as_uri(), wait_until="load", timeout=20000)
                    if not page.evaluate("typeof window.seek === 'function'"):
                        raise MediaError("HTML must provide window.seek(seconds)")
                    for i in range(frames):
                        page.evaluate("(t) => window.seek(t)", i/fps)
                        page.screenshot(path=str(work / f"frame-{i:05}.png"), animations="disabled")
                finally:
                    browser.close()
        temp_video = work / "render.mp4"
        process([ffmpeg, "-hide_banner", "-loglevel", "error", "-nostdin", "-y",
                 "-framerate", str(fps), "-i", str(work / "frame-%05d.png"),
                 "-c:v", "libx264", "-pix_fmt", "yuv420p", "-crf", "20",
                 "-movflags", "+faststart", str(temp_video)], timeout=180, cwd=root)
        if not temp_video.is_file() or temp_video.stat().st_size < 100:
            raise MediaError("HTML render did not produce a video")
        with out.open("xb") as target, temp_video.open("rb") as source_file:
            shutil.copyfileobj(source_file, target)
    return {"ok": True, "out": str(out), "bytes": out.stat().st_size,
            "frames": frames, "fps": fps, "backend": "chromium-ffmpeg-local",
            "duration_ms": round((time.monotonic()-start)*1000)}


def respond(message: dict[str, Any]) -> dict[str, Any] | None:
    if "id" not in message:
        return None
    id_ = message["id"]
    method = message.get("method")
    if method == "initialize":
        result = {"protocolVersion": "2024-11-05", "capabilities": {"tools": {}},
                  "serverInfo": {"name": "lilyco-media-plugins", "version": VERSION}}
    elif method == "tools/list":
        result = {"tools": TOOLS}
    elif method == "tools/call":
        p = message.get("params", {})
        if not isinstance(p, dict):
            return {"jsonrpc": "2.0", "id": id_, "error": {"code": -32602, "message": "invalid params"}}
        name, args = p.get("name"), p.get("arguments", {})
        try:
            if not isinstance(args, dict):
                raise MediaError("tool arguments must be object")
            if name == "manim_render":
                value = manim_render(args)
            elif name == "html_to_video":
                value = html_to_video(args)
            else:
                raise MediaError("tool not found")
            result = {"content": [{"type": "text", "text": json.dumps(value)}], "isError": False}
        except (MediaError, OSError, KeyError, TypeError) as e:
            result = {"content": [{"type": "text", "text": str(e)}], "isError": True}
    elif method == "ping":
        result = {}
    else:
        return {"jsonrpc": "2.0", "id": id_, "error": {"code": -32601, "message": "method not found"}}
    return {"jsonrpc": "2.0", "id": id_, "result": result}


def main() -> None:
    for line in sys.stdin:
        if not line.strip():
            continue
        try:
            payload = json.loads(line)
            result = respond(payload)
        except (ValueError, TypeError):
            result = {"jsonrpc": "2.0", "id": None, "error": {"code": -32700, "message": "parse error"}}
        if result is not None:
            print(json.dumps(result, ensure_ascii=False), flush=True)


if __name__ == "__main__":
    main()
