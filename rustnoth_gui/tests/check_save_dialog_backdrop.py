"""Check F12 screenshots before and during the battlefield's Save Game dialog."""
import sys

from PIL import Image, ImageChops, ImageStat


def check(board_file, save_file):
    with Image.open(board_file) as a, Image.open(save_file) as b:
        board, save = a.convert("RGB"), b.convert("RGB")
        assert board.size == save.size
        w, h = board.size
        scale = min(1.0, w / 516, h / 204)
        strip = (w // 16, h // 12, w * 2 // 3, int((h - 188 * scale) / 2) - 4)
        visible = board.crop(strip)
        assert min(ImageStat.Stat(visible).stddev) > 8, "capture a textured battlefield"
        dimmed = visible.point(lambda channel: round(channel * 130 / 255))
        delta = ImageChops.difference(dimmed, save.crop(strip))
        assert max(ImageStat.Stat(delta).mean) < 3, "save dialog lost the battlefield"
        x, y = round(w / 2 + 202 * scale), round(h / 2 - 14 * scale)
        assert save.getpixel((x, y)) == (63, 79, 100), "capture the initially selected save name"


if __name__ == "__main__":
    check(*sys.argv[1:])
