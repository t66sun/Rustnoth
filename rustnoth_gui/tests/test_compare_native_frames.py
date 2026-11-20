import unittest
from PIL import Image
from compare_native_frames import accepted, difference


class WholeFrameComparison(unittest.TestCase):
    def test_one_channel_one_level_is_a_mismatch_and_97_percent_is_insufficient(self):
        reference = Image.new("RGB", (10, 10))
        actual = reference.copy()
        for index, color in enumerate(((1, 0, 0), (0, 1, 0), (0, 0, 1))):
            actual.putpixel((index, 0), color)
        self.assertEqual(difference(reference, actual).histogram()[0], 97)
        self.assertFalse(accepted(97, 100))
        self.assertTrue(accepted(98, 100))
        with self.assertRaises(ValueError):
            difference(reference, actual.crop((1, 0, 10, 10)))
