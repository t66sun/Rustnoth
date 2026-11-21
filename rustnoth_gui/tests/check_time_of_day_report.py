"""Compare the default battlefield clock with a stock 1200×820 tutorial F12 capture."""
import sys

from PIL import Image, ImageChops


def check(original, rendered):
    with Image.open(original) as stock, Image.open(rendered) as rust:
        assert stock.size == rust.size == (1200, 820)
        for bounds in [(1027, 305, 1152, 344), (1155, 313, 1188, 335)]:
            native = stock.convert("RGB").crop(bounds)
            actual = rust.convert("RGB").crop(bounds)
            red, green, blue = ImageChops.difference(native, actual).split()
            errors = ImageChops.lighter(ImageChops.lighter(red, green), blue).histogram()
            fraction = sum(errors[:25]) / sum(errors)
            assert fraction > .97, (bounds, fraction)


if __name__ == "__main__":
    check(*sys.argv[1:])
