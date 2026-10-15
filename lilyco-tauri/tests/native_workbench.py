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
import tempfile
import time
import urllib.request
import urllib.error
from urllib.parse import urlparse
from windows_native_process import launch_non_elevated

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


def verify_debug_endpoint(value, port):
    """Only attach to the CI-owned loopback WebView, never a remote browser."""
    endpoint = urlparse(value.get('webSocketDebuggerUrl', ''))
    if (endpoint.scheme != 'ws' or endpoint.hostname not in {'127.0.0.1', 'localhost'}
            or endpoint.port != port or endpoint.username or endpoint.password
            or not endpoint.path.startswith('/devtools/browser/')
            or not isinstance(value.get('Browser'), str) or not value['Browser']):
        raise ValueError('Unexpected native WebView debugging endpoint')
    return value


def compare_cli_content(expected, actual, extension):
    """Compare independent real invocations, retaining their measured timings."""
    if extension != 'pdf':
        assert actual == expected, extension + ' native UI changed the original CLI result'
        return None
    # office_pdf.rs measures each invocation independently. Only this documented
    # top-level duration may differ; every document field must still match.
    assert isinstance(expected, dict) and isinstance(actual, dict)
    timings = {'independent_cli_elapsed_ms': expected.get('elapsed_ms'),
               'ui_invocation_elapsed_ms': actual.get('elapsed_ms')}
    assert all(type(value) is int and value >= 0 for value in timings.values()), timings
    expected_content = {key: value for key, value in expected.items() if key != 'elapsed_ms'}
    actual_content = {key: value for key, value in actual.items() if key != 'elapsed_ms'}
    assert actual_content == expected_content, 'pdf native UI changed document content'
    return timings


def run(application_override=None, engine_override=None, label='packaged'):
    global EVIDENCE
    EVIDENCE = EVIDENCE / label
    EVIDENCE.mkdir(parents=True, exist_ok=True)
    suffix = '.exe' if sys.platform == 'win32' else ''
    application = Path(application_override) if application_override else WORKSPACE / ('target/release/lilyco-office' + suffix)
    engine = Path(engine_override) if engine_override else WORKSPACE / ('target/release/lbin' + suffix)
    with (EVIDENCE / 'webdriver.log').open('w', encoding='utf-8') as log:
        # Windows uses Microsoft's documented attach mode with the same frozen
        # application and current Evergreen runtime. No old runtime, alternate
        # runner, test-only application feature, mock IPC, or app config change.
        driver_command = (['msedgedriver.exe', '--port=' + str(PORT), '--verbose'] if sys.platform == 'win32'
                          else ['tauri-driver', '--port', str(PORT), '--native-port', '4448'])
        process = subprocess.Popen(driver_command,
                                   stdout=log, stderr=subprocess.STDOUT)
        session = None
        application_process = None
        application_log = None
        native_launch = None
        try:
            def listening():
                if process.poll() is not None:
                    raise RuntimeError('tauri-driver exited before opening its port')
                try:
                    # The intermediary can listen before its real native driver.
                    # Wait for both listeners and a successful native /status;
                    # never retry the mutating POST /session after disconnection.
                    for port in ([PORT] if sys.platform == 'win32' else [PORT, 4448]):
                        with socket.create_connection(('127.0.0.1', port), timeout=1):
                            pass
                    return request('/status', timeout=2).get('ready') is True
                except (OSError, urllib.error.URLError, http.client.RemoteDisconnected):
                    return False
            wait(listening, 30)
            # Same official capabilities as tauri-apps/webdriver-example.
            capabilities = {'tauri:options': {'application': str(application.resolve())}}
            if sys.platform == 'win32':
                runner_temp = Path(os.environ['RUNNER_TEMP']).resolve(strict=True)
                profile = Path(tempfile.mkdtemp(prefix='office-webview-' + label + '-', dir=runner_temp)).resolve()
                if not profile.is_relative_to(runner_temp):
                    raise ValueError('Native test profile escaped the runner temp directory')
                with socket.socket() as reservation:
                    reservation.bind(('127.0.0.1', 0))
                    debug_port = reservation.getsockname()[1]
                environment = os.environ.copy()
                environment['WEBVIEW2_USER_DATA_FOLDER'] = str(profile)
                environment['WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS'] = (
                    f'--remote-debugging-port={debug_port} --remote-debugging-address=127.0.0.1')
                application_log = (EVIDENCE / 'application.log').open('w', encoding='utf-8')
                application_process, launch_security = launch_non_elevated(
                    [str(application.resolve())], environment, application_log)
                (EVIDENCE / 'native-launch-security.json').write_text(
                    json.dumps(launch_security, indent=2) + '\n', encoding='utf-8')

                def webview_ready():
                    if application_process.poll() is not None:
                        raise RuntimeError('Actual Office process exited before its WebView became ready')
                    try:
                        url = f'http://127.0.0.1:{debug_port}/json/version'
                        with urllib.request.build_opener(urllib.request.ProxyHandler({})).open(url, timeout=2) as response:
                            return verify_debug_endpoint(json.load(response), debug_port)
                    except (OSError, urllib.error.URLError, http.client.RemoteDisconnected):
                        return False
                try:
                    version = wait(webview_ready, 60)
                finally:
                    diagnostics = subprocess.run([
                        'pwsh', '-NoProfile', '-File', str(ROOT / 'tests/native-process-diagnostics.ps1'),
                        '-ApplicationPid', str(application_process.pid),
                    ], capture_output=True, text=True, encoding='utf-8', errors='replace', timeout=20)
                    (EVIDENCE / 'owned-native-processes.json').write_text(diagnostics.stdout, encoding='utf-8')
                    (EVIDENCE / 'owned-native-processes.stderr.log').write_text(diagnostics.stderr, encoding='utf-8')
                native_launch = {'mode': 'microsoft-webview2-attach', 'process_id': application_process.pid,
                                 'isolated_profile': str(profile), 'loopback_port': debug_port,
                                 'actual_runtime': version, 'no_product_config_override': True,
                                 'actual_security_context': launch_security}
                capabilities = {'browserName': 'webview2', 'ms:edgeChromium': True,
                                'ms:edgeOptions': {'debuggerAddress': f'127.0.0.1:{debug_port}'}}
            opened = request('/session', 'POST', {'capabilities': {'alwaysMatch': capabilities}})
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
            fonts = async_js('''
const done = arguments[arguments.length - 1];
document.fonts.load('400 16px "Noto Sans SC Variable"', '文档表格幻灯片读取文件')
  .then(faces => done(faces.map(face => ({family:face.family, status:face.status}))),
        error => done({error:String(error)}));
''')
            assert isinstance(fonts, list) and fonts and all(face['status'] == 'loaded' for face in fonts), fonts
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
                (EVIDENCE / f'native-readable-{extension}.png').write_bytes(base64.b64decode(request(prefix + '/screenshot')))
                click('原始 JSON')
                rendered = js('return document.querySelector("[data-testid=office-result] pre")?.textContent')
                # Preserve the two real values on mismatch. Do not round,
                # sort arrays, omit fields, or substitute mocked results.
                (EVIDENCE / f'original-cli-{extension}.json').write_bytes(expected.stdout)
                (EVIDENCE / f'actual-ui-{extension}.json').write_text(rendered, encoding='utf-8')
                try:
                    timings = compare_cli_content(parsed, json.loads(rendered), extension)
                except AssertionError:
                    repeat = subprocess.run([str(engine), command, '--path', str(fixture), '--json'],
                                            check=True, capture_output=True, timeout=60)
                    (EVIDENCE / f'repeated-cli-{extension}.json').write_bytes(repeat.stdout)
                    print(json.dumps({'mismatched_format': extension,
                                      'cli_repeat_equal': json.loads(repeat.stdout) == parsed,
                                      'expected': parsed, 'actual': json.loads(rendered)}, ensure_ascii=False), file=sys.stderr)
                    raise
                (EVIDENCE / f'native-result-{extension}.png').write_bytes(base64.b64decode(request(prefix + '/screenshot')))
                assert fixture.read_bytes() == original, 'Reader modified the source document'
                receipts.append({'format': extension, 'fixture': fixture.name,
                                 'input_sha256': hashlib.sha256(original).hexdigest(),
                                 'original_cli_document_content_equals_actual_ui': True,
                                 'per_invocation_timings': timings, 'original_file_unchanged': True})

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
                      'actual_windows_launch': native_launch,
                      'self_hosted_chinese_font_loaded': fonts,
                      'failed_read_clears_previous_result': True, 'history_can_be_cleared': True,
                      'native_layouts': layouts}
            (EVIDENCE / ('native-workbench-' + sys.platform + '.json')).write_text(
                json.dumps(report, indent=2) + '\n', encoding='utf-8')
            print(json.dumps(report))
        except Exception:
            log.flush()
            print('Actual driver diagnostics:\n' + (EVIDENCE / 'webdriver.log').read_text(
                encoding='utf-8', errors='replace')[-16384:], file=sys.stderr)
            if application_log:
                application_log.flush()
                print('Actual application diagnostics:\n' + (EVIDENCE / 'application.log').read_text(
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
                if application_process and application_process.poll() is None:
                    application_process.terminate()
                    try:
                        application_process.wait(timeout=10)
                    except subprocess.TimeoutExpired:
                        application_process.kill()
                        application_process.wait()
                if application_log:
                    application_log.close()
                if application_process and hasattr(application_process, 'close'):
                    application_process.close()


if __name__ == '__main__':
    if os.environ.get('GITHUB_ACTIONS') != 'true':
        raise SystemExit('Native Office workbench acceptance only runs in GitHub Actions')
    parser = argparse.ArgumentParser()
    parser.add_argument('--binary')
    parser.add_argument('--engine')
    parser.add_argument('--label', choices=['packaged', 'installed-windows'], default='packaged')
    args = parser.parse_args()
    run(args.binary, args.engine, args.label)
