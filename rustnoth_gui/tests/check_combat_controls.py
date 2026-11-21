"""Compare the stationary controls before a strike with the frames actually presented during it.

Use capture_battle_window.cpp at 1200x820 for the pre-attack battlefield and the
attack recording. Keep the pointer off the controls. If the capture includes the
weapon chooser or the next dialogue, select the strikes with --first and --last
(zero-based, inclusive). Include an Esc skip in a second recording.
"""
import argparse

import numpy as np

from check_tutorial_presentation import read_capture


def check(reference_path, combat_path, first, last):
    reference = read_capture(reference_path)["rgb"][0].astype(np.int16)
    combat = read_capture(combat_path)
    frames = combat["rgb"][first : last + 1 if last is not None else None]
    assert len(frames) >= 3, "capture several consecutive strike frames"
    assert reference.shape == frames.shape[1:], "capture both windows at the same size"
    h, w = reference.shape[:2]

    def region(x, y, right, bottom):
        return (slice(round(y * h / 820), round(bottom * h / 820)),
                slice(round(x * w / 1200), round(right * w / 1200)))

    menus = region(0, 1, 200, 23)
    turn = region(1027, 770, 1195, 814)
    map_area = region(0, 27, 1018, 770)
    assert reference[menus].std() > 10, "reference must show the battlefield menu buttons"
    moved = False
    for index, frame in enumerate(frames, first):
        for name, area in [("map menus", menus), ("turn control", turn)]:
            diff = np.abs(frame[area] - reference[area]).max(axis=2)
            assert (diff > 24).mean() < 0.05, f"{name} changed during strike frame {index}"
        if index > first:
            delta = np.abs(frame[map_area].astype(np.int16) - frames[index - first - 1][map_area])
            moved |= np.count_nonzero(delta.max(axis=2) > 24) >= 4
    assert moved, "the recorded battlefield must contain a moving or striking unit"


if __name__ == "__main__":
    args = argparse.ArgumentParser(description=__doc__)
    args.add_argument("reference")
    args.add_argument("combat")
    args.add_argument("--first", type=int, default=0)
    args.add_argument("--last", type=int)
    parsed = args.parse_args()
    check(parsed.reference, parsed.combat, parsed.first, parsed.last)
