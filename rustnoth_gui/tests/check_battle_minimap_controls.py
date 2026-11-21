"""Compare F12 screenshots of the same 1200x820 battlefield before and after toggling terrain."""
import sys
from pathlib import Path

from PIL import Image, ImageChops


def check(root, before_file, after_file):
    icons = Path(root) / "images/icons/action"
    with Image.open(before_file) as a, Image.open(after_file) as b:
        before, after = a.convert("RGB"), b.convert("RGB")
        assert before.size == after.size == (1200, 820)
        controls = ["zoomdefault", "minimap-draw-terrain", "editor-tool-unit",
                    "editor-tool-village", "minimap-unit-coding", "minimap-terrain-coding"]
        for index, name in enumerate(controls):
            x = 1018 + 14 + 26 * index + (2 if index else 0)
            for pressed, frame in [(True, before), (index != 1, after)]:
                suffix = "-pressed" if pressed else ""
                with Image.open(icons / f"{name}_25{suffix}.png") as handle:
                    icon = handle.convert("RGBA")
                box = (x, 226, x + icon.width, 226 + icon.height)
                delta = ImageChops.difference(frame.crop(box), icon.convert("RGB"))
                r, g, b = delta.split()
                distance = ImageChops.lighter(ImageChops.lighter(r, g), b)
                opaque = icon.getchannel("A").point(lambda alpha: 255 if alpha == 255 else 0)
                histogram = distance.histogram(opaque)
                assert sum(histogram) and sum(histogram[:9]) >= .97 * sum(histogram), name
        r, g, b = ImageChops.difference(before, after).crop((1031, 11, 1191, 223)).split()
        histogram = ImageChops.lighter(ImageChops.lighter(r, g), b).histogram()
        assert sum(histogram[25:]) > .05 * sum(histogram), "terrain drawing did not change"


if __name__ == "__main__":
    check(*sys.argv[1:])
