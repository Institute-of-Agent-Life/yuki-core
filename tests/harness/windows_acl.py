"""Inspect the actual kernel pipe DACL; omit the machine's SID from artifacts."""

import csv
import ctypes
import struct
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
    security.GetSecurityDescriptorControl.argtypes = [pointer, ctypes.POINTER(wintypes.WORD), ctypes.POINTER(wintypes.DWORD)]
    security.GetAce.argtypes = [pointer, wintypes.DWORD, ctypes.POINTER(pointer)]
    security.ConvertSidToStringSidW.argtypes = [pointer, ctypes.POINTER(wintypes.LPWSTR)]
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
    dacl = pointer()
    try:
        error = security.GetSecurityInfo(handle, 1, 4, None, None, ctypes.byref(dacl), None, ctypes.byref(descriptor))
        if error:
            raise ctypes.WinError(error)
        if not dacl:
            raise AssertionError("operator pipe has no DACL")
        control, revision = wintypes.WORD(), wintypes.DWORD()
        if not security.GetSecurityDescriptorControl(descriptor, ctypes.byref(control), ctypes.byref(revision)):
            raise ctypes.WinError(ctypes.get_last_error())
        user = subprocess.check_output(["whoami", "/user", "/fo", "csv", "/nh"], text=True)
        current_sid = next(value for row in csv.reader(user.splitlines()) for value in row
                           if value.startswith("S-1-"))
        # Read the binary SIDs. SDDL can abbreviate RID 500 as LA on hosted runners.
        _, _, _, count, _ = struct.unpack("<BBHHH", ctypes.string_at(dacl, 8))
        trustees = []
        for index in range(count):
            ace = pointer()
            if not security.GetAce(dacl, index, ctypes.byref(ace)):
                raise ctypes.WinError(ctypes.get_last_error())
            kind, _, size, _ = struct.unpack("<BBHI", ctypes.string_at(ace, 8))
            if kind != 0 or size < 16:
                raise AssertionError("unexpected operator ACE type or size")
            text = wintypes.LPWSTR()
            if not security.ConvertSidToStringSidW(ace.value + 8, ctypes.byref(text)):
                raise ctypes.WinError(ctypes.get_last_error())
            try:
                sid = text.value
                trustees.append("current_user" if sid == current_sid else
                                "SYSTEM" if sid == "S-1-5-18" else "unexpected_principal")
            finally:
                kernel.LocalFree(ctypes.cast(text, pointer))
        return {"protected": bool(control.value & 0x1000), "trustees": sorted(trustees)}
    finally:
        if descriptor:
            kernel.LocalFree(descriptor)
        kernel.CloseHandle(handle)
