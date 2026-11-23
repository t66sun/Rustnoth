"""Compare a running Windows game's physical client size with its F12 PNG."""
import argparse
import ctypes
import struct
from ctypes import wintypes
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("hwnd", type=lambda s: int(s, 0), help="game window handle")
    parser.add_argument("screenshot", type=Path, help="F12 capture at the current window size")
    args = parser.parse_args()

    user32 = ctypes.WinDLL("user32", use_last_error=True)
    context = wintypes.HANDLE
    user32.SetThreadDpiAwarenessContext.argtypes = [context]
    user32.SetThreadDpiAwarenessContext.restype = context
    user32.GetWindowDpiAwarenessContext.argtypes = [wintypes.HWND]
    user32.GetWindowDpiAwarenessContext.restype = context
    user32.GetAwarenessFromDpiAwarenessContext.argtypes = [context]
    user32.GetClientRect.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.RECT)]
    if not user32.SetThreadDpiAwarenessContext(-4):  # per-monitor aware v2
        raise ctypes.WinError(ctypes.get_last_error())
    rect = wintypes.RECT()
    if not user32.GetClientRect(args.hwnd, ctypes.byref(rect)):
        raise ctypes.WinError(ctypes.get_last_error())
    physical = (rect.right - rect.left, rect.bottom - rect.top)
    with args.screenshot.open("rb") as stream:
        header = stream.read(24)
    assert header[:8] == b"\x89PNG\r\n\x1a\n" and header[12:16] == b"IHDR"
    framebuffer = struct.unpack(">II", header[16:24])
    assert framebuffer == physical, (framebuffer, physical)
    awareness = user32.GetAwarenessFromDpiAwarenessContext(user32.GetWindowDpiAwarenessContext(args.hwnd))
    assert awareness == 2, f"expected per-monitor DPI awareness, got {awareness}"
    print(f"Physical framebuffer and client area match at {physical[0]}x{physical[1]}")


if __name__ == "__main__":
    main()
