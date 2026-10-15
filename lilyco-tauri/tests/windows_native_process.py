"""CI-only native launch as the same Windows user without administrator rights.

WebView2 ignores environment browser flags in an elevated host. Keep the exact
packaged application and current runtime, and test its ordinary desktop context.
No application configuration, registry policy, or browser security flag changes.
"""
import ctypes
from ctypes import wintypes
import os
import subprocess
import sys


class NativeProcess:
    def __init__(self, kernel, handle, pid, args):
        self.kernel, self.handle, self.pid, self.args = kernel, handle, pid, args

    def poll(self):
        result = self.kernel.WaitForSingleObject(self.handle, 0)
        if result == 258:
            return None
        if result != 0:
            raise ctypes.WinError(ctypes.get_last_error())
        code = wintypes.DWORD()
        if not self.kernel.GetExitCodeProcess(self.handle, ctypes.byref(code)):
            raise ctypes.WinError(ctypes.get_last_error())
        return code.value

    def wait(self, timeout=None):
        result = self.kernel.WaitForSingleObject(
            self.handle, 0xFFFFFFFF if timeout is None else max(0, int(timeout * 1000)))
        if result == 258:
            raise subprocess.TimeoutExpired(self.args, timeout)
        if result != 0:
            raise ctypes.WinError(ctypes.get_last_error())
        return self.poll()

    def terminate(self):
        if self.poll() is None and not self.kernel.TerminateProcess(self.handle, 1):
            raise ctypes.WinError(ctypes.get_last_error())

    kill = terminate

    def close(self):
        if self.handle:
            self.kernel.CloseHandle(self.handle)
            self.handle = None


def launch_non_elevated(args, environment, output):
    if sys.platform != 'win32' or os.environ.get('GITHUB_ACTIONS') != 'true':
        raise RuntimeError('Ordinary-user native launch is for Windows GitHub Actions only')
    import msvcrt
    kernel = ctypes.WinDLL('kernel32', use_last_error=True)
    security = ctypes.WinDLL('advapi32', use_last_error=True)
    pointer = wintypes.LPVOID

    class SidAttributes(ctypes.Structure):
        _fields_ = [('Sid', pointer), ('Attributes', wintypes.DWORD)]

    class StartupInfo(ctypes.Structure):
        _fields_ = [('cb', wintypes.DWORD), ('lpReserved', wintypes.LPWSTR),
                    ('lpDesktop', wintypes.LPWSTR), ('lpTitle', wintypes.LPWSTR),
                    ('dwX', wintypes.DWORD), ('dwY', wintypes.DWORD),
                    ('dwXSize', wintypes.DWORD), ('dwYSize', wintypes.DWORD),
                    ('dwXCountChars', wintypes.DWORD), ('dwYCountChars', wintypes.DWORD),
                    ('dwFillAttribute', wintypes.DWORD), ('dwFlags', wintypes.DWORD),
                    ('wShowWindow', wintypes.WORD), ('cbReserved2', wintypes.WORD),
                    ('lpReserved2', ctypes.POINTER(wintypes.BYTE)),
                    ('hStdInput', wintypes.HANDLE), ('hStdOutput', wintypes.HANDLE),
                    ('hStdError', wintypes.HANDLE)]

    class ProcessInfo(ctypes.Structure):
        _fields_ = [('hProcess', wintypes.HANDLE), ('hThread', wintypes.HANDLE),
                    ('dwProcessId', wintypes.DWORD), ('dwThreadId', wintypes.DWORD)]

    def signature(library, name, arguments, result=wintypes.BOOL):
        function = getattr(library, name)
        function.argtypes, function.restype = arguments, result
        return function

    signature(kernel, 'GetCurrentProcess', [], wintypes.HANDLE)
    signature(kernel, 'OpenProcess', [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD], wintypes.HANDLE)
    signature(kernel, 'CloseHandle', [wintypes.HANDLE])
    signature(kernel, 'LocalFree', [pointer], pointer)
    signature(kernel, 'WaitForSingleObject', [wintypes.HANDLE, wintypes.DWORD], wintypes.DWORD)
    signature(kernel, 'GetExitCodeProcess', [wintypes.HANDLE, ctypes.POINTER(wintypes.DWORD)])
    signature(kernel, 'TerminateProcess', [wintypes.HANDLE, wintypes.UINT])
    signature(security, 'OpenProcessToken', [wintypes.HANDLE, wintypes.DWORD, ctypes.POINTER(wintypes.HANDLE)])
    signature(security, 'GetTokenInformation', [wintypes.HANDLE, ctypes.c_int, pointer, wintypes.DWORD, ctypes.POINTER(wintypes.DWORD)])
    signature(security, 'CreateRestrictedToken', [wintypes.HANDLE, wintypes.DWORD, wintypes.DWORD, pointer,
              wintypes.DWORD, pointer, wintypes.DWORD, pointer, ctypes.POINTER(wintypes.HANDLE)])
    signature(security, 'ConvertStringSidToSidW', [wintypes.LPCWSTR, ctypes.POINTER(pointer)])
    signature(security, 'SetTokenInformation', [wintypes.HANDLE, ctypes.c_int, pointer, wintypes.DWORD])
    signature(security, 'GetLengthSid', [pointer], wintypes.DWORD)
    signature(security, 'GetSidSubAuthorityCount', [pointer], ctypes.POINTER(wintypes.BYTE))
    signature(security, 'GetSidSubAuthority', [pointer, wintypes.DWORD], ctypes.POINTER(wintypes.DWORD))
    signature(security, 'CreateProcessAsUserW', [wintypes.HANDLE, wintypes.LPCWSTR, wintypes.LPWSTR,
              pointer, pointer, wintypes.BOOL, wintypes.DWORD, pointer, wintypes.LPCWSTR,
              ctypes.POINTER(StartupInfo), ctypes.POINTER(ProcessInfo)])

    def check(result):
        if not result:
            raise ctypes.WinError(ctypes.get_last_error())
        return result

    def information(token, kind):
        length = wintypes.DWORD()
        security.GetTokenInformation(token, kind, None, 0, ctypes.byref(length))
        if not length.value:
            raise ctypes.WinError(ctypes.get_last_error())
        buffer = ctypes.create_string_buffer(length.value)
        check(security.GetTokenInformation(token, kind, buffer, length, ctypes.byref(length)))
        return buffer

    def snapshot(token):
        elevated = ctypes.cast(information(token, 20), ctypes.POINTER(wintypes.DWORD)).contents.value
        integrity = information(token, 25)
        sid = ctypes.cast(integrity, ctypes.POINTER(SidAttributes)).contents.Sid
        index = security.GetSidSubAuthorityCount(sid).contents.value - 1
        return {'is_elevated': bool(elevated),
                'integrity_rid': security.GetSidSubAuthority(sid, index).contents.value}

    def process_snapshot(process_id):
        process_handle = check(kernel.OpenProcess(0x1000, False, process_id))
        token = wintypes.HANDLE()
        try:
            check(security.OpenProcessToken(process_handle, 0x0008, ctypes.byref(token)))
            return snapshot(token)
        finally:
            if token:
                kernel.CloseHandle(token)
            kernel.CloseHandle(process_handle)

    parent, restricted, medium_sid = wintypes.HANDLE(), wintypes.HANDLE(), pointer()
    process = None
    try:
        # Duplicate/query/assign-primary/adjust-default rights for our own token.
        check(security.OpenProcessToken(kernel.GetCurrentProcess(), 0x008B, ctypes.byref(parent)))
        original = snapshot(parent)
        if not original['is_elevated'] and original['integrity_rid'] <= 0x2000:
            process = subprocess.Popen(args, env=environment, stdout=output, stderr=subprocess.STDOUT)
            actual = process_snapshot(process.pid)
        else:
            # DISABLE_MAX_PRIVILEGE | LUA_TOKEN; never SANDBOX_INERT.
            check(security.CreateRestrictedToken(parent, 0x0005, 0, None, 0, None, 0, None,
                                                  ctypes.byref(restricted)))
            check(security.ConvertStringSidToSidW('S-1-16-8192', ctypes.byref(medium_sid)))
            label = SidAttributes(medium_sid, 0x0020)
            check(security.SetTokenInformation(restricted, 25, ctypes.byref(label),
                  ctypes.sizeof(label) + security.GetLengthSid(medium_sid)))
            limited = snapshot(restricted)
            if limited['is_elevated'] or limited['integrity_rid'] != 0x2000:
                raise RuntimeError('Refuse to launch the native app with an elevated token')
            if any(not key or '\0' in key + value or ('=' in key and not (
                    len(key) == 3 and key[0] == '=' and key[1].isalpha() and key[2] == ':'))
                    for key, value in environment.items()):
                raise ValueError('Invalid native process environment')
            block = ctypes.create_unicode_buffer('\0'.join(
                key + '=' + value for key, value in sorted(environment.items(), key=lambda item: item[0].upper())) + '\0')
            command = ctypes.create_unicode_buffer(subprocess.list2cmdline(args))
            startup, created = StartupInfo(), ProcessInfo()
            startup.cb = ctypes.sizeof(startup)
            startup.dwFlags = 0x0100  # STARTF_USESTDHANDLES
            with open(os.devnull, 'rb') as input_stream:
                handles = [msvcrt.get_osfhandle(input_stream.fileno()), msvcrt.get_osfhandle(output.fileno())]
                previous = [os.get_handle_inheritable(handle) for handle in handles]
                try:
                    for handle in handles:
                        os.set_handle_inheritable(handle, True)
                    startup.hStdInput, startup.hStdOutput, startup.hStdError = handles[0], handles[1], handles[1]
                    check(security.CreateProcessAsUserW(restricted, args[0], command, None, None,
                          True, 0x0400, block, None, ctypes.byref(startup), ctypes.byref(created)))
                    kernel.CloseHandle(created.hThread)
                    process = NativeProcess(kernel, created.hProcess, created.dwProcessId, args)
                finally:
                    for handle, flag in zip(handles, previous):
                        os.set_handle_inheritable(handle, flag)
            actual = process_snapshot(process.pid)
        if actual['is_elevated'] or actual['integrity_rid'] > 0x2000:
            raise RuntimeError('Actual native application did not run as an ordinary user')
        return process, {'parent': original, 'application': actual,
                         'same_windows_user': True, 'registry_or_product_configuration_changed': False}
    except BaseException:
        if process:
            if process.poll() is None:
                process.kill()
                process.wait(timeout=10)
            if isinstance(process, NativeProcess):
                process.close()
        raise
    finally:
        if medium_sid:
            kernel.LocalFree(medium_sid)
        for handle in [restricted, parent]:
            if handle:
                kernel.CloseHandle(handle)
