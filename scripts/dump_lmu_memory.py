#!/usr/bin/env python3
"""Diagnostic probe for LMU shared-memory layout.

This reads the mapped LMU_Data block in Wine/Proton and prints a compact window of float-like
values to help identify a brake pedal field without changing the racing session.
"""

from __future__ import annotations

import ctypes
import mmap
import os
import sys
import time

TRY_NAMES = [b"LMU_Data", b"Global\\LMU_Data"]


def open_mapping(name: bytes):
    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
    mapping = kernel32.OpenFileMappingA(0x0004, False, name)
    if not mapping:
        return None
    view = kernel32.MapViewOfFile(mapping, 0x0004, 0, 0, 0)
    if not view:
        kernel32.CloseHandle(mapping)
        return None
    return mapping, view


def find_lmu_mapping():
    import ctypes.wintypes

    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
    for name in TRY_NAMES:
        mapping = kernel32.OpenFileMappingA(0x0004, False, name)
        if mapping:
            return mapping
    return None


def read_float_candidates(buf: bytes, start: int, count: int):
    values = []
    for offset in range(start, min(len(buf), start + count * 8), 8):
        try:
            value = ctypes.c_double.from_buffer_copy(buf[offset:offset + 8]).value
        except ValueError:
            continue
        if value != 0.0 and abs(value) < 1_000_000.0 and value != float("inf"):
            values.append((offset, value))
    return values


def main():
    if os.name != "nt":
        print("This probe is intended for the Wine/Proton LMU shared-memory view.")
        return 1

    mapping = find_lmu_mapping()
    if mapping is None:
        print("LMU shared memory not found.")
        return 1

    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
    view = kernel32.MapViewOfFile(mapping, 0x0004, 0, 0, 0)
    if not view:
        print("MapViewOfFile failed.")
        return 1

    try:
        import ctypes.wintypes

        size = 256 * 1024
        # Map the whole file view; a large enough slice is enough for spot-checking candidate float fields.
        data = (ctypes.c_ubyte * size).from_address(view)
        blob = bytes(data)

        print("LMU mapping opened; scanning float-like candidates...")
        print("offset     value")
        seen = 0
        for offset in range(0, min(len(blob) - 8, 200000), 4):
            try:
                value = ctypes.c_float.from_buffer_copy(blob[offset:offset + 4]).value
            except ValueError:
                continue
            if abs(value) <= 1.5 or (0.0 <= value <= 100.0 and value % 1.0 != 0.0):
                print(f"{offset:06d}  {value: .6f}")
                seen += 1
                if seen >= 200:
                    break
        print("scan complete")
    finally:
        kernel32.UnmapViewOfFile(view)
        kernel32.CloseHandle(mapping)

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
