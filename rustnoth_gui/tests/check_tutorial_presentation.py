"""Check the compositor output while resting on the Battle Training welcome or first move.

Record the game client at 1200x820 with the pointer stationary. Input is a lossless
capture: little-endian u32 width, height, followed by (i64 timestamp in 100 ns
units, width*height packed RGB24 pixels) for each presented frame. Downsampling
the client to 300x205 before storing still distinguishes terrain from the GUI.
"""
import struct
import sys
from pathlib import Path

import numpy as np


def check(path):
    with Path(path).open("rb") as stream:
        width, height = struct.unpack("<II", stream.read(8))
    dtype = np.dtype([("ticks", "<i8"), ("rgb", "u1", (height, width, 3))])
    recording = np.memmap(path, mode="r", offset=8, dtype=dtype)
    assert len(recording) >= 30, "sample at least 30 presented frames"
    ticks = recording["ticks"]
    assert ticks[-1] - ticks[0] >= 20_000_000 and (np.diff(ticks) > 0).all(), "capture two seconds or more"
    sidebar_x = round(width * 1018 / 1200)
    frames = recording["rgb"]
    reference = frames[0].astype(np.int16)
    assert reference.std() > 10 and reference[:, sidebar_x:].std() > 10, "battlefield and sidebar must be visible"
    for index, frame in enumerate(frames[1:], 1):
        delta = np.abs(frame.astype(np.int16) - reference).max(axis=2)
        changed = (delta[:, sidebar_x:] > 24).mean()
        assert changed < 0.05, f"frame {index}: {changed:.1%} of the stationary sidebar changed"


if __name__ == "__main__":
    for capture in sys.argv[1:]:
        check(capture)
