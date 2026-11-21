"""Compare captures from a stationary battlefield and its Ctrl+J objectives window."""
import sys

from PIL import Image, ImageChops, ImageStat


def check(board_path, modal_path):
    with Image.open(board_path) as source, Image.open(modal_path) as modal:
        assert source.size == modal.size
        before = source.convert("RGB")
        after = modal.convert("RGB")
        width, height = before.size
        # The centered transient window is at most 800 pixels wide. Capture at
        # desktop size with the pointer parked and without the F12 notification.
        assert width >= 1024 and height >= 768
        margin = (width - 800) // 2
        for bounds in ((0, height // 6, margin, height * 5 // 6),
                       (width - margin, height // 6, width, height * 5 // 6)):
            scene = before.crop(bounds)
            assert max(ImageStat.Stat(scene).stddev) > 10, "missing battlefield detail"
            diff = ImageChops.difference(scene, after.crop(bounds))
            # Map animations can advance between the two independent captures.
            assert max(ImageStat.Stat(diff).mean) < 3, (bounds, ImageStat.Stat(diff).mean)
        middle = (width // 2 - 60, height // 2 - 30, width // 2 + 60, height // 2 + 30)
        diff = ImageChops.difference(before.crop(middle), after.crop(middle))
        assert max(ImageStat.Stat(diff).mean) > 8, "objectives did not cover the map center"


if __name__ == "__main__":
    check(*sys.argv[1:])
