"""Compare wide-window tutorial captures taken at the same system battery percentage."""
import sys
from PIL import Image, ImageChops


def check(native_path, rust_path):
    with Image.open(native_path) as native, Image.open(rust_path) as rust:
        assert native.size == rust.size == (1200, 820)
        for field, region in [("icon", (729, 3, 745, 19)), ("percentage", (745, 3, 800, 23))]:
            reference = native.convert("RGB").crop(region)
            actual = rust.convert("RGB").crop(region)
            if field == "icon":
                # The stock blue battery body has high blue/red contrast even in its dark fill.
                red, _, blue = reference.split()
                ink = ImageChops.subtract(blue, red).point(lambda value: 255 if value > 50 else 0)
            else:
                ink = reference.convert("L").point(lambda value: 255 if value >= 125 else 0)
            ink_pixels = ink.histogram()[255]
            assert ink_pixels > 15, f"stock capture is missing the battery {field}"

            # Count differences anywhere in the field, including extra strokes on the background.
            red, green, blue = ImageChops.difference(reference, actual).split()
            maximum = ImageChops.lighter(red, ImageChops.lighter(green, blue))
            mismatches = sum(maximum.histogram()[25:])
            assert mismatches < .03 * ink_pixels, (field, mismatches, ink_pixels)


if __name__ == "__main__":
    check(*sys.argv[1:])
