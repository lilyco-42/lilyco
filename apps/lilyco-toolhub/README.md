# Lilyco Tool Hub — one MCP endpoint, multiple existing Lilyco binaries

The hub uses ONLY Python standard library. No model API, inference or file processing is performed in the hub itself. It discovers existing Lilyco tools through their native MCP servers and exposes aliases with namespaces, e.g. vision__remove-background, files__Find, grep__Grep, ffmpeg__Ffmpeg, mpkg__MpkgGet. The actual names are read from tools/list (no hardcoded guesses).

Install each CLI binary locally, make a trusted local JSON config (see tools.example.json), then start:

python apps/lilyco-toolhub/hub.py --config /path/to/tools.json

Point your agent MCP client to that command using stdio. The hub returns the union of all reachable providers through one tools/list and forwards tools/call to the specific downstream process on the SAME DEVICE.

Notes:
- Clients with no installed optional binary still see all working providers and hub_status.
- The hub NEVER accepts arbitrary executables as tool arguments; executable paths are read only from the trusted local JSON config.
- Call tools are always prefixed alias__original_name to avoid collisions; downstream registry retains its own safety policy. For example lfiles rename is a protected T1 action, and the hub NEVER bypasses the default MCP safety gate.
- Each call starts a local child process without shell, with a 300s timeout and a 4MiB response limit. No server compute. Child has access to the same local paths as the hub user. Use OS permissions to limit file access; trusted AI callers can invoke permitted tools.
- The first tools/list causes discovery subprocesses. Discovery is cached per hub process; restart the hub to reflect newly installed providers.
- Current gateway does not yet relay live progress/cancellation notifications from subprocesses. Clients still get the final result or a timeout error; future improvement can keep persistent subprocesses, stream progress, and propagate cancellation.
- Do not expose stdio hub directly over a public unauthenticated HTTP endpoint.
- WebGPU/WASM browser code is separate from OS-native MCP; the native ONNX tools run on the user's local CPU or configured GPU backend.

Quality gates:

python -m unittest discover -s apps/lilyco-toolhub/tests -v

## Optional media plugins

The bundled `media_mcp.py` adds `media__manim_render` and `media__html_to_video` when configured as a `media` provider in the Tool Hub.

- Manim: AI selects a pre-existing, trusted local `.py` Manim scene and a new output `.mp4`; Manim Community executes locally (no cloud API). This executes user Python code and is disabled until explicit local opt-in.
- HTML-to-video: AI supplies a trusted local `.html` exposing `window.seek(seconds)`, output `.mp4`, duration/FPS/resolution. On-device Chromium renders deterministic screenshots frame-by-frame, and FFmpeg encodes them into a local MP4. This follows the already working HTML→video design in `lilyco-42/lilyco-brand` (which used hardcoded paths and a standalone script); the adapter replaces hardcoded paths with safe MCP arguments.
- For both tools, an OS user must set `LILYCO_TRUST_LOCAL_MEDIA=1` and `LILYCO_MEDIA_ROOT` to a trusted local working directory; paths outside it are forbidden and existing MP4 output is not overwritten.
- `html_to_video` uses Node.js + `playwright-core` (preferred, install once via `bun install --production` inside the local artifact folder), Chrome and FFmpeg. Python `playwright` is an optional fallback. All external HTTP(S) requests are blocked by browser routing; HTML assets must be locally available. True animated CSS timelines without `window.seek(t)` are not supported in this deterministic mode.
- Runs up to 60 seconds/900 frames/1920×1080; max output 300s renderer subprocess time; no provider API keys or Lilyco server computation. All nested binaries run locally without shell.
- `ly_motion` is a separate Rust animation-engine project, not yet hooked into the MCP interface because no stable CLI/ABI has been confirmed.

Manim and HTML can run executable Python/JavaScript; **only put trusted local project files into LILYCO_MEDIA_ROOT.** Do not allow an agent to populate that folder with unreviewed arbitrary scripts and auto-render without human code review. The browser uses Chrome's sandbox but it is not a substitute for running untrusted documents in an OS container.

## 如何让 AI 调用媒体插件

Manim Community:
- 把已审核的 `demo.py` 放入 `LILYCO_MEDIA_ROOT`。
- AI 工具 `media__manim_render`：
  `{"scene_file":"demo.py","scene":"Demo","output":"demo-manim.mp4","quality":"l"}`
- 使用系统已安装的 manim CLI，运行 Python Scene 并输出 MP4 到同一受信目录。
- 仅允许本机确认过的源代码。生成的 Python 动画脚本应由用户先审核，再允许 AI 渲染。

HTML → Video:
- 把已审核的 `promo.html` 放入 `LILYCO_MEDIA_ROOT`，确保脚本提供 `window.seek(t)`，每次调用可按时间 t 设置动画画面。
- AI 工具 `media__html_to_video`：
  `{"html_file":"promo.html","output":"promo.mp4","duration":5,"fps":24,"width":1280,"height":720}`
- 浏览器在用户设备逐帧截图，本机 FFmpeg 编码为 MP4；浏览器拦截任何外部 HTTP(S) 请求。首选确定性动画，而不是对实时 CSS Animation 录屏。
- 需要客户端 Python Playwright、Chrome/Chromium 和 FFmpeg。若所用 HTML 引用远程字体/资源，必须先转为本地文件。

环境设置：
```
LILYCO_MEDIA_ROOT=<local-trusted-project-dir>
LILYCO_TRUST_LOCAL_MEDIA=1
```
这两个设置必须由用户本人在本地客户端设置，不应由远程模型对任意目录自动开启。媒体插件不执行未经用户授权的任意网络任务，也不应部署在业务服务器上。

相关项目：
- `lilyco-42/lilyco-brand` 的 `render.js` 以 `window.seek(t)` + Playwright 截帧 + FFmpeg 验证过相同渲染思路；新插件抽象成可传参 MCP API，不复制其硬编码路径、歌曲或品牌商标素材。
- `lilyco-42/ly_motion` 是 Rust 动效引擎规划和实现仓库，未来在明确 CLI/ABI 后可作为第三类本地渲染工具接入。

## Install downloaded CI artifact (Windows)

1. Download the platform artifact lilyco-toolhub-Windows and unzip into a local folder; keep the five .exe tools, hub.py, media_mcp.py, html_capture.mjs, package.json, and prepare_bundle.py together.
2. In this folder run un install --production to install pinned lightweight Playwright browser driver, and python prepare_bundle.py . to create a device-local trusted 	ools.json (never reuse CI runner paths).
3. Set env LILYCO_MEDIA_ROOT to a project directory and LILYCO_TRUST_LOCAL_MEDIA=1 only after reviewing the scripts it contains. Install Manim Community CLI, Chrome, FFmpeg on the client.
4. Start python -u hub.py --config tools.json as a stdio MCP server. It exposes media__manim_render, media__html_to_video plus existing vision/files/grep/ffmpeg/mpkg tools.
5. Rendering takes place on the client with limits; HTML must implement window.seek(seconds). Returned output paths can be passed to fmpeg__Ffmpeg for further encoding/trim.
