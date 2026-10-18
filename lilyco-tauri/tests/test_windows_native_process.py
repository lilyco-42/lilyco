"""Actual Windows token, environment and cancellation contracts; Actions only."""
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest

from windows_native_process import launch_non_elevated, probe_native_access, prepare_native_test_access


@unittest.skipUnless(sys.platform == 'win32', 'Uses actual Windows security tokens')
class OrdinaryNativeProcessTests(unittest.TestCase):
    def test_ordinary_descendant_can_query_its_launching_process(self):
        query = '''
import ctypes, json, sys
from ctypes import wintypes
kernel = ctypes.WinDLL('kernel32', use_last_error=True)
kernel.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
kernel.OpenProcess.restype = wintypes.HANDLE
kernel.CloseHandle.argtypes = [wintypes.HANDLE]
kernel.CloseHandle.restype = wintypes.BOOL
results = []
for access in [0x1000, 0x0400]:
    handle = kernel.OpenProcess(access, False, int(sys.argv[1]))
    results.append({'access': access, 'allowed': bool(handle),
                    'winerror': 0 if handle else ctypes.get_last_error()})
    if handle:
        kernel.CloseHandle(handle)
print(json.dumps(results), flush=True)
'''
        program = ('import json,os,subprocess,sys,time\n'
                   'result=subprocess.run([sys.executable,"-c",' + repr(query)
                   + ',str(os.getpid())],capture_output=True,text=True)\n'
                   'print(json.dumps({"exit":result.returncode,"stdout":result.stdout,'
                   '"stderr":result.stderr}),flush=True)\n'
                   'time.sleep(1)\n')
        with tempfile.TemporaryDirectory(dir=os.environ['RUNNER_TEMP']) as temporary:
            output_path = Path(temporary) / 'descendant-query.log'
            with output_path.open('w', encoding='utf-8') as output:
                child, receipt = launch_non_elevated(
                    [sys.executable, '-c', program], dict(os.environ), output)
                try:
                    self.assertEqual(child.wait(timeout=20), 0)
                    self.assertFalse(receipt['application']['is_elevated'])
                finally:
                    if child.poll() is None:
                        child.kill()
                        child.wait(timeout=10)
                    if hasattr(child, 'close'):
                        child.close()
            result = json.loads(output_path.read_text(encoding='utf-8'))
            self.assertEqual(result['exit'], 0, result)
            actual = json.loads(result['stdout'])
            print(json.dumps({'actual_owned_process_query': actual, 'launch': receipt}), flush=True)
            self.assertTrue(all(row['allowed'] for row in actual), {'actual': actual, 'launch': receipt})

    def test_ordinary_child_is_on_the_existing_interactive_desktop(self):
        program = '''
import ctypes, json
from ctypes import wintypes
ui = ctypes.WinDLL('user32', use_last_error=True)
kernel = ctypes.WinDLL('kernel32', use_last_error=True)
ui.GetProcessWindowStation.argtypes = []
ui.GetProcessWindowStation.restype = wintypes.HANDLE
ui.GetThreadDesktop.argtypes = [wintypes.DWORD]
ui.GetThreadDesktop.restype = wintypes.HANDLE
kernel.GetCurrentThreadId.argtypes = []
kernel.GetCurrentThreadId.restype = wintypes.DWORD
ui.GetUserObjectInformationW.argtypes = [wintypes.HANDLE, ctypes.c_int,
    wintypes.LPVOID, wintypes.DWORD, ctypes.POINTER(wintypes.DWORD)]
ui.GetUserObjectInformationW.restype = wintypes.BOOL
def name(handle):
    if not handle:
        raise ctypes.WinError(ctypes.get_last_error())
    length = wintypes.DWORD()
    ui.GetUserObjectInformationW(handle, 2, None, 0, ctypes.byref(length))
    value = ctypes.create_unicode_buffer(length.value // ctypes.sizeof(ctypes.c_wchar) + 1)
    if not ui.GetUserObjectInformationW(handle, 2, value, ctypes.sizeof(value), ctypes.byref(length)):
        raise ctypes.WinError(ctypes.get_last_error())
    return value.value
print(json.dumps({'window_station': name(ui.GetProcessWindowStation()),
    'desktop': name(ui.GetThreadDesktop(kernel.GetCurrentThreadId()))}), flush=True)
'''
        with tempfile.TemporaryDirectory(dir=os.environ['RUNNER_TEMP']) as temporary:
            output_path = Path(temporary) / 'desktop.log'
            with output_path.open('w', encoding='utf-8') as output:
                child, receipt = launch_non_elevated(
                    [sys.executable, '-c', program], dict(os.environ), output)
                try:
                    self.assertEqual(child.wait(timeout=20), 0)
                    self.assertFalse(receipt['application']['is_elevated'])
                finally:
                    if child.poll() is None:
                        child.kill()
                        child.wait(timeout=10)
                    if hasattr(child, 'close'):
                        child.close()
            actual = json.loads(output_path.read_text(encoding='utf-8'))
            self.assertEqual(actual['window_station'].casefold(), 'winsta0', actual)
            self.assertEqual(actual['desktop'].casefold(), 'default', actual)

    def test_ordinary_token_can_access_its_ci_payload_and_webview_folder(self):
        with tempfile.TemporaryDirectory(dir=os.environ['RUNNER_TEMP']) as temporary:
            root = Path(temporary)
            payload = root / 'read-only-probe.exe'
            payload.write_bytes(b'MZ CI fixture; this file is read, not executed')
            profile, evidence = root / 'webview', root / 'evidence'
            profile.mkdir()
            evidence.mkdir()
            receipt = prepare_native_test_access(payload, profile, dict(os.environ))
            self.assertEqual([grant['access'] for grant in receipt['grants']], ['RX', 'F'])
            self.assertTrue(receipt['existing_rules_preserved'])
            result = probe_native_access(payload, profile, evidence, dict(os.environ))
            self.assertEqual(result['exit_code'], 0, result)
            self.assertTrue(result['checks'].get('application_readable'), result)
            self.assertTrue(result['checks'].get('profile_write_read_delete'), result)
            self.assertFalse(result['actual_security']['application']['is_elevated'])
            self.assertFalse(list(profile.glob('.office-native-access-*')))

    def test_access_setup_rejects_system_folders_before_changing_any_permissions(self):
        with tempfile.TemporaryDirectory(dir=os.environ['RUNNER_TEMP']) as temporary:
            root = Path(temporary)
            payload = root / 'read-only-probe.exe'
            payload.write_bytes(b'MZ CI read-only fixture')
            profile = root / 'webview'
            profile.mkdir()
            with self.assertRaisesRegex(ValueError, 'outside CI-owned'):
                prepare_native_test_access(payload, profile, dict(os.environ), [os.environ['SYSTEMROOT']])

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
