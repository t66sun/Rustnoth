"""Check the menu and sidebar textures against the installed classic theme at 1200×820."""
import sys
from pathlib import Path
from PIL import Image, ImageChops


def check(root, screenshot):
    theme = Path(root) / "data/core/images/themes/classic"
    with Image.open(screenshot) as capture:
        assert capture.size == (1200, 820)
        frame = capture.convert("RGB")
        # Empty stretches of both panels, including the first row after each tile repeats.
        for file, origin, area in [
            ("sidebar.png", (1018, 0), (1050, 525, 1160, 750)),
            ("menubar.png", (0, 0), (916, 3, 990, 22)),
        ]:
            with Image.open(theme / file) as image:
                tile = image.convert("RGB")
            x, y, right, bottom = area
            expected = Image.new("RGB", (right - x, bottom - y))
            for yy in range(bottom - y):
                for xx in range(right - x):
                    expected.putpixel((xx, yy), tile.getpixel(
                        ((x + xx - origin[0]) % tile.width, (y + yy - origin[1]) % tile.height)))
            assert ImageChops.difference(frame.crop(area), expected).getbbox() is None, file


if __name__ == "__main__":
    check(*sys.argv[1:])
