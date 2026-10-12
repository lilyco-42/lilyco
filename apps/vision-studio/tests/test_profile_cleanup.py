"""CI-only cleanup regression: transient locks, preservation and honest failure."""
import asyncio
import os
from pathlib import Path
import shutil
import tempfile
import unittest
from unittest.mock import patch

if os.environ.get('GITHUB_ACTIONS') != 'true':
    raise SystemExit('Product tests run only in GitHub Actions')

from studio.profile_cleanup import ScreenshotProfiles


class ProfileCleanupTests(unittest.TestCase):
    def test_transient_lock_retries_only_new_owned_profile(self):
        async def run():
            with tempfile.TemporaryDirectory() as directory:
                parent = Path(directory)
                prior = parent / '.lvision-profile-existing'
                prior.mkdir()
                (prior / 'keep.txt').write_text('pre-existing')
                owner = ScreenshotProfiles('html-screenshot', {'out': str(parent / 'out.png')})
                fresh = parent / '.lvision-profile-new'
                fresh.mkdir()
                (fresh / 'locked.db').write_text('temporary')
                real = shutil.rmtree
                calls = []
                def remove(path, *args, **kwargs):
                    self.assertEqual(path, fresh)
                    calls.append(path)
                    if len(calls) < 3:
                        error = PermissionError('Windows handle release pending')
                        error.winerror = 32
                        raise error
                    return real(path, *args, **kwargs)
                with patch('studio.profile_cleanup.shutil.rmtree', side_effect=remove):
                    await owner.close()
                self.assertEqual(len(calls), 3)
                self.assertFalse(fresh.exists())
                self.assertEqual((prior / 'keep.txt').read_text(), 'pre-existing')
        asyncio.run(run())

    def test_persistent_lock_is_reported(self):
        async def run():
            with tempfile.TemporaryDirectory() as directory:
                parent = Path(directory)
                owner = ScreenshotProfiles('HtmlScreenshot', {'out': str(parent / 'out.png')})
                fresh = parent / '.lvision-profile-new'
                fresh.mkdir()
                error = PermissionError('still locked')
                error.winerror = 32
                with patch('studio.profile_cleanup.shutil.rmtree', side_effect=error):
                    with self.assertRaisesRegex(ValueError, '清理未完成'):
                        await owner.close(timeout=0)
                self.assertTrue(fresh.exists())
        asyncio.run(run())

    def test_cancel_keeps_cleanup_failure_visible(self):
        from studio.jobs import Jobs
        async def run():
            started = asyncio.Event()
            async def locked(*args):
                started.set()
                try:
                    await asyncio.Event().wait()
                finally:
                    raise ValueError('清理未完成')
            with tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                (root / 'output').mkdir()
                jobs = Jobs()
                with (patch('studio.jobs.adapters.executable', return_value=Path('not-executed')),
                      patch('studio.jobs.adapters.arguments', return_value={})):
                    with patch('studio.jobs.mcp.call', side_effect=locked):
                        job = jobs.start('owned', 'ci', root, 'html-screenshot', {})
                        await asyncio.wait_for(started.wait(), timeout=2)
                        result = await jobs.cancel(job['id'])
                self.assertEqual(result['state'], 'failed')
                self.assertEqual(result['error'], '清理未完成')
                await jobs.close()
        asyncio.run(run())
