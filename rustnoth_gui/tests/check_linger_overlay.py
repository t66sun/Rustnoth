"""Compare a stationary hex in lossless battlefield captures before and during linger.

Use a 72-pixel hex at default zoom. Supply its top-left screen position, the
installed darken-linger.png, and optionally the global time's RGB adjustments.
An explored, fogged or shrouded unchanging hex can be checked the same way.
"""
import argparse

import numpy as np
from PIL import Image


def check(before, after, texture, x, y, shift):
    with Image.open(before) as playing, Image.open(after) as lingering, Image.open(texture) as mask:
        assert playing.size == lingering.size
        assert mask.size == (72, 72)
        w, h = mask.size
        assert 0 <= x <= playing.width - w and 0 <= y <= playing.height - h
        area = (x, y, x + w, y + h)
        base = np.asarray(playing.convert("RGB").crop(area), dtype=float)
        shown = np.asarray(lingering.convert("RGB").crop(area), dtype=float)
        overlay = np.asarray(mask.convert("RGBA"), dtype=float)
    alpha = overlay[:, :, 3:4] / 255
    inside = alpha[:, :, 0] > 0
    expected = base * (1 - alpha) + np.clip(overlay[:, :, :3] + shift, 0, 255) * alpha
    assert np.abs(expected - base)[inside].mean() > 5, "choose a hex visibly affected by linger"
    error = np.max(np.abs(shown - expected), axis=2)[inside]
    assert (error <= 2).mean() > 0.97, "the retained battlefield lost the stock linger overlay"


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("before")
    parser.add_argument("after")
    parser.add_argument("texture")
    parser.add_argument("x", type=int)
    parser.add_argument("y", type=int)
    parser.add_argument("--shift", type=int, nargs=3, default=[0, 0, 0])
    check(**vars(parser.parse_args()))
