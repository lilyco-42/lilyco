"""Actual Windows token, environment and cancellation contracts; Actions only."""
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest

from windows_native_process import launch_non_elevated


@unittest.skipUnless(sys.platform == 'win32', 'Uses actual Windows security tokens')
class OrdinaryNativeProcessTests(unittest.TestCase):
    def test_ordinary_child_receives_unicode_environment_and_arguments(self):
        with tempfile.TemporaryDirectory(dir=os.environ['RUNNER_TEMP']) as temporary:
            output_path = Path(temporary) / 'child.log'
            environment = dict(os.environ, OFFICE_CI_PROBE='文件路径 with spaces')
            program = ('import json,os,sys,time; '
                       'print(json.dumps([os.environ["OFFICE_CI_PROBE"],sys.argv[1]]),flush=True); '
                       'time.sleep(1)')
            with output_path.open('w', encoding='utf-8') as output:
                child, receipt = launch_non_elevated(
                    [sys.executable, '-c', program, 'a quoted "office" argument'], environment, output)
                try:
                    self.assertEqual(child.wait(timeout=20), 0)
                    self.assertFalse(receipt['application']['is_elevated'])
                    self.assertLessEqual(receipt['application']['integrity_rid'], 0x2000)
                    self.assertFalse(receipt['registry_or_product_configuration_changed'])
                finally:
                    if child.poll() is None:
                        child.kill()
                        child.wait(timeout=10)
                    if hasattr(child, 'close'):
                        child.close()
            self.assertEqual(json.loads(output_path.read_text(encoding='utf-8')),
                             ['文件路径 with spaces', 'a quoted "office" argument'])

    def test_cancellation_terminates_only_the_created_process(self):
        with tempfile.TemporaryDirectory(dir=os.environ['RUNNER_TEMP']) as temporary:
            with (Path(temporary) / 'cancel.log').open('w') as output:
                child, receipt = launch_non_elevated(
                    [sys.executable, '-c', 'import time; time.sleep(60)'], dict(os.environ), output)
                try:
                    self.assertFalse(receipt['application']['is_elevated'])
                    self.assertIsNone(child.poll())
                    child.terminate()
                    self.assertNotEqual(child.wait(timeout=10), 0)
                    self.assertIsNotNone(child.poll())
                finally:
                    if child.poll() is None:
                        child.kill()
                        child.wait(timeout=10)
                    if hasattr(child, 'close'):
                        child.close()


if __name__ == '__main__':
    if os.environ.get('GITHUB_ACTIONS') != 'true':
        raise SystemExit('Native process contracts only run in GitHub Actions')
    unittest.main()
