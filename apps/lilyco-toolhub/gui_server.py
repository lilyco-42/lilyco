"""Loopback-only browser GUI for Lilyco Tool Hub.

The GUI is a *human* interface to the same ToolHub registry used by MCP.
No compute runs on Lilyco cloud servers. No unauthenticated tool execution.
"""
from __future__ import annotations

import argparse
import json
import logging
import mimetypes
import os
from pathlib import Path
import re
import secrets
import threading
import urllib.parse
import webbrowser
from collections import deque
from datetime import datetime, timezone
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import Any

from hub import ToolHub, ProviderError

MAX_POST_BYTES = 256 * 1024
MAX_HISTORY = 100
MAX_PREVIEW_BYTES = 40 * 1024 * 1024
ALLOWED_PREVIEW_EXT = {".png", ".jpg", ".jpeg", ".webp", ".gif", ".mp4", ".webm"}
API_PREFIX = "/api/"
LOG = logging.getLogger("lilyco-toolhub-gui")


class GUIState:
    def __init__(self, hub: ToolHub, static_root: Path, token: str | None = None):
        self.hub = hub
        self.static_root = static_root.resolve()
        self.token = token or secrets.token_urlsafe(32)
        self.history: deque[dict[str, Any]] = deque(maxlen=MAX_HISTORY)
        self.artifacts: dict[str, Path] = {}
        self.lock = threading.RLock()

    def record(self, tool: str, arguments: dict[str, Any], result: dict[str, Any]) -> dict[str, Any]:
        # Never store full tool arguments: they may include sensitive paths or text.
        item: dict[str, Any] = {
            "id": secrets.token_hex(8),
            "name": tool,
            "at": datetime.now(timezone.utc).isoformat(),
            "ok": not bool(result.get("isError")),
            "preview": None,
        }
        if not result.get("isError"):
            for content in result.get("content", []):
                if content.get("type") != "text":
                    continue
                try:
                    payload = json.loads(content.get("text", ""))
                except (ValueError, TypeError):
                    continue
                output = payload.get("out") if isinstance(payload, dict) else None
                if isinstance(output, str):
                    path = Path(output).resolve()
                    if path.is_file() and path.suffix.lower() in ALLOWED_PREVIEW_EXT:
                        try:
                            if path.stat().st_size <= MAX_PREVIEW_BYTES:
                                key = secrets.token_urlsafe(12)
                                self.artifacts[key] = path
                                item["preview"] = key
                        except OSError:
                            pass
                    break
        with self.lock:
            self.history.appendleft(item)
            # Do not retain unbounded file handles or preview references.
            alive = {h["preview"] for h in self.history if h.get("preview")}
            self.artifacts = {k: v for k, v in self.artifacts.items() if k in alive}
        return item


def make_handler(state: GUIState):
    class Handler(BaseHTTPRequestHandler):
        server_version = "LilycoLocalGUI/0.1"
        def log_message(self, format: str, *args: Any) -> None:
            LOG.info("%s %s", self.address_string(), format % args)

        def _json(self, status: int, value: Any) -> None:
            data = json.dumps(value, ensure_ascii=False).encode("utf-8")
            self.send_response(status)
            self.send_header("Content-Type", "application/json; charset=utf-8")
            self.send_header("Content-Length", str(len(data)))
            self.send_header("Cache-Control", "no-store")
            self.send_header("X-Content-Type-Options", "nosniff")
            self.send_header("Content-Security-Policy", "default-src 'none'")
            self.end_headers()
            self.wfile.write(data)

        def _authorized(self) -> bool:
            if not secrets.compare_digest(self.headers.get("X-Lilyco-Token", ""), state.token):
                self._json(401, {"error": "Unauthorized client. Open the GUI from its launcher."})
                return False
            return True

        def _origin_ok(self) -> bool:
            origin = self.headers.get("Origin", "")
            # Browsers send the Origin header for JSON POST; requests via
            # file://, cross-origin websites, etc. must not execute tools.
            allowed = f"http://127.0.0.1:{self.server.server_port}"
            if origin != allowed:
                self._json(403, {"error": "Invalid origin"})
                return False
            return True

        def do_GET(self) -> None:
            route = urllib.parse.urlsplit(self.path).path
            if route.startswith(API_PREFIX):
                if not self._authorized():
                    return
                if route == "/api/status":
                    state.hub.discover()
                    return self._json(200, {
                        "name": "Lilyco Tool Hub", "local": True,
                        "providers": state.hub.status,
                        "tools": len(state.hub.schemas or []),
                    })
                if route == "/api/tools":
                    return self._json(200, {"tools": state.hub.discover()})
                if route == "/api/history":
                    with state.lock:
                        return self._json(200, {"runs": list(state.history)})
                if route.startswith("/api/preview/"):
                    key = route.removeprefix("/api/preview/")
                    with state.lock:
                        path = state.artifacts.get(key)
                    if path is None:
                        return self._json(404, {"error": "Unknown output"})
                    try:
                        if path.stat().st_size > MAX_PREVIEW_BYTES:
                            return self._json(413, {"error": "File is too large"})
                        content = path.read_bytes()
                    except OSError:
                        return self._json(404, {"error": "Output unavailable"})
                    mime = mimetypes.guess_type(path.name)[0] or "application/octet-stream"
                    self.send_response(200)
                    self.send_header("Content-Type", mime)
                    self.send_header("Content-Length", str(len(content)))
                    self.send_header("Cache-Control", "no-store")
                    self.send_header("X-Content-Type-Options", "nosniff")
                    self.send_header("Content-Security-Policy", "sandbox")
                    self.end_headers()
                    self.wfile.write(content)
                    return
                return self._json(404, {"error": "Not found"})

            if route not in ("/", "/index.html") and not re.fullmatch(r"/assets/[A-Za-z0-9._-]+", route):
                return self._json(404, {"error": "Not found"})
            if route == "/":
                route = "/index.html"
            file = (state.static_root / route.lstrip("/")).resolve()
            if not file.is_relative_to(state.static_root) or not file.is_file():
                return self._json(404, {"error": "GUI not built; run bun run build in apps/lilyco-toolhub/gui"})
            content = file.read_bytes()
            self.send_response(200)
            self.send_header("Content-Type", mimetypes.guess_type(file.name)[0] or "text/html")
            self.send_header("Content-Length", str(len(content)))
            self.send_header("Cache-Control", "no-store")
            self.send_header("X-Content-Type-Options", "nosniff")
            self.send_header("Referrer-Policy", "no-referrer")
            self.send_header("Content-Security-Policy",
                             "default-src 'self'; connect-src 'self'; img-src 'self' data: blob:; "
                             "style-src 'self' 'unsafe-inline'; script-src 'self'; font-src 'self'; media-src 'self' blob:; "
                             "object-src 'none'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'")
            self.end_headers()
            self.wfile.write(content)

        def do_POST(self) -> None:
            route = urllib.parse.urlsplit(self.path).path
            if route not in ("/api/run", "/api/pick"):
                return self._json(404, {"error": "Not found"})
            if not self._authorized() or not self._origin_ok():
                return
            if not self.headers.get("Content-Type", "").startswith("application/json"):
                return self._json(415, {"error": "JSON required"})
            try:
                length = int(self.headers.get("Content-Length", "-1"))
            except ValueError:
                return self._json(400, {"error": "Invalid length"})
            if length <= 0 or length > MAX_POST_BYTES:
                return self._json(413, {"error": "Request exceeds 256KiB"})
            try:
                data = json.loads(self.rfile.read(length).decode("utf-8"))
            except (ValueError, UnicodeDecodeError):
                return self._json(400, {"error": "Invalid JSON"})
            if route == "/api/pick":
                return self._pick_local(data if isinstance(data, dict) else {})
            if not isinstance(data, dict) or not isinstance(data.get("name"), str) or not isinstance(data.get("arguments"), dict):
                return self._json(400, {"error": "Expected name and arguments object"})
            name = data["name"]
            # Browser submissions require additional human confirmation for
            # tools with potentially destructive side effects.
            confirmed = data.get("confirmed") is True
            suspicious = re.compile(r"(rename|delete|remove|write|move|replace|reset|exec|shell|install|manim_render|html_to_video)", re.I)
            if suspicious.search(name) and name != "vision__remove-background" and not confirmed:
                return self._json(409, {"error": "This tool needs explicit confirmation"})
            try:
                result = state.hub.call(name, data["arguments"])
            except ProviderError as exc:
                result = {"content": [{"type": "text", "text": str(exc)}], "isError": True}
            record = state.record(name, data["arguments"], result)
            return self._json(200, {"result": result, "run": record})

        def _pick_local(self, data: dict[str, Any]) -> None:
            """Native OS file chooser. Returned path only; never reads file data."""
            mode = data.get("mode")
            if mode not in ("file", "save", "folder"):
                return self._json(400, {"error": "Unsupported picker mode"})
            try:
                import tkinter as tk
                from tkinter import filedialog
                root = tk.Tk()
                root.withdraw()
                root.attributes("-topmost", True)
                try:
                    if mode == "folder":
                        filename = filedialog.askdirectory(parent=root, title="选择本地文件夹")
                    elif mode == "save":
                        filename = filedialog.asksaveasfilename(
                            parent=root, title="选择保存位置",
                            filetypes=[("PNG 图片", "*.png"), ("MP4 视频", "*.mp4"), ("全部文件", "*.*")])
                    else:
                        filename = filedialog.askopenfilename(
                            parent=root, title="选择本地文件",
                            filetypes=[("图片和媒体", "*.png *.jpg *.jpeg *.webp *.gif *.mp4 *.html *.py"),
                                       ("全部文件", "*.*")])
                finally:
                    root.destroy()
            except Exception as exc:
                LOG.warning("native picker unavailable: %s", type(exc).__name__)
                return self._json(503, {"error": "系统文件选择器暂不可用，请手动粘贴本地路径"})
            return self._json(200, {"path": filename or ""})
    return Handler


def main() -> None:
    parser = argparse.ArgumentParser(description="Lilyco local browser GUI")
    parser.add_argument("--config", required=True)
    parser.add_argument("--port", type=int, default=0, help="0 chooses a random free loopback port")
    parser.add_argument("--no-browser", action="store_true")
    args = parser.parse_args()
    hub = ToolHub(json.loads(Path(args.config).read_text(encoding="utf-8-sig")))
    static_root = (Path(__file__).parent / "gui" / "dist").resolve()
    if not (static_root / "index.html").exists():
        raise SystemExit("GUI assets missing; run bun run build in apps/lilyco-toolhub/gui")
    state = GUIState(hub, static_root)
    with ThreadingHTTPServer(("127.0.0.1", args.port), make_handler(state)) as server:
        url = f"http://127.0.0.1:{server.server_port}/#token={state.token}"
        print("LILYCO_TOOLHUB_GUI_URL=" + url, flush=True)
        if not args.no_browser:
            webbrowser.open(url)
        try:
            server.serve_forever()
        except KeyboardInterrupt:
            pass


if __name__ == "__main__":
    main()
