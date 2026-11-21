"""Compare the tutorial introduction, character chooser and welcome captured at the same window size."""
import sys

from PIL import Image, ImageChops, ImageStat


def check(introduction, character, welcome):
    with Image.open(introduction) as a, Image.open(character) as b, Image.open(welcome) as c:
        assert a.size == b.size == c.size
        width, height = a.size
        forest = (round(width*0.05), round(height*0.1), round(width*0.30), round(height*0.47))
        center = (round(width*0.405), round(height*0.41), round(width*0.595), round(height*0.60))
        images = [im.convert("RGB") for im in (a, b, c)]
        terrain = images[-1].crop(forest)
        assert max(ImageStat.Stat(terrain).stddev) > 10, "capture the visible tutorial map"
        for scene in images[:2]:
            delta = ImageChops.difference(scene.crop(forest), terrain)
            channels = delta.split()
            changed = ImageChops.lighter(ImageChops.lighter(channels[0], channels[1]), channels[2])
            assert sum(changed.histogram()[25:]) < terrain.width * terrain.height * 0.03, "tutorial terrain disappeared"
        diff = ImageChops.difference(images[0].crop(center), images[1].crop(center))
        assert max(ImageStat.Stat(diff).mean) > 15, "capture the character chooser over the introductory map"


if __name__ == "__main__":
    check(*sys.argv[1:])
