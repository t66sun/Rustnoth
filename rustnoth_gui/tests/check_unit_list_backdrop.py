"""Check F12 captures immediately before and while browsing the same side's Unit List."""
import sys

from PIL import Image, ImageChops, ImageStat


def check(board_path, roster_path):
    with Image.open(board_path) as board, Image.open(roster_path) as roster:
        assert board.size == roster.size
        width, height = board.size
        scale = min(1, width / 1106, height / 554)
        panel_top = (height - 538 * scale) / 2
        # Outside the modal, the regular battlefield remains under its dark scrim.
        strip = (10, 30, width - 192, int(panel_top) - 5)
        assert strip[2] > strip[0] and strip[3] > strip[1], "capture a desktop-sized window"
        terrain = board.convert("RGB").crop(strip)
        assert max(ImageStat.Stat(terrain).stddev) > 10, "reference must show the battlefield"
        expected = terrain.point(lambda channel: round(channel * 130 / 255))
        difference = ImageChops.difference(expected, roster.convert("RGB").crop(strip))
        assert max(ImageStat.Stat(difference).mean) < 3, "the roster lost its live battlefield backdrop"
        center = (width // 2, height // 2)
        assert max(roster.convert("RGB").getpixel(center)) > 0, "the roster is blank"


if __name__ == "__main__":
    check(*sys.argv[1:])
