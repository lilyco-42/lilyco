import json
from pathlib import Path
import subprocess
import sys
import unittest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from hub import ToolHub

FAKE = str(ROOT / "tests" / "fake_mcp.py")


class ToolHubTests(unittest.TestCase):
    def setUp(self):
        self.hub = ToolHub({"servers": {
            "vision": {"command": sys.executable, "args": [FAKE]},
            "files": {"command": sys.executable, "args": [FAKE]}
        }})

    def test_all_tools_namespaced_and_routed(self):
        tools = self.hub.discover()
        names = [t["name"] for t in tools]
        self.assertEqual(len(names), len(set(names)))
        self.assertIn("vision__Echo", names)
        self.assertIn("files__Echo", names)
        result = self.hub.call("vision__Echo", {"input": "photo.jpg"})
        parsed = json.loads(result["content"][0]["text"])
        self.assertEqual(parsed["original"], "Echo")
        self.assertEqual(parsed["args"]["input"], "photo.jpg")

    def test_downstream_safety_denial_unchanged(self):
        result = self.hub.call("files__Denied", {})
        self.assertTrue(result["isError"])
        self.assertIn("permission denied", result["content"][0]["text"])

    def test_unknown_tool_does_not_launch_any_program(self):
        response = self.hub.handle({"jsonrpc": "2.0", "id": 10, "method": "tools/call",
                                    "params": {"name": "unlisted__unsafe", "arguments": {}}})
        self.assertTrue(response["result"]["isError"])
        self.assertIn("unknown tool", response["result"]["content"][0]["text"])

    def test_missing_provider_is_reported_not_fatal(self):
        hub = ToolHub({"servers": {"offline": {"command": "this-does-not-exist-lilyco123"}}})
        self.assertEqual(len(hub.discover()), 1)
        self.assertIn("unavailable", hub.status["offline"])

    def test_protocol_initialize_and_notification(self):
        result = self.hub.handle({"jsonrpc": "2.0", "id": 1, "method": "initialize"})
        self.assertEqual(result["result"]["serverInfo"]["name"], "lilyco-toolhub")
        self.assertIsNone(self.hub.handle({"jsonrpc": "2.0", "method": "notifications/initialized"}))
        response = self.hub.handle({"jsonrpc": "2.0", "id": 3, "method": "tools/list", "params": {}})
        self.assertEqual(len(response["result"]["tools"]), 5)

    def test_mcp_stdio_end_to_end(self):
        cfg = ROOT / "tests" / "_toolhub_test_config.json"
        try:
            cfg.write_text(json.dumps({"servers": {"vision": {
                "command": sys.executable, "args": [FAKE]
            }}}), encoding="utf-8")
            requests = [
                {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}},
                {"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}},
                {"jsonrpc": "2.0", "id": 3, "method": "tools/call",
                 "params": {"name": "vision__Echo", "arguments": {"input": "hello"}}},
            ]
            proc = subprocess.run(
                [sys.executable, str(ROOT / "hub.py"), "--config", str(cfg)],
                input="".join(json.dumps(req) + "\n" for req in requests),
                text=True, capture_output=True, timeout=15,
            )
            self.assertEqual(proc.returncode, 0, proc.stderr)
            rows = [json.loads(line) for line in proc.stdout.splitlines()]
            self.assertEqual([row["id"] for row in rows], [1, 2, 3])
            self.assertIn("vision__Echo", [t["name"] for t in rows[1]["result"]["tools"]])
            self.assertIn("hello", rows[2]["result"]["content"][0]["text"])
        finally:
            cfg.unlink(missing_ok=True)


if __name__ == "__main__":
    unittest.main()
