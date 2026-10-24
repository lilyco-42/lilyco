"""Generate portable Tool Hub provider configuration from a downloaded release bundle."""
import json
import os
from pathlib import Path
import sys

folder = Path(sys.argv[1] if len(sys.argv) > 1 else ".").resolve()
name_to_binary = {
    "vision": "lvision", "files": "lfiles", "grep": "lgrep",
    "ffmpeg": "lffmpeg", "mpkg": "lmpkg",
}
extension = ".exe" if os.name == "nt" else ""
servers = {}
for name, binary in name_to_binary.items():
    executable = folder / (binary + extension)
    if not executable.is_file():
        raise SystemExit(f"Missing {name} binary: {executable}")
    servers[name] = {"command": str(executable), "args": ["--mcp"]}
plugin = folder / "media_mcp.py"
if not plugin.is_file():
    raise SystemExit(f"Missing media MCP script: {plugin}")
servers["media"] = {"command": sys.executable, "args": ["-u", str(plugin)]}
out = folder / "tools.json"
out.write_text(json.dumps({"servers": servers}, indent=2, ensure_ascii=False), encoding="utf-8")
print("Generated trusted Tool Hub config:", out)
