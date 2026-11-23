"""Compare the leader on the keep in stock and Rust 1200x820 tutorial captures."""
import sys

from PIL import Image


def check(stock_path, rendered_path):
    with Image.open(stock_path) as stock, Image.open(rendered_path) as rendered:
        assert stock.size == rendered.size == (1200, 820)
        for part, bounds in [("crown", (470, 367, 487, 380)), ("student", (481, 378, 505, 406))]:
            native = stock.convert("RGB").crop(bounds)
            actual = rendered.convert("RGB").crop(bounds)
            visible = []
            for y in range(native.height):
                for x in range(native.width):
                    reference = native.getpixel((x, y))
                    red, green, blue = reference
                    gold = red > 140 and red > green + 15 and green > blue + 32
                    tabard = blue > 70 and blue > red + 24 and blue > green + 6
                    if gold or (part == "student" and tabard):
                        candidate = actual.getpixel((x, y))
                        visible.append(max(abs(a - b) for a, b in zip(reference, candidate)) < 25)
            assert len(visible) > 15, f"missing stock {part}"
            assert sum(visible) / len(visible) > .97, (part, sum(visible), len(visible))


if __name__ == "__main__":
    check(*sys.argv[1:])
