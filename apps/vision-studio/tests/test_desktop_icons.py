"""Icon regressions run with product APIs in Actions; no local conversion."""
import importlib.util
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from PIL import Image, ImageDraw

spec = importlib.util.spec_from_file_location('desktop_icons',
    Path(__file__).resolve().parents[2] / 'desktop-common/desktop_icons.py')
icons = importlib.util.module_from_spec(spec)
spec.loader.exec_module(icons)


class DesktopIconTests(unittest.TestCase):
    def fixture(self, root):
        folder = Path(root) / 'resources'
        folder.mkdir()
        image = Image.new('RGBA', (512, 512), (0, 0, 0, 0))
        ImageDraw.Draw(image).rectangle((64, 64, 448, 448), fill=(128, 64, 240, 255))
        source = folder / 'icon-source.png'
        image.save(source)
        digest = icons.hashlib.sha256(source.read_bytes()).hexdigest()
        (folder / 'icon-source.json').write_text(json.dumps({'sha256': digest}))
        return folder

    def test_complete_transparent_sizes_and_original_sha(self):
        with tempfile.TemporaryDirectory() as root, patch.dict(os.environ, {'GITHUB_ACTIONS': 'true'}):
            folder = self.fixture(root)
            receipt = icons.prepare_icon(root)
            self.assertTrue(receipt['transparent_source_retained'])
            self.assertEqual(list(icons.SIZES), receipt['sizes'])
            self.assertEqual(7, len(icons.ico_frames(folder / 'app.ico')))

    def test_changed_source_is_rejected(self):
        with tempfile.TemporaryDirectory() as root, patch.dict(os.environ, {'GITHUB_ACTIONS': 'true'}):
            folder = self.fixture(root)
            (folder / 'icon-source.png').write_bytes(b'changed')
            with self.assertRaises(ValueError):
                icons.prepare_icon(root)

    def test_conversion_outside_actions_is_rejected(self):
        with patch.dict(os.environ, {'GITHUB_ACTIONS': 'false'}), self.assertRaises(RuntimeError):
            icons.prepare_icon('.')

    def test_opaque_source_is_rejected(self):
        with tempfile.TemporaryDirectory() as root, patch.dict(os.environ, {'GITHUB_ACTIONS': 'true'}):
            folder = self.fixture(root)
            source = folder / 'icon-source.png'
            Image.new('RGB', (512, 512), 'purple').save(source)
            pin = {'sha256': icons.hashlib.sha256(source.read_bytes()).hexdigest()}
            (folder / 'icon-source.json').write_text(json.dumps(pin))
            with self.assertRaises(ValueError):
                icons.prepare_icon(root)
