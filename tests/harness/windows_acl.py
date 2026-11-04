"""Inspect the actual kernel pipe DACL; omit the machine's SID from artifacts."""

import csv
import ctypes
import re
import subprocess
import time
from ctypes import wintypes


def inspect_pipe(name):
    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    security = ctypes.WinDLL("advapi32", use_last_error=True)
    pointer = ctypes.c_void_p
    kernel.CreateFileW.argtypes = [wintypes.LPCWSTR, wintypes.DWORD, wintypes.DWORD,
                                  pointer, wintypes.DWORD, wintypes.DWORD, wintypes.HANDLE]
    kernel.CreateFileW.restype = wintypes.HANDLE
    kernel.CloseHandle.argtypes = [wintypes.HANDLE]
    kernel.LocalFree.argtypes = [pointer]
    security.GetSecurityInfo.argtypes = [wintypes.HANDLE, ctypes.c_int, wintypes.DWORD,
                                        pointer, pointer, pointer, pointer, ctypes.POINTER(pointer)]
    security.GetSecurityInfo.restype = wintypes.DWORD
    security.ConvertSecurityDescriptorToStringSecurityDescriptorW.argtypes = [
        pointer, wintypes.DWORD, wintypes.DWORD, ctypes.POINTER(wintypes.LPWSTR), pointer]
    deadline = time.monotonic() + 5
    while True:
        handle = kernel.CreateFileW(name, 0x20000, 0, None, 3, 0, None)
        if handle != wintypes.HANDLE(-1).value:
            break
        error = ctypes.get_last_error()
        if error != 231 or time.monotonic() > deadline:
            raise ctypes.WinError(error)
        time.sleep(0.01)
    descriptor = pointer()
    text = wintypes.LPWSTR()
    try:
        error = security.GetSecurityInfo(handle, 1, 4, None, None, None, None, ctypes.byref(descriptor))
        if error:
            raise ctypes.WinError(error)
        if not security.ConvertSecurityDescriptorToStringSecurityDescriptorW(
                descriptor, 1, 4, ctypes.byref(text), None):
            raise ctypes.WinError(ctypes.get_last_error())
        sddl = text.value
        user = subprocess.check_output(["whoami", "/user", "/fo", "csv", "/nh"], text=True)
        current_sid = next(value for row in csv.reader(user.splitlines()) for value in row
                           if value.startswith("S-1-"))
        trustees = re.findall(r";;;([^)]+)\)", sddl)
        return {"protected": sddl.startswith("D:P"), "trustees": sorted(
            "current_user" if item == current_sid else "SYSTEM" if item == "SY" else item
            for item in trustees)}
    finally:
        if text: kernel.LocalFree(ctypes.cast(text, pointer))
        if descriptor: kernel.LocalFree(descriptor)
        kernel.CloseHandle(handle)
