"""Private, disposable local workspaces; no arbitrary filesystem API."""
import re
import shutil
import tempfile
from pathlib import Path, PurePosixPath

MAX_UPLOAD = 32 * 1024 * 1024
MAX_FILE = 16 * 1024 * 1024
MAX_OUTPUT = 128 * 1024 * 1024
MAX_FILES = 10


def child(root: Path, name: str) -> Path:
    parts = PurePosixPath(name.replace('\\', '/')).parts
    if (not parts or name.startswith(('/', '\\')) or len(name) > 240
            or any(p in {'.', '..'} or ':' in p or any(ord(c) < 32 for c in p)
                   or p.endswith((' ', '.')) or re.fullmatch(
                       r'(?i)(con|prn|aux|nul|com[1-9]|lpt[1-9])(\..*)?', p)
                   for p in parts)):
        raise ValueError('文件名不能包含越界路径或系统保留名称')
    path = root.joinpath(*parts).resolve()
    if not path.is_relative_to(root.resolve()):
        raise ValueError('文件路径越界')
    return path


class Workspaces:
    def __init__(self):
        self.directory = tempfile.TemporaryDirectory(prefix='lain42-vision-studio-')
        self.root = Path(self.directory.name)
        self.items = {}

    def new(self, identifier):
        if len(self.items) >= 10:
            raise ValueError('最多保留 10 组文件，请先删除不用的工作区')
        root = self.root / identifier
        (root / 'input').mkdir(parents=True)
        (root / 'output').mkdir()
        self.items[identifier] = root
        return root

    def get(self, identifier):
        if identifier not in self.items:
            raise ValueError('工作区已删除或程序已重启，请重新选择文件')
        return self.items[identifier]

    def delete(self, identifier):
        shutil.rmtree(self.get(identifier))
        del self.items[identifier]

    def close(self):
        self.directory.cleanup()
