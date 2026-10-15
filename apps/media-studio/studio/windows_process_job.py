"""Own one spawned engine tree using Windows nested Job Objects (Win8+).

The engine is attached before any MCP request is sent, so its browser children
inherit the job. Never terminate processes by executable name or global PID scan.
"""
import asyncio
import ctypes as c
from ctypes import wintypes as w


class BasicLimit(c.Structure):
    _fields_ = [('process_time', c.c_int64), ('job_time', c.c_int64),
                ('flags', w.DWORD), ('min_ws', c.c_size_t), ('max_ws', c.c_size_t),
                ('active_limit', w.DWORD), ('affinity', c.c_size_t),
                ('priority', w.DWORD), ('scheduling', w.DWORD)]


class IoCounters(c.Structure):
    _fields_ = [(name, c.c_uint64) for name in
                ['read_ops', 'write_ops', 'other_ops', 'read_bytes', 'write_bytes', 'other_bytes']]


class ExtendedLimit(c.Structure):
    _fields_ = [('basic', BasicLimit), ('io', IoCounters),
                ('process_memory', c.c_size_t), ('job_memory', c.c_size_t),
                ('peak_process_memory', c.c_size_t), ('peak_job_memory', c.c_size_t)]


class Accounting(c.Structure):
    _fields_ = [(name, c.c_int64) for name in
                ['user_time', 'kernel_time', 'period_user', 'period_kernel']] + [
                    (name, w.DWORD) for name in ['page_faults', 'total', 'active', 'terminated']]


class WindowsProcessJob:
    def __init__(self, pid):
        self.kernel = c.WinDLL('kernel32', use_last_error=True)
        functions = {
            'CreateJobObjectW': ([c.c_void_p, w.LPCWSTR], w.HANDLE),
            'OpenProcess': ([w.DWORD, w.BOOL, w.DWORD], w.HANDLE),
            'CloseHandle': ([w.HANDLE], w.BOOL),
            'SetInformationJobObject': ([w.HANDLE, c.c_int, c.c_void_p, w.DWORD], w.BOOL),
            'AssignProcessToJobObject': ([w.HANDLE, w.HANDLE], w.BOOL),
            'TerminateJobObject': ([w.HANDLE, w.UINT], w.BOOL),
            'QueryInformationJobObject': ([w.HANDLE, c.c_int, c.c_void_p, w.DWORD, c.c_void_p], w.BOOL),
        }
        for name, (args, result) in functions.items():
            function = getattr(self.kernel, name)
            function.argtypes, function.restype = args, result
        self.handle = self.kernel.CreateJobObjectW(None, None)
        if not self.handle:
            raise c.WinError(c.get_last_error())
        process = None
        try:
            info = ExtendedLimit()
            info.basic.flags = 0x2000  # JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
            if not self.kernel.SetInformationJobObject(self.handle, 9, c.byref(info), c.sizeof(info)):
                raise c.WinError(c.get_last_error())
            process = self.kernel.OpenProcess(0x0100 | 0x0001, False, pid)
            if not process or not self.kernel.AssignProcessToJobObject(self.handle, process):
                raise c.WinError(c.get_last_error())
        except BaseException:
            self.kernel.CloseHandle(self.handle)
            self.handle = None
            raise
        finally:
            if process:
                self.kernel.CloseHandle(process)

    async def stop(self):
        if not self.handle:
            return
        try:
            if not self.kernel.TerminateJobObject(self.handle, 1):
                raise c.WinError(c.get_last_error())
            for _ in range(100):
                info = Accounting()
                if not self.kernel.QueryInformationJobObject(self.handle, 1, c.byref(info), c.sizeof(info), None):
                    raise c.WinError(c.get_last_error())
                if info.active == 0:
                    return
                await asyncio.sleep(.02)
            raise ValueError('浏览器子进程未能退出，请关闭应用后重试')
        finally:
            self.kernel.CloseHandle(self.handle)
            self.handle = None
