"""CI-only checks for actual original component and Lyco source payloads."""
import hashlib
import json
import os
from pathlib import Path
import sys

if os.environ.get('GITHUB_ACTIONS') != 'true':
    raise SystemExit('Asset processing only in GitHub Actions')
sys.path.insert(0, str(Path(__file__).resolve().parents[2] / 'desktop-common'))
from desktop_icons import prepare_icon

sources = json.loads(Path('ui/src/shadcn-sources.json').read_text(encoding='utf-8'))
for item in sources:
    payload = Path('ui/src/components/ui', item['file']).read_bytes()
    assert hashlib.sha256(payload).hexdigest() == item['sha256'], item['file']
icon = prepare_icon(Path.cwd())
Path('evidence').mkdir(exist_ok=True)
Path('evidence/source-assets.json').write_text(json.dumps(
    {'commit': os.environ['GITHUB_SHA'], 'components': sources, 'icon': icon}, indent=2))
