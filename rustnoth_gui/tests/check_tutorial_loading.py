"""Check a lossless animated capture from character confirmation through the first map."""
import sys

from PIL import Image, ImageChops, ImageSequence, ImageStat


def check(path):
    with Image.open(path) as recording:
        assert recording.n_frames >= 3, "capture the character choice, loading panel and first battlefield"
        first = recording.convert("RGB")
        for index, frame in enumerate(ImageSequence.Iterator(recording)):
            last = frame.convert("RGB")
            dark = sum(last.convert("L").histogram()[:5])
            assert dark / (last.width * last.height) < 0.99, f"tutorial displayed a blank frame at {index}"
        assert max(ImageStat.Stat(ImageChops.difference(first, last)).mean) > 15, "capture must enter the map"


if __name__ == "__main__":
    check(sys.argv[1])
