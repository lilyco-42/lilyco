"""Reject foreign/ambiguous browser endpoints in the real native acceptance host."""
import os
import unittest
from native_workbench import compare_cli_content, verify_debug_endpoint


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

    def test_only_pdf_invocation_duration_can_differ(self):
        expected = {'elapsed_ms': 2, 'pages': [{'text': 'original'}]}
        actual = {'elapsed_ms': 3, 'pages': [{'text': 'original'}]}
        self.assertEqual(compare_cli_content(expected, actual, 'pdf'),
                         {'independent_cli_elapsed_ms': 2, 'ui_invocation_elapsed_ms': 3})
        with self.assertRaises(AssertionError):
            compare_cli_content(expected, actual, 'docx')
        for changed in [{'elapsed_ms': 3, 'pages': []},
                        {'elapsed_ms': 3, 'pages': [{'text': 'original'}], 'extra': 1},
                        {'elapsed_ms': True, 'pages': [{'text': 'original'}]},
                        {'elapsed_ms': -1, 'pages': [{'text': 'original'}]},
                        {'pages': [{'text': 'original'}]}]:
            with self.subTest(changed=changed), self.assertRaises(AssertionError):
                compare_cli_content(expected, changed, 'pdf')


if __name__ == '__main__':
    if os.environ.get('GITHUB_ACTIONS') != 'true':
        raise SystemExit('Native endpoint checks only run in GitHub Actions')
    unittest.main()
