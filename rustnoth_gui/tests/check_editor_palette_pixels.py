"""Check an opening 1200x820 editor capture against the installed water icons."""
import sys
from pathlib import Path

from PIL import Image


def check(capture, installation):
    terrain = Path(installation) / "data/core/images/terrain/water"
    icons = ("ocean-grey", "ocean", "ocean-tropical", "coast-grey")
    with Image.open(capture) as opened:
        frame = opened.convert("RGB")
    assert frame.size == (1200, 820)
    for column, name in enumerate(icons):
        with Image.open(terrain / f"{name}-tile.png") as source:
            # At 72 -> 36, the centers of the output pixels select (1, 1), (3, 1), ...
            icon = source.convert("RGBA").resize((36, 36), Image.Resampling.NEAREST)
        for y in range(36):
            for x in range(36):
                *rgb, alpha = icon.getpixel((x, y))
                if alpha == 255:
                    pos = (1035 + column * 39 + x, 508 + y)
                    assert frame.getpixel(pos) == tuple(rgb), (name, pos)


if __name__ == "__main__":
    check(*sys.argv[1:])
