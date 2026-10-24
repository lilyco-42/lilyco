# Lilyco AI vision tools

Lilyco lvision provides MCP tool discovery and invocation using the existing #[derive(App)] registry.

## Real AI background removal (remove-background)

The new tool is registered with the other image tools through lvision --mcp and --list.
Arguments in generated JSON Schema: image (local input PNG/JPEG/WebP), out (optional NEW output PNG), and model (u2netp default, u2net, silueta, isnet-general-use).

Returned JSON: ok, input, out, model, backend=rembg-local, width, height, bytes, duration_ms. The AI agent should pass the returned out path to another tool, e.g. resize or image-info.

The Rust tool invokes the trusted rembg CLI on the SAME computer where lvision runs, not Lilyco business servers, without a shell. RemBg loads its local ONNX model using CPU or the locally configured GPU provider. First use may download/cache model weights on the client, not on the Lilyco server.

Installation on the AI agent CLIENT: Python 3.11-3.13 and pip install "rembg[cpu,cli]" (or supported "rembg[gpu,cli]"). Optionally configure LILYCO_REMBG_BIN with the absolute path of a trusted rembg executable; otherwise it is resolved from PATH.

A sample MCP host configuration:

{
  "mcpServers": {
    "lilyco-vision": {
      "command": "/path/to/lvision",
      "args": ["--mcp"]
    }
  }
}

Example tool call:
remove-background({"image":"/path/to/product.jpg","model":"isnet-general-use","out":"/path/to/product.nobg.png"})

Existing extract-foreground is a deterministic edge-based tool, not AI. The other registered tools include crop, resize, image-info, color analysis, pixel-diff, trace, and screenshot.

## Safety
- Valid image only, <=25MiB and <=50MP; no output overwrite.
- Output is written to a temporary sibling file, validated as PNG, and persisted after success.
- Fixed model allowlist excludes custom paths and remote/API backends; never accepts arbitrary shell command.
- Cancellation kills the child process; 300 second maximum.
- No remote pixels or generated file content sent to any Lilyco backend.
- If an agent runs on a REMOTE server, its MCP process would execute on that remote server. To use user device CPU/GPU, connect it to an MCP server running on the user device.
- Binary dependency availability and model licensing must be reviewed prior to commercial rollout. The browser WebGPU/WASM UI pipeline is a separate optional client, not the mechanism for this native MCP tool.
