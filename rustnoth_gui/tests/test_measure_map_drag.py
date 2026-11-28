import tempfile
import unittest
from pathlib import Path

import numpy as np
from measure_map_drag import trace_pacing, translation


class DragProbe(unittest.TestCase):
    def test_detects_signed_terrain_motion_and_rejects_a_stationary_patch(self):
        patch = np.random.default_rng(5489).integers(0, 256, (96, 96))
        reference = np.fft.rfft2(patch)
        for delta in (-24, 0, 24):
            dx, dy, score = translation(reference, np.roll(patch, delta, axis=0))
            self.assertEqual((dx, dy), (0, delta))
            self.assertGreater(score, .95)

    def test_requires_the_same_process_and_display(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / "trace.tsv"
            path.write_text("elapsed_us\tevent\tvalue\n0\tprocess_id\t42\n0\twindow_width\t800\n"
                            "0\twindow_height\t600\n0\tdisplay_hz\t90\n0\tdefault_pacing\t0\n")
            self.assertEqual(trace_pacing(path, 42, 800, 600, 90), "default_pacing")
            with self.assertRaises(ValueError):
                trace_pacing(path, 41, 800, 600, 90)
