"""Compare the empty default-theme unit report in stock and Rust 1200×820 captures."""
import sys

import numpy as np
from PIL import Image


def check(stock_path, rendered_path):
    with Image.open(stock_path) as stock, Image.open(rendered_path) as rendered:
        assert stock.size == rendered.size == (1200, 820)
        for label, x, y, width in [
            ("HP", 1107, 350, 60), ("XP", 1107, 378, 65),
            ("MP", 1107, 406, 65), ("def", 1173, 406, 25),
        ]:
            box = (x, y, x + width, y + 14)
            # The high-luminance glyphs sit over the dark textured sidebar.
            def ink(image):
                return np.asarray(image.crop(box).convert("RGB")).min(axis=2) >= 140

            reference, actual = ink(stock), ink(rendered)
            assert reference.sum() > 15, (label, "missing stock glyphs")
            overlap = (reference & actual).sum() / (reference | actual).sum()
            assert overlap > .90, (label, overlap)


if __name__ == "__main__":
    check(*sys.argv[1:])
