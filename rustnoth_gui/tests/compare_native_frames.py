"""Compare complete native and Rust PNG frames using exact RGB equality."""
import argparse
from pathlib import Path

from PIL import Image, ImageChops


def difference(reference, actual):
    if reference.size != actual.size:
        raise ValueError(f"frame dimensions differ: {reference.size} and {actual.size}")
    red, green, blue = ImageChops.difference(reference.convert("RGB"), actual.convert("RGB")).split()
    return ImageChops.lighter(ImageChops.lighter(red, green), blue)


def compare(reference, actual, diff_path=None):
    with Image.open(reference) as native_image, Image.open(actual) as rust_image:
        for img in (native_image, rust_image):
            if img.format != "PNG":
                raise ValueError(f"{img.filename} is not a lossless PNG capture")
        delta = difference(native_image, rust_image)
    if diff_path:
        delta.point(lambda v: min(255, 50 + v * 8) if v else 0).save(diff_path)
    return delta.histogram()[0], delta.width * delta.height


def accepted(equal, total):
    return equal * 100 > 97 * total


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pair", action="append", nargs=3, metavar=("SCENE", "ORIGINAL", "RUST"), required=True)
    parser.add_argument("--diff-dir", type=Path, help="save amplified differences for visual inspection")
    args = parser.parse_args()
    if args.diff_dir:
        args.diff_dir.mkdir(parents=True, exist_ok=True)
    passed = True
    for index, (scene, reference, actual) in enumerate(args.pair, start=1):
        output = args.diff_dir / f"diff-{index}.png" if args.diff_dir else None
        try:
            equal, total = compare(reference, actual, output)
        except (ValueError, OSError) as error:
            print(f"{scene}: {error}")
            passed = False
            continue
        passes = accepted(equal, total)
        print(f"{scene}: {equal}/{total} identical RGB pixels ({equal / total:.4%}) {'PASS' if passes else 'FAIL'}")
        passed &= passes
    return 0 if passed else 1


if __name__ == "__main__":
    raise SystemExit(main())
