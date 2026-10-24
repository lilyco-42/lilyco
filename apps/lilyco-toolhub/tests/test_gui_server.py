"""Security and human GUI HTTP contract tests, no real model work."""
from http.server import ThreadingHTTPServer
import json
from pathlib import Path
import tempfile
import threading
import unittest
from urllib.request import Request, urlopen
from urllib.error import HTTPError
import sys

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from gui_server import GUIState, make_handler


class FakeHub:
    def __init__(self, output: Path):
        self.output = output
        self.status = {"vision": "ready (1 tools)"}
        self.schemas = [{"name": "vision__remove-background", "inputSchema": {"type": "object", "properties": {"image": {"type": "string"}}}}]
        self.called = []

    def discover(self):
        return self.schemas

    def call(self, name, args):
        self.called.append((name, args))
        return {"content": [{"type": "text", "text": json.dumps({"ok": True, "out": str(self.output)})}], "isError": False}


class GUIContractTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmp = tempfile.TemporaryDirectory()
        folder = Path(cls.tmp.name)
        (folder / "index.html").write_text("<html>workbench</html>", encoding="utf-8")
        cls.output = folder / "result.png"
        cls.output.write_bytes(b"\x89PNG\r\n\x1a\n")
        cls.fake = FakeHub(cls.output)
        cls.state = GUIState(cls.fake, folder, token="unit-test-token")
        cls.server = ThreadingHTTPServer(("127.0.0.1", 0), make_handler(cls.state))
        cls.thread = threading.Thread(target=cls.server.serve_forever, daemon=True)
        cls.thread.start()
        cls.base = f"http://127.0.0.1:{cls.server.server_port}"

    @classmethod
    def tearDownClass(cls):
        cls.server.shutdown()
        cls.server.server_close()
        cls.thread.join(timeout=4)
        cls.tmp.cleanup()

    def get(self, path, token=None):
        headers = {} if token is None else {"X-Lilyco-Token": token}
        return urlopen(Request(self.base + path, headers=headers), timeout=5)

    def post(self, path, obj, token="unit-test-token", origin=None):
        headers = {
            "X-Lilyco-Token": token, "Content-Type": "application/json",
            "Origin": origin or self.base,
        }
        return urlopen(Request(self.base + path, data=json.dumps(obj).encode(), headers=headers), timeout=5)

    def test_page_is_local_and_api_requires_token(self):
        self.assertIn(b"workbench", self.get("/").read())
        with self.assertRaises(HTTPError) as error:
            self.get("/api/tools")
        self.assertEqual(error.exception.code, 401)
        result = json.load(self.get("/api/tools", "unit-test-token"))
        self.assertEqual(result["tools"][0]["name"], "vision__remove-background")

    def test_cross_site_and_wrong_token_cannot_invoke(self):
        request = {"name":"vision__remove-background", "arguments":{"image":"pic.png"}}
        for token, origin, code in [
            ("wrong-token", self.base, 401),
            ("unit-test-token", "https://attacker.example", 403),
        ]:
            with self.assertRaises(HTTPError) as error:
                self.post("/api/run", request, token=token, origin=origin)
            self.assertEqual(error.exception.code, code)
        self.assertEqual(self.fake.called, [])

    def test_same_origin_invokes_hub_and_creates_preview(self):
        request = {"name":"vision__remove-background", "arguments":{"image":"product.png"}}
        result = json.load(self.post("/api/run", request))
        self.assertFalse(result["result"]["isError"])
        self.assertEqual(self.fake.called[-1][0], request["name"])
        self.assertIsNotNone(result["run"]["preview"])
        artifact = self.get("/api/preview/" + result["run"]["preview"], "unit-test-token")
        self.assertEqual(artifact.read(), self.output.read_bytes())
        self.assertEqual(json.load(self.get("/api/history", "unit-test-token"))["runs"][0]["name"], request["name"])

    def test_modifying_local_files_needs_confirmation(self):
        data = {"name": "files__rename", "arguments": {"root":"."}}
        previous = len(self.fake.called)
        with self.assertRaises(HTTPError) as error:
            self.post("/api/run", data)
        self.assertEqual(error.exception.code, 409)
        self.assertEqual(len(self.fake.called), previous)
        data["confirmed"] = True
        self.assertEqual(self.post("/api/run", data).status, 200)

    def test_script_execution_needs_confirmation(self):
        data = {"name": "media__manim_render", "arguments": {"scene_file": "sample.py"}}
        with self.assertRaises(HTTPError) as error:
            self.post("/api/run", data)
        self.assertEqual(error.exception.code, 409)

    def test_native_picker_requires_authentication_and_origin(self):
        with self.assertRaises(HTTPError) as error:
            self.post("/api/pick", {"mode": "file"}, token="invalid")
        self.assertEqual(error.exception.code, 401)
        with self.assertRaises(HTTPError) as error:
            self.post("/api/pick", {"mode": "file"}, origin="https://external.example")
        self.assertEqual(error.exception.code, 403)

    def test_no_arbitrary_static_paths(self):
        with self.assertRaises(HTTPError) as error:
            self.get("/../../../secret.txt")
        self.assertEqual(error.exception.code, 404)


if __name__ == "__main__":
    unittest.main()

