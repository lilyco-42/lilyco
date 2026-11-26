"""Refuse altered provenance and incomplete original notices; Actions only."""
import base64
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
import unittest
from unittest.mock import patch
import warnings
import zipfile

OFFICE = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('rust_notices', OFFICE / 'collect_rust_notices.py')
notices = importlib.util.module_from_spec(spec)
spec.loader.exec_module(notices)


class ProvenancePolicy(unittest.TestCase):
    def test_sdk_bytes_and_catalog_must_identify_the_same_archive(self):
        archive = b'official archive fixture'
        catalog = {'packageSize': len(archive), 'packageHashAlgorithm': 'SHA512',
                   'packageHash': base64.b64encode(hashlib.sha512(archive).digest()).decode('ascii')}
        notices.verify_sdk_archive(archive, catalog)
        for changed in [archive + b'changed', b'X' + archive[1:]]:
            with self.assertRaises(ValueError):
                notices.verify_sdk_archive(changed, catalog)
        with self.assertRaisesRegex(ValueError, 'SHA512'):
            notices.verify_sdk_archive(archive, {**catalog, 'packageHashAlgorithm': 'SHA256'})

    def test_sdk_member_is_bounded_and_cannot_have_duplicate_names(self):
        buffer = io.BytesIO()
        with zipfile.ZipFile(buffer, 'w') as archive:
            archive.writestr('LICENSE.txt', b'complete original text')
        with zipfile.ZipFile(io.BytesIO(buffer.getvalue())) as archive:
            self.assertEqual(notices.original_sdk_member(archive, 'LICENSE.txt', 256), b'complete original text')
            with self.assertRaisesRegex(ValueError, 'oversized'):
                notices.original_sdk_member(archive, 'LICENSE.txt', 5)
        duplicate = io.BytesIO()
        with warnings.catch_warnings():
            warnings.simplefilter('ignore', UserWarning)
            with zipfile.ZipFile(duplicate, 'w') as archive:
                archive.writestr('LICENSE.txt', b'original text')
                archive.writestr('LICENSE.txt', b'replaced text')
        with zipfile.ZipFile(io.BytesIO(duplicate.getvalue())) as archive:
            with self.assertRaisesRegex(ValueError, 'duplicated'):
                notices.original_sdk_member(archive, 'LICENSE.txt', 256)

    def test_notice_preserves_original_encoding_and_rejects_empty_or_escaped_files(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            package = root / 'package'
            package.mkdir()
            license_file = package / 'LICENSE'
            original = b'copyright \xa9 complete original notice'
            license_file.write_bytes(original)
            report = notices.read_notice(package, license_file)
            self.assertEqual(base64.b64decode(report['original_base64']), original)
            self.assertEqual(report['sha256'], hashlib.sha256(original).hexdigest())
            license_file.write_bytes(b'')
            with self.assertRaisesRegex(ValueError, 'empty'):
                notices.read_notice(package, license_file)
            outside = root / 'LICENSE'
            outside.write_bytes(original)
            with self.assertRaisesRegex(ValueError, 'escaped'):
                notices.read_notice(package, outside)

    def test_provenance_rejects_redirects_to_other_hosts_and_embedded_credentials(self):
        for url in ['http://api.nuget.org/v3/index.json',
                    'https://api.nuget.org.evil.test/index.json',
                    'https://secret@api.nuget.org/v3/index.json',
                    'https://raw.githubusercontent.com:444/file']:
            with self.assertRaisesRegex(ValueError, 'origin'):
                notices.provenance_url(url)

    def test_cargo_lock_seals_actual_original_member_without_vendor_metadata(self):
        buffer = io.BytesIO()
        payload = b'original static library fixture'
        path = 'bindings-1.0/x64/WebView2LoaderStatic.lib'
        with tarfile.open(fileobj=buffer, mode='w:gz') as archive:
            member = tarfile.TarInfo(path)
            member.size = len(payload)
            archive.addfile(member, io.BytesIO(payload))
        data = buffer.getvalue()
        checksum = hashlib.sha256(data).hexdigest()
        notices.verify_crate_archive(data, checksum)
        self.assertEqual(notices.original_crate_member(data, path, 1024), payload)
        with self.assertRaisesRegex(ValueError, 'Cargo.lock'):
            notices.verify_crate_archive(data + b'changed', checksum)
        with self.assertRaisesRegex(ValueError, 'missing'):
            notices.original_crate_member(data, path + '.changed', 1024)
        with self.assertRaisesRegex(ValueError, 'oversized'):
            notices.original_crate_member(data, path, 4)

    def test_actual_collector_cannot_run_outside_actions(self):
        environment = dict(os.environ)
        environment.pop('GITHUB_ACTIONS', None)
        result = subprocess.run([sys.executable, str(OFFICE / 'collect_rust_notices.py')],
                                env=environment, capture_output=True, text=True, timeout=10)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('only runs in GitHub Actions', result.stderr)

    def test_upstream_notice_requires_exact_locked_vcs_and_original_bytes(self):
        package = {'name': 'example', 'version': '1.0', 'repository': 'https://github.com/example/library'}
        original = b'Complete original copyright and permission notice'
        policy = {'repository': package['repository'], 'commit': 'a' * 40, 'package_path': 'example',
                  'url': 'https://raw.githubusercontent.com/example/library/' + 'a' * 40 + '/LICENSE',
                  'sha256': hashlib.sha256(original).hexdigest()}
        vcs = {'git': {'sha1': policy['commit']}, 'path_in_vcs': policy['package_path']}
        def archive(info):
            data = json.dumps(info).encode()
            output = io.BytesIO()
            with tarfile.open(fileobj=output, mode='w:gz') as tar:
                member = tarfile.TarInfo('example-1.0/.cargo_vcs_info.json');member.size = len(data)
                tar.addfile(member, io.BytesIO(data))
            return output.getvalue()
        with patch.dict(notices.UPSTREAM_NOTICES, {('example', '1.0'): policy}), patch.object(notices, 'fetch', return_value=original):
            result = notices.collect_upstream_notice(package, archive(vcs))
            self.assertEqual(base64.b64decode(result['LICENSE']['original_base64']), original)
            for info in [{**vcs, 'git': {'sha1': 'b' * 40}},
                         {**vcs, 'git': {'sha1': 'a' * 40, 'dirty': True}},
                         {**vcs, 'path_in_vcs': 'another-package'}]:
                with self.assertRaisesRegex(ValueError, 'VCS'):
                    notices.collect_upstream_notice(package, archive(info))
            with self.assertRaisesRegex(ValueError, 'repository'):
                notices.collect_upstream_notice({**package, 'repository': 'https://github.com/other/library'}, archive(vcs))
            with patch.object(notices, 'fetch', return_value=original + b'changed'), self.assertRaisesRegex(ValueError, 'bytes'):
                notices.collect_upstream_notice(package, archive(vcs))


if __name__ == '__main__':
    if os.environ.get('GITHUB_ACTIONS') != 'true':
        raise SystemExit('Office provenance tests only run in GitHub Actions')
    unittest.main()
