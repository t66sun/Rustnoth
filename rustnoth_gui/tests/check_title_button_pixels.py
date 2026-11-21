"""Check the unlettered body of the Campaigns button against the installed artwork."""
import sys
from pathlib import Path
from PIL import Image


def check(frame_path, installation):
    asset = Path(installation) / "images/buttons/button_normal/background.png"
    with Image.open(frame_path) as captured, Image.open(asset) as texture:
        assert captured.size == (1200, 820)
        frame = captured.convert("RGB")
        body = texture.convert("RGB")
        # Top and bottom strips inside the bevel contain only the scaled bitmap.
        for y in [4, 5, 6, 23, 24, 25]:
            for x in range(4, 112):
                source_x = ((2 * (x - 2) + 1) * body.width) // (2 * 114)
                source_y = ((2 * (y - 2) + 1) * body.height) // (2 * 28)
                assert frame.getpixel((981 + x, 305 + y)) == body.getpixel((source_x, source_y)), (x, y)


if __name__ == "__main__":
    check(*sys.argv[1:])
