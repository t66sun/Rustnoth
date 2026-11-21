"""The title's Language button stays idle when its picker opens by shortcut."""
import sys
from pathlib import Path
from PIL import Image


def check(native_path, rust_path):
    with Image.open(native_path) as native, Image.open(rust_path) as rust:
        assert native.size == rust.size == (1200, 820)
        native = native.convert("RGB")
        rust = rust.convert("RGB")
        # Top canvas accent from GUI__BUTTON_NORMAL_FRAME. All points lie
        # outside the modal and inside the horizontal accent of the button.
        for x in range(1084, 1187):
            assert native.getpixel((x, 782)) == (21, 79, 109)
            assert rust.getpixel((x, 782)) == (21, 79, 109)


if __name__ == "__main__":
    check(*map(Path, sys.argv[1:]))

