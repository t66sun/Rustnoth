"""Check two same-size framebuffer captures from the campaign list and its difficulty dialog."""
import sys

from PIL import Image, ImageChops, ImageStat


def check(campaign_path, difficulty_path):
    with Image.open(campaign_path) as campaign, Image.open(difficulty_path) as difficulty:
        assert campaign.size == difficulty.size, (campaign.size, difficulty.size)
        width, height = campaign.size
        strip = max(1, width // 20)
        for start in (0, width - strip):
            bounds = (start, 0, start + strip, height)
            original = campaign.crop(bounds).convert("RGB")
            assert max(ImageStat.Stat(original).stddev) > 10, "missing campaign background"
            expected = original.point(lambda level: round(level * 175 / 255))
            actual = difficulty.crop(bounds).convert("RGB")
            error = ImageStat.Stat(ImageChops.difference(actual, expected)).mean
            assert max(error) < 3, (bounds, error)


if __name__ == "__main__":
    check(*sys.argv[1:])
