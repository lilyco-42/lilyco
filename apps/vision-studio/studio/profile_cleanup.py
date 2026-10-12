"""Remove only newly created screenshot profiles after the owned process tree stops."""
import asyncio
from pathlib import Path
import shutil
import time


class ScreenshotProfiles:
    def __init__(self, tool, arguments):
        self.parent = None
        self.existing = set()
        if tool.replace('-', '').replace('_', '').lower() == 'htmlscreenshot' and arguments.get('out'):
            self.parent = Path(arguments['out']).resolve().parent
            self.existing = set(self.parent.glob('.lvision-profile-*'))

    async def close(self, timeout=5):
        if self.parent is None:
            return
        deadline = time.monotonic() + timeout
        for profile in set(self.parent.glob('.lvision-profile-*')) - self.existing:
            # Never follow a link or remove files outside this call's output directory.
            if profile.is_symlink() or profile.resolve().parent != self.parent:
                raise ValueError('截图临时目录越界，未删除')
            while profile.exists():
                try:
                    await asyncio.to_thread(shutil.rmtree, profile)
                except FileNotFoundError:
                    break  # Rust may already have removed it.
                except PermissionError as error:
                    if getattr(error, 'winerror', None) not in {5, 32, 33} or time.monotonic() >= deadline:
                        raise ValueError('截图临时文件仍被占用，清理未完成') from error
                    # Process termination and Windows handle release are asynchronous.
                    # Retry this owned directory only; persistent locks remain failures.
                    await asyncio.sleep(.05)
