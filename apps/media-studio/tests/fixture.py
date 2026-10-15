"""Original synthetic video, generated only by GitHub Actions."""
import os
from pathlib import Path
import subprocess


def create(path, seconds=2, size='320x180'):
    subprocess.run([
        'ffmpeg', '-v', 'error', '-y', '-f', 'lavfi', '-i', f'testsrc=size={size}:rate=12',
        '-f', 'lavfi', '-i', 'sine=frequency=440:sample_rate=48000', '-t', str(seconds),
        '-c:v', 'libx264', '-preset', 'ultrafast', '-pix_fmt', 'yuv420p',
        '-c:a', 'aac', '-shortest', str(path)], check=True, timeout=60)


if __name__ == '__main__':
    if os.environ.get('GITHUB_ACTIONS') != 'true':
        raise SystemExit('Fixtures only in GitHub Actions')
    create(Path('tests/source.mp4'))
    create(Path('tests/cancel-source.mp4'), 8, '1280x720')
