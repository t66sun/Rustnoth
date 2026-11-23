"""Compare the stock English 1200×820 tutorial hero chooser with its Rust F12 capture."""
import sys
from PIL import Image, ImageChops


def check(original, rendered):
    with Image.open(original) as stock, Image.open(rendered) as rust:
        assert stock.size == rust.size == (1200, 820)
        for region in [(466, 305, 733, 514), (481, 470, 593, 496), (603, 470, 715, 496)]:
            a = stock.convert("RGB").crop(region)
            b = rust.convert("RGB").crop(region)
            r, g, blue = ImageChops.difference(a, b).split()
            counts = ImageChops.lighter(ImageChops.lighter(r, g), blue).histogram()
            fraction = sum(counts[:25]) / sum(counts)
            assert fraction > .97, (region, fraction)


if __name__ == "__main__":
    check(*sys.argv[1:])
