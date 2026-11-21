"""Verify exposed classic theme tiles against the original installed panel pixels."""
from pathlib import Path
import sys
from PIL import Image


def check(frame_path, installation):
    root = Path(installation) / "data/core/images/themes"
    # The toolbar bitmap is 833 pixels wide. These strips span a full tile boundary.
    with Image.open(frame_path) as frame, Image.open(root / "editor/classic/toolbar.png") as toolbar:
        assert frame.size == (1200, 820)
        frame, toolbar = frame.convert("RGB"), toolbar.convert("RGB")
        for x in range(740, 931):
            for y in range(63, 67):
                assert frame.getpixel((x, y)) == toolbar.getpixel((x % toolbar.width, y - 27)), (x, y)


if __name__ == "__main__":
    check(*sys.argv[1:])
