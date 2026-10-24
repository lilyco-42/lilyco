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
