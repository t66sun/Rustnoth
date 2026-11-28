"""Run the two stock scenarios under the 1.18.8 single-player campaign controller."""
import argparse
import gzip
from pathlib import Path
import re
import shutil
import subprocess
import time

IDS = ("01_Born_to_the_Banner", "02_Proven_by_the_Sword")
DEFINE = "RUSTNOTH_SOUTH_GUARD_ACCEPTANCE"
WESTIN_OBJECTIVES = """
local capture = wesnoth.require('~add-ons/RustnothAudit/capture.lua')
capture.on_snapshot = function(step)
    local objectives = tostring(wesnoth.sides[1].objectives)
    local bits = {}
    for i, label in ipairs({"Bring Deoran to the Citadel of Westin", "Defeat Urza Nalmath, the bandit leader",
                            "Death of Deoran", "Death of Sir Gerrick", "Death of Minister Hylas", "Turns run out"}) do
        bits[i] = objectives:find(label, 1, true) and "1" or "0"
    end
    wesnoth.log("info", "RUSTNOTH_WESTIN_OBJECTIVES " .. step .. " " .. table.concat(bits), false)
end
"""


def configure(out, fixed):
    addon = out / "userdata/data/add-ons/RustnothAudit"
    addon.mkdir(parents=True)
    campaign = (
        f'[campaign]\nid=South_Guard_Acceptance\nname="The South Guard"\nabbrev=SGA\n'
        f'define={DEFINE}\nfirst_scenario={IDS[0]}\n[difficulty]\ndefine=EASY\nlabel=Civilian\n'
        'default=yes\n[/difficulty]\n[/campaign]\n'
    )
    pieces = [campaign, f"#ifdef {DEFINE}"]
    for index, scenario_id in enumerate(IDS):
        template = out / "templates" / scenario_id / "userdata/data/add-ons/RustnothAudit"
        original = (template / "_main.cfg").read_text(encoding="utf-8")
        before, scenario = original.split("[test]", 1)
        if index == 0:
            pieces.append(before)
            for name in ("actions.lua", "capture.lua"):
                shutil.copyfile(template / name, addon / name)
        script = (template / "script.lua").read_text(encoding="utf-8")
        script = "wesnoth.require('~add-ons/RustnothAudit/capture.lua').full_visibility=true\n" + script
        if index == 1:
            script = WESTIN_OBJECTIVES + script
        # In a unit test a matching prefix can stop the engine. A campaign stage
        # must end through its own WML, including when the fixed script runs out.
        end_call = r'wesnoth\.wml_actions\.endlevel\s*\{\s*test_result\s*=\s*"(?:pass|fail)"\s*\}'
        script, count = re.subn(end_call, 'error("acceptance action limit before authored victory")', script)
        if count != (2 if fixed else 1):
            raise ValueError("unexpected unit-test termination commands in the campaign driver")
        if not fixed:
            shutil.copyfile(template / "choice.lua", addon / f"choice{index}.lua")
            script = script.replace("RustnothAudit/choice.lua", f"RustnothAudit/choice{index}.lua")
        (addon / f"script{index}.lua").write_text(script, encoding="utf-8")
        scenario = scenario.replace('id="rustnoth_trace"', f'id="{scenario_id}"', 1)
        scenario = scenario.replace("RustnothAudit/script.lua", f"RustnothAudit/script{index}.lua")
        # The regular campaign controller owns the inter-scenario seed.
        scenario = re.sub(r'^    random_(?:seed|calls)="[^"]*"\s*$', '', scenario, flags=re.M)
        scenario = scenario.replace("[endlevel]", "[endlevel]\n            linger_mode=no\n            carryover_report=no")
        scenario = scenario.replace('[/test]', '[/scenario]')
        if index == 1:
            scenario = scenario.replace('next_scenario="03_A_Desperate_Errand"', 'next_scenario=""', 1)
        hook = r"wesnoth\.require\('~add-ons/RustnothAudit/capture.lua'\)\.campaign_end\((nil|\d+)\)"
        scenario, count = re.subn(hook, r"local c=wesnoth.require('~add-ons/RustnothAudit/capture.lua'); if c.campaign_pending() then c.campaign_end(\1); wesnoth.log('info','RUSTNOTH_NATIVE_END " + scenario_id + "',false) end", scenario)
        if count != 1:
            raise ValueError(f"{scenario_id} needs the original campaign end hook")
        pieces.append("[scenario]" + scenario)
    (addon / "_main.cfg").write_text("\n".join([*pieces, "#endif", ""]), encoding="utf-8")


def native_run(exe, out, deterministic):
    user = out / "userdata"
    saves = user / "saves"
    saves.mkdir()
    (saves / "Acceptance-entry").write_text(
        'version="1.18.8"\ncampaign_type=scenario\nera_id=era_default\ncampaign=South_Guard_Acceptance\n'
        f'campaign_define={DEFINE}\ndifficulty=EASY\nabbrev=SGA\n'
        + ('random_mode=deterministic\n' if deterministic else '')
        + f'[carryover_sides_start]\nnext_scenario={IDS[0]}\nrandom_seed=00001571\nrandom_calls=0\n[/carryover_sides_start]\n',
        encoding="utf-8",
    )
    (user / "preferences").write_text(
        "locale=en_US\nsave_replays=yes\nask_save_replay=no\nconfirm_end_turn=no\ndelete_saves=no\nshow_turn_dialog=no\nskip_ai_moves=yes\n",
        encoding="utf-8",
    )
    plugin = out / "dialogs.lua"
    plugin.write_text("return function() while true do local _,c,i=coroutine.yield(); "
                      "if i and i.name=='Game' and c.skip_dialog then c.skip_dialog({}) end end end", encoding="utf-8")
    command = [exe, "--wnoconsole", "--no-log-to-file", "--nogui", "--nosound", "--nobanner", "--nodelay",
               "--plugin", plugin, "--campaign-skip-story", "--rng-seed=5489", "--log-debug=random",
               "--log-info=wml,unit", "--userdata-dir", user, "--userconfig-dir", user,
               "--usercache-dir", out / "cache", "--load", "Acceptance-entry"]
    log = out / "native.stderr"
    with (out / "native.stdout").open("wb") as stdout, log.open("wb") as stderr:
        proc = subprocess.Popen(command, cwd=exe.parent, stdout=stdout, stderr=stderr)
        try:
            deadline = time.monotonic() + 120
            while proc.poll() is None and time.monotonic() < deadline:
                status = log.read_bytes()
                if b"error scripting/lua:" in status:
                    break
                if f"RUSTNOTH_NATIVE_END {IDS[1]}".encode() in status and any(saves.glob("SGA-Proven by the Sword replay*.gz")):
                    break
                time.sleep(.25)
        finally:
            if proc.poll() is None:
                proc.terminate()
            proc.wait(timeout=10)
    err = log.read_text(encoding="utf-8")
    if any(f"RUSTNOTH_NATIVE_END {scenario_id}" not in err for scenario_id in IDS):
        raise RuntimeError(f"original campaign did not finish both scenarios, see {log}")
    replays = list(saves.glob("SGA-Proven by the Sword replay*.gz"))
    if len(replays) != 1 or b"[replay]" not in gzip.decompress(replays[0].read_bytes()):
        raise RuntimeError("missing complete original second-scenario replay")
    for path in sorted(saves.iterdir()):
        data = path.read_bytes()
        if data.startswith(b"\x1f\x8b"):
            data = gzip.decompress(data)
        if (b"[carryover_sides_start]" in data and b"[snapshot]" not in data
                and re.search(rb'next_scenario="?02_Proven_by_the_Sword"?', data)
                and b"[replay_start]" not in data):
            (out / "incoming-native.cfg").write_bytes(data)
            return
    raise RuntimeError("original campaign did not produce the second scenario start save")


def split_capture(out, fixed):
    err = (out / "native.stderr").read_text(encoding="utf-8")
    first_end = re.search(rf"^.* info wml: RUSTNOTH_NATIVE_END {IDS[0]}\s*$", err, re.M)
    if not first_end:
        raise RuntimeError("missing original first-scenario end")
    stderr = [err[:first_end.end()], err[first_end.end():]]
    lines = (out / "native.stdout").read_text(encoding="utf-8").splitlines(keepends=True)
    starts = [i for i, line in enumerate(lines) if line.startswith("step\t0\t")]
    if len(starts) != 2:
        raise RuntimeError("expected exactly two original opening frames")
    (out / "actions").mkdir()
    for index, scenario_id in enumerate(IDS):
        stage = out / scenario_id
        stage.mkdir()
        text = stderr[index]
        (stage / "native.stderr").write_text(text, encoding="utf-8")
        frames = lines[starts[index]:starts[index+1] if index == 0 else len(lines)]
        (stage / "native.stdout").write_text("".join(frames), encoding="utf-8")
        if fixed:
            shutil.copyfile(fixed / f"{scenario_id}.tsv", stage / "actions.tsv")
        else:
            actions = re.findall(r" info wml: RUSTNOTH_AUDIT_COMMAND (\d+)\t([^\r\n]+)", text)
            if [int(n) for n, _ in actions] != list(range(1, len(actions)+1)):
                raise RuntimeError("missing original action")
            (stage / "actions.tsv").write_text("".join(f"{a}\n" for _, a in actions), encoding="utf-8")
        shutil.copyfile(stage / "actions.tsv", out / "actions" / f"{scenario_id}.tsv")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("wesnoth", "runner", "inputs", "output"):
        parser.add_argument(name, type=Path)
    parser.add_argument("--fixed-actions", type=Path, help="directory of scenario-id.tsv action scripts")
    parser.add_argument("--deterministic", action="store_true", help="use the original game's deterministic combat setting")
    args = parser.parse_args()
    exe, runner, inputs, out = (p.resolve() for p in (args.wesnoth, args.runner, args.inputs, args.output))
    version = subprocess.run([exe, "--wnoconsole", "--no-log-to-file", "--version"],
                             cwd=exe.parent, capture_output=True, timeout=20, check=True)
    if b"1.18.8" not in version.stdout + version.stderr:
        raise RuntimeError("expected original Wesnoth 1.18.8")
    out.mkdir()
    subprocess.run([runner, "prepare", inputs, out / "templates", *([args.fixed_actions] if args.fixed_actions else [])], check=True)
    configure(out, args.fixed_actions)
    native_run(exe, out, args.deterministic)
    split_capture(out, args.fixed_actions)
    subprocess.run([runner, "check", inputs, *(out / key for key in IDS), out / "incoming-native.cfg", out / "replay"], check=True)


if __name__ == "__main__":
    main()
