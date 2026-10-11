"""Single freezing recipe used by public and licensed-product GitHub Actions."""
import os
import subprocess
import sys

if os.environ.get('GITHUB_ACTIONS') != 'true':
    raise SystemExit('Build only in GitHub Actions')
args=[sys.executable,'-m','PyInstaller','--noconfirm','--name','Lain42-Asset-Studio',
      '--add-data','frontend:frontend','--add-data','bin:bin','--collect-submodules','uvicorn']
if sys.platform=='win32':
    args += ['--collect-all','webview','--collect-all','pythonnet','--collect-all','clr_loader',
             '--exclude-module','PyQt6','--exclude-module','PySide6','--exclude-module','gi']
else:
    args += ['--exclude-module','webview']
subprocess.run(args+['launcher.py'],check=True)
