"""Measure middle-drag input to the first translated terrain pixels in a foreground Rustnoth window."""
import argparse
import ctypes as c
from ctypes import wintypes as w
import csv
import json
import math
from pathlib import Path
import time

import numpy as np


class BitmapInfo(c.Structure):
    _fields_ = [("size", w.DWORD), ("width", w.LONG), ("height", w.LONG), ("planes", w.WORD),
                ("bits", w.WORD), ("compression", w.DWORD), ("image_size", w.DWORD),
                ("xppm", w.LONG), ("yppm", w.LONG), ("colors", w.DWORD), ("important", w.DWORD)]


class MouseInput(c.Structure):
    _fields_ = [("x", w.LONG), ("y", w.LONG), ("data", w.DWORD), ("flags", w.DWORD),
                ("time", w.DWORD), ("extra", c.c_size_t)]


class Input(c.Structure):
    _fields_ = [("kind", w.DWORD), ("mouse", MouseInput)]


def translation(before_fft, after):
    cross = np.fft.rfft2(after) * before_fft.conjugate()
    corr = np.fft.irfft2(cross / np.maximum(1.0, np.abs(cross)), s=after.shape, axes=(0, 1))
    y, x = np.unravel_index(np.argmax(corr), corr.shape)
    h, width = after.shape
    return int((x+width//2) % width-width//2), int((y+h//2) % h-h//2), float(corr[y, x])


def trace_pacing(trace, pid, width, height, hz):
    with trace.open(encoding="utf-8") as stream:
        rows = list(csv.DictReader(stream, delimiter="\t"))
    def field(key):
        return {int(row["value"]) for row in rows if row["event"] == key}
    if (field("process_id"), field("window_width"), field("window_height"), field("display_hz")) != (
            {pid}, {width}, {height}, {hz}):
        raise ValueError("trace must match this running process, client size and display")
    modes = [name for name in ("default_pacing", "uncapped") if field(name)]
    if len(modes) != 1:
        raise ValueError("trace has no unique pacing setting")
    return modes[0]


def measure(pid, count, trace):
    user, gdi = c.windll.user32, c.windll.gdi32
    user.SetProcessDpiAwarenessContext(c.c_void_p(-4))
    user.GetForegroundWindow.restype = w.HWND
    hwnd = user.GetForegroundWindow()
    owner = w.DWORD()
    user.GetWindowThreadProcessId(w.HWND(hwnd), c.byref(owner))
    name = c.create_unicode_buffer(128)
    user.GetClassNameW(w.HWND(hwnd), name, len(name))
    title = c.create_unicode_buffer(256)
    user.GetWindowTextW(w.HWND(hwnd), title, len(title))
    if owner.value != pid or name.value != "MINIQUADAPP" or not title.value.startswith("Rustnoth"):
        raise ValueError("bring the requested Rustnoth game board to the foreground")
    rect, origin, cursor = w.RECT(), w.POINT(), w.POINT()
    user.GetClientRect(w.HWND(hwnd), c.byref(rect))
    user.ClientToScreen(w.HWND(hwnd), c.byref(origin))
    user.GetCursorPos(c.byref(cursor))
    width, height = rect.right, rect.bottom
    sx, sy = user.GetSystemMetrics(0), user.GetSystemMetrics(1)
    if origin.x < 0 or origin.y < 0 or origin.x + width > sx or origin.y + height > sy:
        raise ValueError("the measured client must fit on the primary display")
    x, y = origin.x + width*3//5, origin.y + height//2
    patch_x, patch_y = origin.x + width//3, origin.y + height*2//5
    size = 96
    user.GetDC.restype = w.HDC
    user.ReleaseDC.argtypes = [w.HWND, w.HDC]
    gdi.CreateCompatibleDC.argtypes, gdi.CreateCompatibleDC.restype = [w.HDC], w.HDC
    gdi.CreateDIBSection.argtypes = [w.HDC, c.POINTER(BitmapInfo), w.UINT, c.POINTER(c.c_void_p), w.HANDLE, w.DWORD]
    gdi.CreateDIBSection.restype = w.HANDLE
    gdi.SelectObject.argtypes, gdi.SelectObject.restype = [w.HDC, w.HANDLE], w.HANDLE
    gdi.BitBlt.argtypes = [w.HDC, c.c_int, c.c_int, c.c_int, c.c_int, w.HDC, c.c_int, c.c_int, w.DWORD]
    gdi.GetDeviceCaps.argtypes = [w.HDC, c.c_int]
    gdi.DeleteObject.argtypes, gdi.DeleteDC.argtypes = [w.HANDLE], [w.HDC]
    user.SendInput.argtypes = [w.UINT, c.POINTER(Input), c.c_int]
    screen = user.GetDC(None)
    dc = gdi.CreateCompatibleDC(screen)
    pixels = c.c_void_p()
    info = BitmapInfo(c.sizeof(BitmapInfo), size, -size, 1, 32)
    bitmap = gdi.CreateDIBSection(screen, c.byref(info), 0, c.byref(pixels), None, 0)
    if not bitmap:
        raise c.WinError()
    prior = gdi.SelectObject(dc, bitmap)
    plane = np.ctypeslib.as_array((c.c_uint8*(4*size*size)).from_address(pixels.value)).reshape(size, size, 4)

    def grab():
        if not gdi.BitBlt(dc, 0, 0, size, size, screen, patch_x, patch_y, 0x00CC0020):
            raise c.WinError()
        gdi.GdiFlush()
        return time.perf_counter_ns(), plane[:, :, 1].copy()

    def mouse(flags, dx=0, dy=0):
        event = Input(0, MouseInput(dx, dy, 0, flags, 0, 0))
        if user.SendInput(1, c.byref(event), c.sizeof(event)) != 1:
            raise c.WinError()

    hz = gdi.GetDeviceCaps(screen, 116)
    rows = []
    try:
        pacing = trace_pacing(trace, pid, width, height, hz)
        for i in range(count):
            if user.GetForegroundWindow() != hwnd:
                raise ValueError("Rustnoth lost focus during sampling")
            user.SetCursorPos(x, y)
            time.sleep(.055 + .0017 * (i % 7))
            direction = 1 if i % 2 == 0 else -1
            mouse(0x20)  # Middle button down, independent of the timed movement.
            try:
                time.sleep(.05)
                _, before = grab()
                if before.std() < 6:
                    raise ValueError("the terrain sample has insufficient texture")
                reference = np.fft.rfft2(before)
                ax = math.floor((x + .5) * 65536 / sx)
                ay = math.floor((y + 24*direction + .5) * 65536 / sy)
                start = time.perf_counter_ns()
                mouse(0xA001, ax, ay)  # Absolute, non-coalesced WM_MOUSEMOVE.
                last = start
                row = {"sample": i+1, "dy_input": 24*direction, "latency_ms": None}
                while time.perf_counter_ns()-start < 450_000_000:
                    captured, after = grab()
                    dx, dy, confidence = translation(reference, after)
                    if abs(dx) <= 2 and 2 <= dy*direction <= 27 and confidence >= .16:
                        row.update(latency_ms=(captured-start)/1e6, poll_gap_ms=(captured-last)/1e6,
                                   dx=dx, dy=dy, correlation=round(confidence, 3))
                        break
                    last = captured
                rows.append(row)
            finally:
                mouse(0x40)
            time.sleep(.065)
    finally:
        user.SetCursorPos(cursor.x, cursor.y)
        gdi.SelectObject(dc, prior)
        gdi.DeleteObject(bitmap)
        gdi.DeleteDC(dc)
        user.ReleaseDC(None, screen)
    hits = sorted(r["latency_ms"] for r in rows if r["latency_ms"] is not None)
    return {"client_pixels": [width, height], "display_hz": hz, "pacing": pacing, "trace": str(trace),
            "samples": count, "detected": len(hits),
            "p95_ms": hits[math.ceil(.95*count)-1] if len(hits) == count else None, "observations": rows}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("pid", type=int)
    parser.add_argument("output", type=Path)
    parser.add_argument("--trace", required=True, type=Path, help="RUSTNOTH_FRAME_TRACE from the running game")
    parser.add_argument("--samples", type=int, default=30)
    args = parser.parse_args()
    if args.samples < 1 or args.samples > 200:
        parser.error("use 1 to 200 samples")
    result = measure(args.pid, args.samples, args.trace)
    with args.output.open("x", encoding="utf-8") as out:
        json.dump(result, out, indent=2)
    print({k:v for k,v in result.items() if k != "observations"})
    raise SystemExit(result["detected"] != result["samples"])
