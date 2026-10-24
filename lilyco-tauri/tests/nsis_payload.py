"""Check Tauri's exact documented NSIS bundle marker, not arbitrary PE changes."""
import argparse
import hashlib
import json
import os
from pathlib import Path


UNPATCHED = b'__TAURI_BUNDLE_TYPE_VAR_UNK'
NSIS = b'__TAURI_BUNDLE_TYPE_VAR_NSS'


def digest(value):
    return hashlib.sha256(value).hexdigest()


def verify_nsis_application(original, installed):
    # Tauri CLI 2.12.1 bundle.rs patches this single fixed-width marker while
    # bundling, then restores the original executable after each package type.
    # Build the exact expected bytes; never mask regions in the installed file.
    if original.count(UNPATCHED) != 1:
        raise ValueError('Expected one reviewed Tauri bundle marker')
    offset = original.index(UNPATCHED)
    expected = original[:offset] + NSIS + original[offset + len(UNPATCHED):]
    if installed != expected:
        raise ValueError('Installed application differs from the exact NSIS-marked binary')
    return {'bundle_marker_offset': offset, 'bundle_marker': NSIS.decode('ascii'),
            'expected_installed_sha256': digest(expected),
            'all_other_application_bytes_identical': True}


def main():
    if os.environ.get('GITHUB_ACTIONS') != 'true':
        raise SystemExit('Actual Office installer payload acceptance only runs in Actions')
    parser = argparse.ArgumentParser()
    parser.add_argument('--original-app', required=True, type=Path)
    parser.add_argument('--installed-app', required=True, type=Path)
    parser.add_argument('--original-engine', required=True, type=Path)
    parser.add_argument('--installed-engine', required=True, type=Path)
    parser.add_argument('--report', required=True, type=Path)
    args = parser.parse_args()
    receipt = {}
    try:
        original, installed = args.original_app.read_bytes(), args.installed_app.read_bytes()
        original_engine, installed_engine = args.original_engine.read_bytes(), args.installed_engine.read_bytes()
        receipt.update(original_application_sha256=digest(original),
                       installed_application_sha256=digest(installed),
                       original_engine_sha256=digest(original_engine),
                       installed_engine_sha256=digest(installed_engine))
        receipt.update(verify_nsis_application(original, installed))
        if installed_engine != original_engine:
            raise ValueError('Installer changed original sidecar bytes')
        receipt['all_original_engine_bytes_identical'] = True
    except Exception as error:
        receipt['error'] = str(error)
        raise
    finally:
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(json.dumps(receipt, indent=2) + '\n', encoding='utf-8')
        print(json.dumps(receipt), flush=True)


if __name__ == '__main__':
    main()
