"""Actual packaged Tauri -> original lbin -> visible UI round trip, only in CI."""
import base64
import argparse
import hashlib
import http.client
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import time
import urllib.request
import urllib.error

ROOT = Path(__file__).resolve().parents[1]
WORKSPACE = ROOT.parent
EVIDENCE = ROOT / 'tests/evidence'
PORT = 4447


def request(path, method='GET', value=None, timeout=120):
    body = None if value is None else json.dumps(value).encode('utf-8')
    req = urllib.request.Request(f'http://127.0.0.1:{PORT}{path}', data=body,
                                 method=method, headers={'Content-Type': 'application/json'})
    # Native driver traffic stays on loopback even if CI defines an HTTP proxy.
    try:
        with urllib.request.build_opener(urllib.request.ProxyHandler({})).open(req, timeout=timeout) as response:
            payload = json.load(response)
    except urllib.error.HTTPError as error:
        raise RuntimeError('Native driver HTTP error: ' + error.read(16384).decode('utf-8', errors='replace')) from error
    result = payload.get('value')
    if isinstance(result, dict) and 'error' in result:
        raise RuntimeError(json.dumps(result, ensure_ascii=False))
    return result


def wait(check, seconds=60):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        value = check()
        if value:
            return value
        time.sleep(.1)
    raise TimeoutError('Actual native workbench did not reach the expected state')


def run(application_override=None, engine_override=None, label='packaged'):
    global EVIDENCE
    EVIDENCE = EVIDENCE / label
    EVIDENCE.mkdir(parents=True, exist_ok=True)
    suffix = '.exe' if sys.platform == 'win32' else ''
    application = Path(application_override) if application_override else WORKSPACE / ('target/release/lilyco-office' + suffix)
    engine = Path(engine_override) if engine_override else WORKSPACE / ('target/release/lbin' + suffix)
    with (EVIDENCE / 'webdriver.log').open('w', encoding='utf-8') as log:
        process = subprocess.Popen(['tauri-driver', '--port', str(PORT), '--native-port', '4448'],
                                   stdout=log, stderr=subprocess.STDOUT)
        session = None
        try:
            def listening():
                if process.poll() is not None:
                    raise RuntimeError('tauri-driver exited before opening its port')
                try:
                    # The intermediary can listen before its real native driver.
                    # Wait for both listeners and a successful native /status;
                    # never retry the mutating POST /session after disconnection.
                    for port in [PORT, 4448]:
                        with socket.create_connection(('127.0.0.1', port), timeout=1):
                            pass
                    return request('/status', timeout=2).get('ready') is True
                except (OSError, urllib.error.URLError, http.client.RemoteDisconnected):
                    return False
            wait(listening, 30)
            # Same official capabilities as tauri-apps/webdriver-example.
            opened = request('/session', 'POST', {'capabilities': {'alwaysMatch': {
                'tauri:options': {'application': str(application.resolve())},
            }}})
            session = opened['sessionId']
            prefix = '/session/' + session
            request(prefix + '/timeouts', 'POST', {'script': 90000, 'pageLoad': 120000, 'implicit': 0})

            def js(script, *args):
                return request(prefix + '/execute/sync', 'POST', {'script': script, 'args': list(args)})

            def async_js(script, *args):
                return request(prefix + '/execute/async', 'POST', {'script': script, 'args': list(args)})

            wait(lambda: js('return document.querySelector("h1")?.textContent === "lilyco office"'))
            # No browser mock: this namespace comes from the production Tauri runtime.
            assert js('return typeof window.__TAURI__?.core?.invoke === "function"')
            wait(lambda: js('return document.querySelectorAll("nav button").length === 10'))
            assert not js('return document.querySelector("[role=alert]")?.textContent')

            def drop(path):
                result = async_js('''
const done = arguments[arguments.length - 1];
window.__TAURI__.event.emitTo('main', 'tauri://drag-drop',
  { paths: [arguments[0]], position: { x: 100, y: 100 } })
  .then(() => done({ok:true}), error => done({error:String(error)}));
''', str(path.resolve()))
                assert result == {'ok': True}, result
                wait(lambda: js('return document.querySelector("[data-testid=selected-file]")?.textContent',) == path.name)

            def click(text):
                assert js('''const button = [...document.querySelectorAll('button')].find(b => b.textContent.trim() === arguments[0]);
if (!button || button.disabled) return false; button.click(); return true;''', text), text

            fixtures = WORKSPACE / 'lilyco-binfmt/tests/fixtures/office'
            native_dialog = None
            if sys.platform == 'win32':
                dialog_fixture = sorted(fixtures.glob('*.docx'))[0]
                for mode, button in [('select', '选择文件'), ('cancel', '更换文件')]:
                    operator = subprocess.Popen([
                        'pwsh', '-NoProfile', '-File', str(ROOT / 'tests/native-file-dialog.ps1'),
                        '-Application', str(application.resolve()), '-Mode', mode,
                        '-Fixture', str(dialog_fixture.resolve()),
                    ], stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, encoding='utf-8', errors='replace')
                    try:
                        click(button)
                        output, _ = operator.communicate(timeout=45)
                        (EVIDENCE / f'native-dialog-{mode}.log').write_text(output, encoding='utf-8')
                        assert operator.returncode == 0, output
                        wait(lambda: js('return document.querySelector("[data-testid=selected-file]")?.textContent') == dialog_fixture.name)
                        assert not js('return document.querySelector("[role=alert]")?.textContent')
                    finally:
                        if operator.poll() is None:
                            operator.kill()
                            operator.wait()
                native_dialog = {'actual_os_file_selected': True, 'actual_os_cancel_preserves_selection': True}
            receipts = []
            for extension, command in [('docx', 'office-doc'), ('xlsx', 'office-sheet'),
                                        ('pptx', 'office-slide'), ('pdf', 'office-pdf')]:
                candidates = sorted(fixtures.glob('*.' + extension))
                if not candidates:
                    raise ValueError('Missing real producer fixture: ' + extension)
                fixture = candidates[0]
                original = fixture.read_bytes()
                expected = subprocess.run([str(engine), command, '--path', str(fixture), '--json'],
                                          check=True, capture_output=True, timeout=60)
                parsed = json.loads(expected.stdout)
                # First Windows read consumes the actual OS chooser selection;
                # the remaining formats exercise the native event route.
                if not (sys.platform == 'win32' and extension == 'docx'):
                    drop(fixture)
                click('读取文件')
                wait(lambda: js('return !!document.querySelector("[data-testid=office-result]")'))
                assert not js('return document.querySelector("[role=alert]")?.textContent')
                click('原始 JSON')
                rendered = js('return document.querySelector("[data-testid=office-result] pre")?.textContent')
                assert json.loads(rendered) == parsed, extension + ' native UI changed the original CLI result'
                assert fixture.read_bytes() == original, 'Reader modified the source document'
                receipts.append({'format': extension, 'fixture': fixture.name,
                                 'input_sha256': hashlib.sha256(original).hexdigest(),
                                 'original_cli_equals_actual_ui': True, 'original_file_unchanged': True})

            # A failed next file must remove the previous successful result.
            missing = fixtures / 'not-a-real-office-file.docx'
            assert not missing.exists()
            drop(missing)
            click('读取文件')
            wait(lambda: js('return document.querySelector("[role=alert]")?.textContent'))
            assert missing.name in js('return document.querySelector("[role=alert]").textContent')
            assert not js('return !!document.querySelector("[data-testid=office-result]")')
            click('清除')

            # Forget the in-memory history using the actual UI control.
            js('''document.querySelector('button[aria-label="清空本次文件记录"]').click()''')
            wait(lambda: js('''return !document.querySelector('button[aria-label="清空本次文件记录"]')'''))

            layouts = []
            for width in [320, 520, 768, 1180, 1440]:
                request(prefix + '/window/rect', 'POST', {'width': width, 'height': 900})
                time.sleep(.2)
                metrics = js('return {width:innerWidth, body:document.documentElement.scrollWidth, height:innerHeight}')
                assert metrics['body'] <= metrics['width'] + 1, metrics
                screenshot = request(prefix + '/screenshot')
                (EVIDENCE / f'native-{sys.platform}-{width}.png').write_bytes(base64.b64decode(screenshot))
                layouts.append({'requested_width': width, **metrics, 'no_horizontal_overflow': True})
            # A real native driver screenshot, not a rendered mock image.
            report = {'platform': sys.platform, 'application_sha256': hashlib.sha256(application.read_bytes()).hexdigest(),
                      'actual_tauri_namespace': True, 'actual_runtime_event_to_rust_sidecar_to_ui': receipts,
                      'actual_windows_file_dialog': native_dialog,
                      'failed_read_clears_previous_result': True, 'history_can_be_cleared': True,
                      'native_layouts': layouts}
            (EVIDENCE / ('native-workbench-' + sys.platform + '.json')).write_text(
                json.dumps(report, indent=2) + '\n', encoding='utf-8')
            print(json.dumps(report))
        except Exception:
            log.flush()
            print('Actual driver diagnostics:\n' + (EVIDENCE / 'webdriver.log').read_text(
                encoding='utf-8', errors='replace')[-16384:], file=sys.stderr)
            raise
        finally:
            try:
                if session:
                    request('/session/' + session, 'DELETE')
            finally:
                process.terminate()
                try:
                    process.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()


if __name__ == '__main__':
    if os.environ.get('GITHUB_ACTIONS') != 'true':
        raise SystemExit('Native Office workbench acceptance only runs in GitHub Actions')
    parser = argparse.ArgumentParser()
    parser.add_argument('--binary')
    parser.add_argument('--engine')
    parser.add_argument('--label', choices=['packaged', 'installed-windows'], default='packaged')
    args = parser.parse_args()
    run(args.binary, args.engine, args.label)
