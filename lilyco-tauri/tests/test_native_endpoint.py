"""Reject foreign/ambiguous browser endpoints in the real native acceptance host."""
import os
import unittest
from native_workbench import verify_debug_endpoint


class NativeEndpointTests(unittest.TestCase):
    def endpoint(self, url):
        return {'Browser': 'Edg/153.0.4234.32', 'webSocketDebuggerUrl': url}

    def test_ci_owned_loopback_endpoint(self):
        value = self.endpoint('ws://127.0.0.1:45678/devtools/browser/native-id')
        self.assertIs(verify_debug_endpoint(value, 45678), value)

    def test_remote_wrong_port_credentials_and_non_browser_targets_rejected(self):
        for url in ['ws://example.com:45678/devtools/browser/native-id',
                    'ws://127.0.0.1:45679/devtools/browser/native-id',
                    'ws://user:secret@127.0.0.1:45678/devtools/browser/native-id',
                    'ws://127.0.0.1:45678/devtools/page/native-id',
                    'https://127.0.0.1:45678/devtools/browser/native-id']:
            with self.subTest(url=url), self.assertRaises(ValueError):
                verify_debug_endpoint(self.endpoint(url), 45678)

    def test_missing_runtime_identity_rejected(self):
        with self.assertRaises(ValueError):
            verify_debug_endpoint({'webSocketDebuggerUrl': 'ws://127.0.0.1:45678/devtools/browser/id'}, 45678)


if __name__ == '__main__':
    if os.environ.get('GITHUB_ACTIONS') != 'true':
        raise SystemExit('Native endpoint checks only run in GitHub Actions')
    unittest.main()
