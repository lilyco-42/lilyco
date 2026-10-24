import json
from pathlib import Path
import os
import tempfile
import unittest
from unittest.mock import patch
import sys
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from media_mcp import MediaError, html_to_video, manim_render, respond, TOOLS, permission, trusted_file


class MediaPluginTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        (self.root/"scene.py").write_text("class Example: pass", encoding="utf-8")
        (self.root/"animation.html").write_text("<html><body><script>window.seek=function(t){}</script></body></html>", encoding="utf-8")
        self.env = patch.dict(os.environ, {
            "LILYCO_MEDIA_ROOT": str(self.root),
            "LILYCO_TRUST_LOCAL_MEDIA": "1"
        })
        self.env.start()

    def tearDown(self):
        self.env.stop()
        self.tmp.cleanup()

    def test_abilities_list_and_deny_unlisted(self):
        names = [tool["name"] for tool in TOOLS]
        self.assertEqual(names, ["manim_render", "html_to_video"])
        response = respond({"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"unknown","arguments":{}}})
        self.assertTrue(response["result"]["isError"])

    def test_explicit_user_consent(self):
        with patch.dict(os.environ, {"LILYCO_TRUST_LOCAL_MEDIA":"0"}):
            with self.assertRaises(MediaError):
                permission()

    def test_workspace_escape_protection(self):
        with self.assertRaises(MediaError):
            trusted_file("../scene.py", ".py", self.root)
        with self.assertRaises(MediaError):
            trusted_file(str(self.root/"notfound.html"), ".html", self.root)
        with self.assertRaises(MediaError):
            trusted_file(str(self.root/"animation.html"), ".py", self.root)

    def test_manim_arguments_validated(self):
        for scene in ["-bad", "A; rm", "../file"]:
            with self.assertRaises(MediaError):
                manim_render({"scene_file":"scene.py","scene":scene,"output":"out.mp4"})
        with self.assertRaises(MediaError):
            manim_render({"scene_file":"scene.py","scene":"Example","quality":"bad","output":"out.mp4"})

    def test_html_limits_before_loading_browser(self):
        for seconds in [0, -1, 60.1, 10000]:
            with self.assertRaises(MediaError):
                html_to_video({"html_file":"animation.html","output":"out.mp4","duration":seconds})
        with self.assertRaises(MediaError):
            html_to_video({"html_file":"animation.html","output":"out.mp4","duration":60,"fps":30})

    def test_tool_failures_are_mcp_errors_not_crashes(self):
        req = {"jsonrpc":"2.0","id":9,"method":"tools/call","params":{
            "name":"manim_render","arguments":{"scene_file":"missing.py","scene":"X","output":"x.mp4"}}}
        result = respond(req)
        self.assertTrue(result["result"]["isError"])


if __name__ == "__main__":
    unittest.main()
