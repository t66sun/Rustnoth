"""Compare wide-window tutorial captures taken at the same system battery percentage."""
import sys
from PIL import Image, ImageChops


def check(native_path, rust_path):
    with Image.open(native_path) as native, Image.open(rust_path) as rust:
        assert native.size == rust.size == (1200, 820)
        for region in [(729, 3, 745, 19), (745, 3, 800, 23)]:
            reference = native.convert("RGB").crop(region)
            actual = rust.convert("RGB").crop(region)
            ink = reference.convert("L").point(lambda pixel: 255 if pixel >= 125 else 0)
            assert ink.histogram()[255] > 15, "reference needs a populated battery report"
            delta = ImageChops.difference(reference, actual)
            for channel in delta.split():
                counts = channel.histogram(ink)
                assert sum(counts[:25]) > .97 * sum(counts), region


if __name__ == "__main__":
    check(*sys.argv[1:])
