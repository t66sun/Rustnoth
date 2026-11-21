"""Check a lossless animated capture from character confirmation through the first map."""
import sys
from pathlib import Path

from PIL import Image, ImageChops, ImageSequence, ImageStat


def frames(path):
    if Path(path).suffix == ".rgb":
        from check_tutorial_presentation import read_capture

        recording = read_capture(path)
        assert len(recording) >= 30 and recording["ticks"][-1] - recording["ticks"][0] >= 20_000_000
        for rgb in recording["rgb"]:
            yield Image.fromarray(rgb)
    else:
        with Image.open(path) as recording:
            assert recording.n_frames >= 3, "capture character selection through the first battlefield"
            for frame in ImageSequence.Iterator(recording):
                yield frame.convert("RGB")


def check(path):
    first = None
    for index, frame in enumerate(frames(path)):
        if first is None:
            first = frame.copy()
        dark = sum(frame.convert("L").histogram()[:5])
        assert dark / (frame.width * frame.height) < 0.99, f"tutorial displayed a blank frame at {index}"
    # Character selection shares the introductory map with the welcome message.
    w, h = first.size
    message = (0, round(h * 0.80), round(w * 0.82), h)
    delta = ImageChops.difference(first.crop(message), frame.crop(message))
    assert max(ImageStat.Stat(delta).mean) > 15, "capture must reach the welcome message"


if __name__ == "__main__":
    check(sys.argv[1])
