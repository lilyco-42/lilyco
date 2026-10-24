"""Lilyco Tool Hub: dependency-free MCP stdio gateway for existing Lilyco binaries.

No file content or inference leaves the client computer. The config is trusted
local configuration; AI clients can only invoke discovered, namespaced tools.
"""
from __future__ import annotations
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import sys
from typing import Any

PROTOCOL = "2024-11-05"
MAX_RESPONSE = 4 * 1024 * 1024
DISCOVERY_TIMEOUT = 20
CALL_TIMEOUT = 300


class ProviderError(Exception):
    pass


class ToolHub:
    def __init__(self, config: dict[str, Any]):
        providers = config.get("servers", {})
        if not isinstance(providers, dict) or not providers:
            raise ValueError("config must contain nonempty servers map")
        self.providers = {}
        for alias, entry in providers.items():
            if not isinstance(alias, str) or not re.fullmatch(r"[a-z][a-z0-9_-]{0,31}", alias):
                raise ValueError("invalid provider alias")
            if not isinstance(entry, dict) or not isinstance(entry.get("command"), str):
                raise ValueError(f"invalid provider definition: {alias}")
            command = entry["command"]
            args = entry.get("args", ["--mcp"])
            if not command or not isinstance(args, list) or not all(isinstance(v, str) for v in args):
                raise ValueError(f"invalid command/args for {alias}")
            self.providers[alias] = (command, args)
        self.tools: dict[str, tuple[str, str]] = {}
        self.schemas: list[dict[str, Any]] | None = None
        self.status: dict[str, str] = {}

    def _run(self, alias: str, request: dict[str, Any], timeout: int) -> dict[str, Any]:
        command, args = self.providers[alias]
        messages = [
            {"jsonrpc": "2.0", "id": 1, "method": "initialize",
             "params": {"protocolVersion": PROTOCOL, "capabilities": {},
                        "clientInfo": {"name": "lilyco-toolhub", "version": "0.1.0"}}},
            {"jsonrpc": "2.0", "method": "notifications/initialized"},
            {"jsonrpc": "2.0", "id": 2, **request},
        ]
        payload = "".join(json.dumps(message, ensure_ascii=False) + "\n" for message in messages)
        try:
            child = subprocess.Popen(
                [command, *args], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                stderr=subprocess.PIPE, text=True, encoding="utf-8", errors="replace",
                shell=False,
            )
            try:
                stdout, stderr = child.communicate(payload, timeout=timeout)
            except subprocess.TimeoutExpired:
                child.kill()
                child.communicate()
                raise ProviderError(f"{alias}: local tool timeout after {timeout}s")
        except OSError as exc:
            raise ProviderError(f"{alias}: could not start installed client tool: {exc}") from exc
        if len(stdout.encode("utf-8")) > MAX_RESPONSE:
            raise ProviderError(f"{alias}: response exceeds 4 MiB")
        for line in stdout.splitlines():
            try:
                data = json.loads(line)
            except json.JSONDecodeError:
                continue
            if isinstance(data, dict) and data.get("id") == 2:
                if "error" in data:
                    raise ProviderError(f"{alias}: {data['error'].get('message', 'tool error')}")
                if "result" not in data:
                    raise ProviderError(f"{alias}: missing MCP result")
                return data["result"]
        brief = (stderr or "").strip().splitlines()[-1:] or []
        raise ProviderError(f"{alias}: no MCP response (exit {child.returncode}) " + " ".join(brief)[:160])

    def discover(self) -> list[dict[str, Any]]:
        if self.schemas is not None:
            return self.schemas
        schemas = [
            {"name": "hub_status", "description": "List active Lilyco local tool providers and connection status, without running compute.",
             "inputSchema": {"type": "object", "properties": {}}}
        ]
        routes: dict[str, tuple[str, str]] = {}
        for alias in self.providers:
            try:
                result = self._run(alias, {"method": "tools/list", "params": {}}, DISCOVERY_TIMEOUT)
                count = 0
                for schema in result.get("tools", []):
                    origin = schema.get("name")
                    if not isinstance(origin, str) or not origin:
                        continue
                    name = f"{alias}__{origin}"
                    item = dict(schema)
                    item["name"] = name
                    item["description"] = f"[{alias}] " + item.get("description", "")
                    schemas.append(item)
                    routes[name] = (alias, origin)
                    count += 1
                self.status[alias] = f"ready ({count} tools)"
            except (ProviderError, TypeError, AttributeError) as exc:
                self.status[alias] = f"unavailable: {exc}"
                print(f"toolhub: {alias}: unavailable", file=sys.stderr)
        self.tools = routes
        self.schemas = schemas
        return schemas

    def call(self, name: str, arguments: dict[str, Any]) -> dict[str, Any]:
        self.discover()
        if name == "hub_status":
            return {"content": [{"type": "text", "text": json.dumps(self.status, ensure_ascii=False)}], "isError": False}
        if name not in self.tools:
            raise ProviderError(f"unknown tool: {name}")
        if not isinstance(arguments, dict):
            raise ProviderError("tool arguments must be a JSON object")
        alias, original = self.tools[name]
        # Security rules are still enforced by each downstream Lilyco Registry.
        result = self._run(alias, {"method": "tools/call", "params": {"name": original, "arguments": arguments}}, CALL_TIMEOUT)
        if not isinstance(result, dict):
            raise ProviderError(f"{alias}: invalid tools/call response")
        return result

    def handle(self, request: Any) -> dict[str, Any] | None:
        if not isinstance(request, dict):
            return self._error(None, -32600, "invalid request")
        request_id = request.get("id")
        if "id" not in request:
            return None
        method = request.get("method")
        try:
            if method == "initialize":
                result = {"protocolVersion": PROTOCOL, "capabilities": {"tools": {}},
                          "serverInfo": {"name": "lilyco-toolhub", "version": "0.1.0"}}
            elif method == "ping":
                result = {}
            elif method == "tools/list":
                result = {"tools": self.discover()}
            elif method == "tools/call":
                params = request.get("params", {})
                if not isinstance(params, dict):
                    return self._error(request_id, -32602, "params must be object")
                result = self.call(params.get("name", ""), params.get("arguments", {}))
            else:
                return self._error(request_id, -32601, "method not found")
            return {"jsonrpc": "2.0", "id": request_id, "result": result}
        except ProviderError as exc:
            # Tool execution failure is an MCP tool error, not an unknown JSON-RPC method.
            if method == "tools/call":
                return {"jsonrpc": "2.0", "id": request_id, "result": {
                    "content": [{"type": "text", "text": str(exc)}], "isError": True}}
            return self._error(request_id, -32603, str(exc))

    @staticmethod
    def _error(request_id: Any, code: int, message: str) -> dict[str, Any]:
        return {"jsonrpc": "2.0", "id": request_id, "error": {"code": code, "message": message}}


def main() -> None:
    if hasattr(sys.stdin, "reconfigure"): sys.stdin.reconfigure(encoding="utf-8", errors="replace")
    if hasattr(sys.stdout, "reconfigure"): sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    parser = argparse.ArgumentParser()
    parser.add_argument("--config", required=True, help="Trusted local tools.json file")
    args = parser.parse_args()
    try:
        config = json.loads(Path(args.config).read_text(encoding="utf-8-sig"))
        hub = ToolHub(config)
    except (OSError, ValueError, TypeError) as exc:
        print(f"toolhub config error: {exc}", file=sys.stderr)
        sys.exit(2)
    for line in sys.stdin:
        if not line.strip():
            continue
        try:
            request = json.loads(line)
        except json.JSONDecodeError:
            result = ToolHub._error(None, -32700, "parse error")
        else:
            result = hub.handle(request)
        if result is not None:
            sys.stdout.write(json.dumps(result, ensure_ascii=False) + "\n")
            sys.stdout.flush()


if __name__ == "__main__":
    main()
