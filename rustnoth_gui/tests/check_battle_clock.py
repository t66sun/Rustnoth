"""Compare the installed clock icon and verify a visible time on a wide battery-powered desktop."""
import sys
from PIL import Image, ImageChops


def check(stock_path, rust_path):
    with Image.open(stock_path) as stock, Image.open(rust_path) as rust:
        assert stock.size == rust.size == (1200, 820)
        icon = (820, 3, 836, 19)
        original = stock.convert("RGB").crop(icon)
        rendered = rust.convert("RGB").crop(icon)
        mask = original.convert("L").point(lambda c: 255 if c > 92 else 0)
        r, g, b = ImageChops.difference(original, rendered).split()
        error = ImageChops.lighter(ImageChops.lighter(r, g), b).histogram(mask)
        assert sum(error) > 20 and sum(error[:25]) > .97 * sum(error), "stock hourglass"
        # Captures taken at different times legitimately have different digits.
        for scene in (stock, rust):
            text = scene.convert("L").crop((836, 3, 900, 23))
            assert sum(text.histogram()[170:]) > 30, "the hour and minute must follow the icon"


if __name__ == "__main__":
    check(*sys.argv[1:])
