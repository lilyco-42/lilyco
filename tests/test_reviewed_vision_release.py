"""Release-policy unit regressions; behavior coverage remains in actual app CI."""
import importlib.util
import io
import json
from pathlib import Path
import unittest
import zipfile

spec = importlib.util.spec_from_file_location('reviewed_release', Path(__file__).parents[1] / 'scripts/reviewed_vision_release.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
COMMIT = 'a' * 40


class ReleasePolicyTests(unittest.TestCase):
    def test_failed_or_candidate_run_cannot_be_used(self):
        valid = {'head_sha': COMMIT, 'head_branch': 'main', 'event': 'push', 'name': 'Vision Studio Desktop',
                 'status': 'completed', 'conclusion': 'success', 'head_repository': {'full_name': module.REPO}}
        module.valid_run(valid, COMMIT)
        for field, value in [('conclusion', 'failure'), ('status', 'in_progress'), ('event', 'pull_request'),
                             ('head_branch', 'other'), ('head_sha', 'b' * 40)]:
            with self.subTest(field=field), self.assertRaises(ValueError):
                module.valid_run({**valid, field: value}, COMMIT)

    def test_missing_or_string_boolean_receipt_is_rejected(self):
        module.require_receipt({'commit': COMMIT, 'ok': True}, COMMIT, ['ok'])
        for receipt in ({'ok': True}, {'commit': COMMIT, 'ok': 'true'}, {'commit': 'b' * 40, 'ok': True}):
            with self.assertRaises(ValueError):
                module.require_receipt(receipt, COMMIT, ['ok'])

    def test_manifest_requires_exact_content_and_paths(self):
        for bad in ('digest', 'unlisted', 'traversal', 'source'):
            buffer = io.BytesIO()
            content = b'own test fixture, not a shipped binary'
            manifest = {'commit': 'b' * 40 if bad == 'source' else COMMIT,
                        'engine': 'lilyco-vision/lvision --mcp',
                        'files': {'own.txt': module.sha(content) if bad != 'digest' else '0' * 64}}
            with zipfile.ZipFile(buffer, 'w') as archive:
                archive.writestr(module.PREFIX + 'manifest.json', json.dumps(manifest))
                archive.writestr(module.PREFIX + 'own.txt', content)
                if bad == 'unlisted':
                    archive.writestr(module.PREFIX + 'unlisted.txt', content)
                if bad == 'traversal':
                    archive.writestr(module.PREFIX + '../outside.txt', content)
            with zipfile.ZipFile(io.BytesIO(buffer.getvalue())) as archive, self.assertRaises(ValueError):
                module.package_manifest(archive, COMMIT)


if __name__ == '__main__':
    unittest.main()
