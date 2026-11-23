"""Compare native and Rust screenshots of the same stationary Battle Training introduction."""
import sys

from PIL import Image


def layout(path):
    with Image.open(path) as im:
        pixels = im.convert("RGB")
    w, h = pixels.size
    assert (w, h) == (1200, 820), "use equal 1200x820 client captures"
    # Find the solid five-pixel rim of the stock translucent65 dialogue frame.
    rim = [(76, 42, 24), (177, 140, 80), (16, 4, 1), (114, 79, 46), (4, 3, 3)]
    top = next(y for y in range(h // 2, h - 5)
               if all(pixels.getpixel((240, y + k)) == col for k, col in enumerate(rim)))
    # The narrator portrait occupies the left column. Ink on the right identifies
    # both paragraph wrapping and the enlarged continuation prompt independently.
    groups = []
    for y in range(top + 10, h):
        xs = []
        for x in range(348, 1013):
            r, g, b = pixels.getpixel((x, y))
            title = abs(r-186) <= 3 and abs(g-172) <= 3 and abs(b-125) <= 3
            body = min(r, g, b) >= 152 and max(r, g, b) - min(r, g, b) <= 5
            if title or body:
                xs.append(x)
        if len(xs) < 3:
            continue
        if groups and groups[-1][1] == y - 1:
            groups[-1][1] = y
            groups[-1][2] = min(groups[-1][2], xs[0])
            groups[-1][3] = max(groups[-1][3], xs[-1])
        else:
            groups.append([y, y, xs[0], xs[-1]])
    return top, groups


def check(original, rust):
    expected_top, expected = layout(original)
    actual_top, actual = layout(rust)
    assert len(expected) == 5, "use the introductory narrator dialogue before choosing a hero"
    assert abs(actual_top - expected_top) <= 1, (expected_top, actual_top)
    assert len(actual) == len(expected), (expected, actual)
    for native, rendered in zip(expected, actual):
        assert all(abs(a - b) <= 2 for a, b in zip(native, rendered)), (native, rendered)


if __name__ == "__main__":
    check(*sys.argv[1:])
