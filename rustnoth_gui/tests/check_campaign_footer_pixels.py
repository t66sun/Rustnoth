"""Check the two campaign footer button frames in a 1200×820 English capture."""
import sys
from pathlib import Path

from PIL import Image


def check(capture, installation):
    art = Path(installation) / "images/buttons/button_normal/background.png"
    with Image.open(capture) as screenshot, Image.open(art) as source:
        frame, texture = screenshot.convert("RGB"), source.convert("RGB")
        assert frame.size == (1200, 820)
        for left, top, width, height in [(811, 727, 172, 40), (993, 734, 112, 26)]:
            # The normal GUI2 frame has gold, dark and blue top edges above the bitmap.
            for y, rgb in enumerate([(162, 127, 68), (1, 10, 16), (21, 79, 109)]):
                for x in range(4, width - 4):
                    assert frame.getpixel((left + x, top + y)) == rgb, (left + x, top + y)
            # Four unlettered scanlines cover the full width of both scaled textures.
            for y in (4, 5, height - 5, height - 4):
                sy = (2 * (y - 2) + 1) * texture.height // (2 * (height - 2))
                for x in range(4, width - 4):
                    sx = (2 * (x - 2) + 1) * texture.width // (2 * (width - 2))
                    assert frame.getpixel((left + x, top + y)) == texture.getpixel((sx, sy)), (left + x, top + y)


if __name__ == "__main__":
    check(*sys.argv[1:])
