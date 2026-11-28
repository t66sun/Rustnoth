"""Summarize actual desktop frame intervals and time to the first available input handler."""
import argparse
import bisect
import csv
import json
import math
from pathlib import Path


SETTINGS = {"window_width", "window_height", "display_hz", "default_pacing", "uncapped"}
READY = {"map_ready", "dialogue_ready", "objectives_ready", "victory_ready", "turn_notice_ready", "replay_ready"}
KINDS = ("load_preview", "load", "end_turn", "ai", "move", "fog_reveal", "combat", "combat_animation", "animation_recovery")


def p95(values):
    return sorted(values)[math.ceil(.95 * len(values)) - 1] if values else None


def summarize(path):
    with path.open(encoding="utf-8") as stream:
        rows = [(int(row["elapsed_us"]), row["event"], int(row["value"]))
                for row in csv.DictReader(stream, delimiter="\t")]
    settings = {}
    if not any(event == "process_id" and value > 0 for _, event, value in rows):
        raise ValueError(f"trace predates explicit AI beginnings and modal readiness in {path}")
    for _, event, value in rows:
        if event in SETTINGS and settings.setdefault(event, value) != value:
            raise ValueError(f"{event} changed during {path}")
    if settings.get("display_hz", 0) <= 0:
        raise ValueError(f"no measured display refresh rate in {path}")
    frames = [t for t, event, _ in rows if event == "frame"]
    if len(frames) < 2 or any(a[0] > b[0] for a, b in zip(rows, rows[1:])):
        raise ValueError(f"insufficient or unordered frame samples in {path}")
    samples, incomplete = [], []
    pending = {}
    animations_finished = []
    revealed = 0

    def finish(kind, start, end, ready, hexes=0):
        if start < frames[0] or end > frames[-1]:
            incomplete.append({"operation": kind, "start_us": start, "reason": "missing enclosing frame"})
            return
        first = max(1, bisect.bisect_right(frames, start))
        stop = bisect.bisect_left(frames, end) + 1
        intervals = [(frames[i] - frames[i-1]) / 1000 for i in range(first, stop)]
        samples.append({"operation": kind, "start_us": start, "end_us": end, "ready": ready,
                        "duration_ms": (end-start)/1000, "revealed_hexes": hexes, "frames_ms": intervals})

    def begin(kind, t):
        if kind in pending:
            incomplete.append({"operation": kind, "start_us": pending[kind], "reason": "superseded"})
        pending[kind] = t

    for t, event, value in rows:
        if event in ("move_input", "end_turn_input", "combat_input", "load_input"):
            begin(event.removesuffix("_input"), t)
            if event == "move_input":
                revealed = 0
        elif event == "revealed_hexes" and "move" in pending:
            revealed += value
        elif event == "ai_action_begin":
            pending.setdefault("ai", t)
        elif event == "ai_action_us" and "ai" not in pending:
            raise ValueError(f"computer action missing its beginning in {path}")
        elif event == "animation_begin":
            begin("combat_animation", t)
        elif event == "animation_end" and "combat_animation" in pending:
            finish("combat_animation", pending.pop("combat_animation"), t, event)
            animations_finished.append(t)
        elif event == "preview_begin":
            begin("load_preview", t)
        elif event == "preview_end" and "load_preview" in pending:
            finish("load_preview", pending.pop("load_preview"), t, event)
        elif event in READY:
            for kind in ("load", "end_turn", "ai", "move", "combat"):
                if kind in pending:
                    fog = revealed if kind == "move" else 0
                    finish("fog_reveal" if fog else kind, pending.pop(kind), t, event, fog)
            for end in animations_finished:
                finish("animation_recovery", end, t, event)
            animations_finished.clear()
    for kind, start in pending.items():
        incomplete.append({"operation": "fog_reveal" if kind == "move" and revealed else kind,
                           "start_us": start, "reason": "no end marker"})
    for end in animations_finished:
        incomplete.append({"operation": "animation_recovery", "start_us": end, "reason": "no input handler"})
    return settings, samples, incomplete


def report(paths):
    samples, runs = [], []
    for path in paths:
        settings, observed, incomplete = summarize(path)
        runs.append({"file": str(path), "settings": settings, "unfinished": incomplete})
        if runs[0]["settings"] != settings:
            raise ValueError("traces require identical window, refresh rate and pacing")
        samples.extend(observed)
    threshold = 2000 / runs[0]["settings"]["display_hz"]
    groups = {}
    for kind in KINDS:
        group = [r for r in samples if r["operation"] == kind]
        lost = sum(r["operation"] == kind for run in runs for r in run["unfinished"])
        intervals = [ms for r in group for ms in r["frames_ms"]]
        groups[kind] = {"operations": len(group), "unfinished": lost, "frames": len(intervals),
                        "frame_p95_ms": None if lost else p95(intervals),
                        "worst_frame_ms": max(intervals, default=None),
                        "long_frames": sum(ms > threshold for ms in intervals),
                        "duration_p95_ms": None if lost else p95([r["duration_ms"] for r in group]),
                        "first_available": {k: sum(r["ready"] == k for r in group)
                                            for k in sorted({r["ready"] for r in group})}}
    return {"runs": runs, "longer_than_ms": threshold, "groups": groups, "samples": samples}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("traces", nargs="+", type=Path)
    parser.add_argument("--out", required=True, type=Path)
    args = parser.parse_args()
    data = report(args.traces)
    with args.out.open("x", encoding="utf-8") as output:
        json.dump(data, output, indent=2)
    print(json.dumps(data["groups"], indent=2))
