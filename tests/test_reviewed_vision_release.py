"""Release-policy unit regressions; behavior coverage remains in actual app CI."""
import importlib.util
import io
import json
from pathlib import Path
import unittest
from unittest.mock import patch
import urllib.error
import zipfile

spec = importlib.util.spec_from_file_location('reviewed_release', Path(__file__).parents[1] / 'scripts/reviewed_vision_release.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
COMMIT = 'a' * 40


class ReleasePolicyTests(unittest.TestCase):
    def test_existing_draft_uses_authenticated_list_and_stable_id(self):
        tag = 'own-test-tag'
        asset = {'name': 'own.txt', 'size': 4, 'digest': 'sha256:' + module.sha(b'own\n')}
        draft = {'id': 42, 'tag_name': tag, 'draft': True, 'target_commitish': COMMIT, 'assets': [asset]}
        seen = []

        def fake_github(path):
            seen.append(path)
            if path == 'releases/tags/' + tag:
                raise urllib.error.HTTPError('https://api.github.com/test', 404, 'Not Found', {}, None)
            if path == 'releases?per_page=100&page=1':
                return [draft]
            if path == 'releases/42':
                return draft
            self.fail('Unexpected request ' + path)

        with patch.object(module, 'github', side_effect=fake_github), patch.object(module, 'tag_commit', side_effect=[None, COMMIT]), \
                patch.object(module.subprocess, 'run') as execute:
            module.publish({'tag': tag, 'source_commit': COMMIT, 'assets': [asset]}, Path('unused-test-path'))
        self.assertEqual([(['gh', 'release', 'edit', tag, '-R', module.REPO, '--draft=false', '--latest=false'],)],
                         [call.args for call in execute.call_args_list])
        self.assertIn('releases/42', seen)

    def test_draft_lookup_is_paginated_and_duplicates_are_rejected(self):
        missing = urllib.error.HTTPError('https://api.github.com/test', 404, 'Not Found', {}, None)
        unrelated = [{'tag_name': 'other-' + str(i)} for i in range(100)]
        intended = {'id': 7, 'tag_name': 'wanted', 'draft': True}
        with patch.object(module, 'github', side_effect=[missing, unrelated, [intended]]):
            self.assertEqual(intended, module.release_by_tag('wanted'))
        with patch.object(module, 'github', side_effect=[missing, [intended, {**intended, 'id': 8}]]):
            with self.assertRaises(ValueError):
                module.release_by_tag('wanted')

    def test_wrong_draft_target_cannot_upload_or_publish(self):
        draft = {'id': 42, 'draft': True, 'target_commitish': 'b' * 40, 'assets': []}
        with patch.object(module, 'release_by_tag', return_value=draft), patch.object(module, 'tag_commit', return_value=None), \
                patch.object(module, 'github', return_value=draft), patch.object(module.subprocess, 'run') as execute:
            with self.assertRaises(ValueError):
                module.publish({'tag': 'wanted', 'source_commit': COMMIT, 'assets': []}, Path('unused-test-path'))
            execute.assert_not_called()

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
