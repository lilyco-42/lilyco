"""Exact metadata exception contracts; actual NSIS/native gates remain separate."""
import os
import unittest
from nsis_payload import NSIS, UNPATCHED, verify_nsis_application


class NsisPayloadTests(unittest.TestCase):
    def test_only_the_exact_nsis_marker_change_is_accepted(self):
        original = b'MZ original prefix\0' + UNPATCHED + b'original suffix'
        installed = original.replace(UNPATCHED, NSIS)
        receipt = verify_nsis_application(original, installed)
        self.assertTrue(receipt['all_other_application_bytes_identical'])
        self.assertEqual(receipt['bundle_marker_offset'], len(b'MZ original prefix\0'))

    def test_changed_code_prefix_suffix_or_length_is_rejected(self):
        original = b'MZ prefix' + UNPATCHED + b'suffix'
        installed = original.replace(UNPATCHED, NSIS)
        for changed in [b'X' + installed[1:], installed[:-1] + b'X',
                        installed + b'additional payload', installed[:-1]]:
            with self.subTest(changed=changed), self.assertRaises(ValueError):
                verify_nsis_application(original, changed)

    def test_unpatched_or_wrong_installer_metadata_is_rejected(self):
        original = b'MZ' + UNPATCHED
        for changed in [original, original.replace(UNPATCHED, b'__TAURI_BUNDLE_TYPE_VAR_MSI')]:
            with self.subTest(changed=changed), self.assertRaises(ValueError):
                verify_nsis_application(original, changed)

    def test_missing_or_ambiguous_reviewed_markers_are_rejected(self):
        for original in [b'MZ no marker', b'MZ' + UNPATCHED + UNPATCHED]:
            with self.subTest(original=original), self.assertRaises(ValueError):
                verify_nsis_application(original, original.replace(UNPATCHED, NSIS))


if __name__ == '__main__':
    if os.environ.get('GITHUB_ACTIONS') != 'true':
        raise SystemExit('Office NSIS payload contracts only run in GitHub Actions')
    unittest.main()
