"""Compare lossless client screenshots immediately before and while choosing a weapon at 1200x820."""
import sys

from PIL import Image, ImageChops


def check(before, chooser):
    with Image.open(before) as a, Image.open(chooser) as b:
        assert a.size == b.size
        width, height = a.size
        delta = ImageChops.difference(a.convert("RGB"), b.convert("RGB"))
        changed = ImageChops.lighter(ImageChops.lighter(*delta.split()[:2]), delta.getchannel("B"))
        menus = changed.crop((0, 0, round(width * 190 / 1200), round(height * 27 / 820)))
        assert sum(menus.histogram()[25:]) < menus.width * menus.height * 0.03, "map menu buttons disappeared"
        dialog = changed.crop((width * 3 // 10, height * 4 // 10, width * 7 // 10, height * 6 // 10))
        assert sum(dialog.histogram()[25:]) > dialog.width * dialog.height * 0.3, "capture an open attack dialog"


if __name__ == "__main__":
    check(*sys.argv[1:])
