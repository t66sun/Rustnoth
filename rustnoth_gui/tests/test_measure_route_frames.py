import tempfile
import unittest
from pathlib import Path
from measure_route_frames import p95, report


class FrameWindows(unittest.TestCase):
    def test_load_crossing_a_long_frame_and_fog_returning_to_a_dialogue(self):
        with tempfile.TemporaryDirectory() as directory:
            trace = Path(directory) / "trace.tsv"
            trace.write_text("elapsed_us\tevent\tvalue\n0\tprocess_id\t42\n0\tdisplay_hz\t100\n1000\tframe\t0\n"
                             "3000\tload_input\t0\n45000\tmap_ready\t0\n46000\tframe\t0\n"
                             "51000\tmove_input\t0\n53000\trevealed_hexes\t3\n56000\tframe\t0\n"
                             "57000\tdialogue_ready\t0\n66000\tframe\t0\n", encoding="utf-8")
            data = report([trace])
        load = data["groups"]["load"]
        self.assertEqual((load["operations"], load["long_frames"], load["duration_p95_ms"]), (1, 1, 42))
        self.assertEqual(load["frame_p95_ms"], 45)
        self.assertEqual(data["groups"]["fog_reveal"]["first_available"], {"dialogue_ready": 1})
        self.assertEqual(data["groups"]["end_turn"]["operations"], 0)
        self.assertEqual(p95(range(1, 21)), 19)

    def stats(self, sequence):
        with tempfile.TemporaryDirectory() as directory:
            trace = Path(directory) / "trace.tsv"
            rows = ["elapsed_us\tevent\tvalue", "0\tprocess_id\t42", "0\tdisplay_hz\t100"]
            rows.extend(f"{time}\t{event}\t{value}" for time, event, value in sequence)
            trace.write_text("\n".join(rows), encoding="utf-8")
            return report([trace])["groups"]

    def test_ai_after_dialogue_and_animation_return_to_first_real_handler(self):
        groups = self.stats([(5,"frame",0), (6,"end_turn_input",0), (7,"map_ready",0),
                             (8,"ai_action_begin",0), (10,"ai_action_us",2), (11,"dialogue_ready",0),
                             (19,"frame",0), (20,"ai_action_begin",0), (21,"animation_begin",0),
                             (24,"animation_end",0), (27,"ai_action_begin",0), (32,"victory_ready",0),
                             (35,"frame",0)])
        self.assertEqual(groups["end_turn"]["operations"], 1)
        self.assertEqual(groups["ai"]["operations"], 2)
        self.assertEqual(groups["ai"]["first_available"], {"dialogue_ready": 1, "victory_ready": 1})
        self.assertEqual(groups["combat_animation"]["duration_p95_ms"], .003)
        self.assertEqual(groups["animation_recovery"]["duration_p95_ms"], .008)

    def test_preview_and_confirm_exclude_time_spent_choosing_a_save(self):
        groups = self.stats([(5,"frame",0), (7,"preview_begin",0), (20,"preview_end",0),
                             (22,"frame",0), (80,"load_input",0), (100,"objectives_ready",0), (102,"frame",0)])
        self.assertEqual(groups["load_preview"]["duration_p95_ms"], .013)
        self.assertEqual(groups["load"]["duration_p95_ms"], .020)
        self.assertEqual(groups["ai"]["operations"], 0)

    def test_truncated_frames_and_missing_ready_invalidate_the_group_percentile(self):
        for tail in ([], [(35, "map_ready", 0)]):
            groups = self.stats([(1,"frame",0), (2,"load_input",0), (6,"map_ready",0), (10,"frame",0),
                                 (12,"load_input",0), (14,"frame",0)] + tail)
            self.assertEqual(groups["load"]["operations"], 1)
            self.assertEqual(groups["load"]["unfinished"], 1)
            self.assertIsNone(groups["load"]["frame_p95_ms"])
            self.assertIsNone(groups["load"]["duration_p95_ms"])

    def test_rejects_changed_window_dimensions(self):
        with self.assertRaisesRegex(ValueError, "window_width changed"):
            self.stats([(1,"window_width",800), (2,"frame",0), (3,"window_width",900), (4,"frame",0)])

    def test_ai_completion_without_start_cannot_estimate_a_recovery_interval(self):
        with self.assertRaisesRegex(ValueError, "missing its beginning"):
            self.stats([(1,"frame",0), (4,"ai_action_us",2), (20_000,"dialogue_ready",0), (20_001,"frame",0)])
