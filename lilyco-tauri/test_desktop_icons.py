"""Mutation checks for original artwork and notices, executed only by Actions."""
import importlib.util
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

OFFICE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('office_icons', OFFICE / 'desktop_icons.py')
icons = importlib.util.module_from_spec(spec)
spec.loader.exec_module(icons)


class ArtworkPolicy(unittest.TestCase):
    def setUp(self):
        self.folder = tempfile.TemporaryDirectory()
        self.root = Path(self.folder.name)
        shutil.copytree(OFFICE / 'resources', self.root / 'resources')
        (self.root / 'frontend').mkdir()
        shutil.copyfile(OFFICE / 'frontend/app-icon.png', self.root / 'frontend/app-icon.png')
        self.original_root = icons.ROOT
        icons.ROOT = self.root

    def tearDown(self):
        icons.ROOT = self.original_root
        self.folder.cleanup()

    def test_reviewed_original_is_accepted_but_changed_image_is_refused(self):
        self.assertEqual(icons.check_source(), self.root / 'resources/icon-source.png')
        with (self.root / 'resources/icon-source.png').open('ab') as image:
            image.write(b'changed')
        with self.assertRaisesRegex(ValueError, 'reviewed original'):
            icons.check_source()

    def test_shortened_notice_is_refused_even_when_name_is_correct(self):
        (self.root / 'resources/ICON-LICENSE.md').write_text('Lyco copyright', encoding='utf-8')
        with self.assertRaisesRegex(ValueError, 'complete unchanged'):
            icons.check_source()

    def test_source_pin_and_actual_favicon_cannot_drift(self):
        (self.root / 'frontend/app-icon.png').write_bytes(b'old default icon')
        with self.assertRaisesRegex(ValueError, 'favicon'):
            icons.check_source()

    def test_conversion_is_refused_outside_actions(self):
        environment = dict(os.environ)
        environment.pop('GITHUB_ACTIONS', None)
        output = subprocess.run([sys.executable, str(OFFICE / 'desktop_icons.py'), 'prepare'],
                                env=environment, capture_output=True, text=True, timeout=10)
        self.assertNotEqual(output.returncode, 0)
        self.assertIn('only run in GitHub Actions', output.stderr)


if __name__ == '__main__':
    if os.environ.get('GITHUB_ACTIONS') != 'true':
        raise SystemExit('Office artwork tests only run in GitHub Actions')
    unittest.main()
