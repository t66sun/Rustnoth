"""Compare the tutorial's modal welcome and first playable frame at 1200 x 820."""
import sys
from PIL import Image


def check(welcome, play):
    for path, disabled in [(welcome, True), (play, False)]:
        with Image.open(path) as image:
            assert image.size == (1200, 820)
            pixels = list(image.convert("RGB").crop((1043, 782, 1172, 804)).getdata())
        neutral = sum(max(rgb) - min(rgb) <= 3 for rgb in pixels) / len(pixels)
        if disabled:
            assert neutral > 0.97, "end turn is still displayed as available during the welcome"
        else:
            assert neutral < 0.2, "end turn did not regain its regular artwork after dismissal"


if __name__ == "__main__":
    check(*sys.argv[1:])
