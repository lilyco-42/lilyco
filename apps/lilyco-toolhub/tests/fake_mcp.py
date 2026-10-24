import json
import sys

for line in sys.stdin:
    try:
        request = json.loads(line)
    except json.JSONDecodeError:
        continue
    if "id" not in request:
        continue
    method = request.get("method")
    if method == "initialize":
        result = {"protocolVersion": "2024-11-05", "capabilities": {"tools": {}}, "serverInfo": {"name": "fake", "version": "1"}}
    elif method == "tools/list":
        result = {"tools": [
            {"name": "Echo", "description": "Echo arguments", "inputSchema": {"type": "object", "properties": {"input": {"type": "string"}}}},
            {"name": "Denied", "description": "Read-only safety guard example", "inputSchema": {"type": "object"}},
        ]}
    elif method == "tools/call":
        name = request.get("params", {}).get("name")
        if name == "Denied":
            result = {"content": [{"type": "text", "text": "permission denied"}], "isError": True}
        else:
            result = {"content": [{"type": "text", "text": json.dumps({"original": name, "args": request["params"]["arguments"]})}], "isError": False}
    else:
        result = {}
    print(json.dumps({"jsonrpc": "2.0", "id": request["id"], "result": result}), flush=True)
