"""Compare decoded VP8 wood colors in paired, lossless, full-window title captures."""
import sys
from PIL import Image, ImageChops, ImageStat


def check(native_path, rust_path):
    with Image.open(native_path) as native, Image.open(rust_path) as rust:
        assert native.size == rust.size, (native.size, rust.size)
        width, height = native.size
        # The title map has a 4:3 aspect. These two bands expose only wood,
        # clear of the centered map, title controls, footer, and cursor.
        band = (width - round(height * 4 / 3)) // 2
        assert band >= 8, "use a widescreen title window with both wood margins visible"
        for x in (2, width - band + 2):
            box = x, height // 6, x + band - 4, height * 2 // 3
            a, b = (frame.crop(box).convert("RGB") for frame in (native, rust))
            assert min(ImageStat.Stat(a).stddev) > 5, "empty reference margin"
            errors = ImageStat.Stat(ImageChops.difference(a, b)).mean
            assert max(errors) < 2, (box, errors)


if __name__ == "__main__":
    check(*sys.argv[1:])
