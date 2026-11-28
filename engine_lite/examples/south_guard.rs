//! Prepare and verify the two-scenario South Guard referee route.
use engine_lite::{
    audit_actions, audit_compare, audit_replay, fixed_match::{MatchState, carryover::CampaignCarryover},
    native_addon, native_draws, wml::{self, Node}, wml_emit, scenario_bundle::Bundle,
};
use std::{collections::BTreeSet, env, error::Error, fs, io, path::Path};

type Result<T> = std::result::Result<T, Box<dyn Error>>;
const IDS: [&str; 2] = ["01_Born_to_the_Banner", "02_Proven_by_the_Sword"];

fn children<'a>(node: &'a Node, name: &'a str) -> impl Iterator<Item = &'a Node> {
    node.children.iter().filter(move |(tag, _)| tag == name).map(|(_, child)| child)
}

fn prepare(inputs: &Path, out: &Path, scripts: Option<&Path>) -> Result<()> {
    for (i, id) in IDS.iter().enumerate() {
        let dir = out.join(id);
        let source = fs::read(inputs.join(format!("{id}.cfg")))?;
        let map = |name: &str| fs::read_to_string(inputs.join(name));
        if let Some(scripts) = scripts {
            let actions = audit_actions::parse(&fs::read_to_string(scripts.join(format!("{id}.tsv")))?)?;
            native_addon::install(&dir, &source, &actions, 5489, map)?;
        } else {
            let chooser = [include_str!("south_guard/banner.lua"), include_str!("south_guard/westin.lua")][i];
            native_addon::install_exploration(&dir, &source, 350, 5489, chooser, map)?;
        }
    }
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
struct Veteran { kind: String, experience: i32, gender: String, traits: Vec<String> }

fn veterans(side: &Node) -> Result<Vec<(String, Veteran)>> {
    let mut result = Vec::new();
    let mut ids = BTreeSet::new();
    for u in children(side, "unit") {
        let id = u.attribute("id").ok_or("veteran has no id")?;
        let traits = children(u, "modifications").flat_map(|m| children(m, "trait"))
            .filter_map(|t| t.attribute("id").map(str::to_owned)).collect();
        let veteran = Veteran {
            kind: u.attribute("type").ok_or("veteran has no type")?.to_owned(),
            experience: u.attribute("experience").unwrap_or("0").parse()?,
            gender: u.attribute("gender").unwrap_or("male").to_owned(), traits,
        };
        if !ids.insert(id) { return Err(format!("duplicate veteran {id}").into()); }
        result.push((id.to_owned(), veteran));
    }
    Ok(result)
}

/// Check authored route landmarks in the original recording after its rows have
/// passed the shared structural and per-step comparison.
fn landmarks(id: &str, trace: &str) -> Result<()> {
    let frames: Vec<_> = trace.trim_start_matches("step\t").split("\nstep\t").collect();
    let unit = |frame: &str, x, y, kind| {
        let prefix = format!("unit\t{x}\t{y}\t1\t{kind}\t");
        frame.lines().any(|line| line.starts_with(&prefix))
    };
    let enemy_gold = |frame: &str| frame.lines().find_map(|row| row.strip_prefix("gold\t2\t")?.parse::<i64>().ok());
    for (step, frame) in frames.iter().enumerate() {
        for side in [1, 2] {
            for row in [format!("view\t{side}\t1\t"), format!("seen\t{side}\t")] {
                if !frame.lines().any(|line| line.starts_with(&row)) {
                    return Err(format!("{id}: missing side {side} visibility at step {step}").into());
                }
            }
        }
    }
    let arrival = |x, y, kind, bonus: i64| frames.windows(2).any(|pair| {
        !unit(pair[0], x, y, kind) && unit(pair[1], x, y, kind)
            && enemy_gold(pair[0]).zip(enemy_gold(pair[1])).is_some_and(|(a,b)| b-a == bonus)
    });
    let visited = if id == IDS[0] {
        arrival(10, 9, "Infantry Lieutenant", 30) && arrival(12, 12, "Longbowman", 0)
    } else {
        arrival(20, 13, "White Mage", 40) && frames.iter().any(|frame| {
            unit(frame, 22, 13, "Horseman Commander") && unit(frame, 18, 12, "Spearman")
                && unit(frame, 18, 13, "Bowman") && unit(frame, 19, 15, "Peasant")
                && unit(frame, 20, 15, "Peasant")
        })
    };
    if !visited { return Err(format!("{id}: missing the route's authored reinforcement or gold change").into()); }
    Ok(())
}

const WESTIN_OBJECTIVES: [&str; 6] = [
    "Bring Deoran to the Citadel of Westin", "Defeat Urza Nalmath, the bandit leader",
    "Death of Deoran", "Death of Sir Gerrick", "Death of Minister Hylas", "Turns run out",
];

fn objective_history(log: &str, actions: usize) -> Result<Vec<&str>> {
    let mut rows = Vec::new();
    for text in log.lines().filter_map(|line| line.split_once("info wml: RUSTNOTH_WESTIN_OBJECTIVES ").map(|(_, v)| v)) {
        let (step, bits) = text.split_once(' ').ok_or("invalid original objective record")?;
        if step.parse::<usize>()? != rows.len() || bits.len() != WESTIN_OBJECTIVES.len() || !bits.bytes().all(|c| matches!(c, b'0' | b'1')) {
            return Err(format!("invalid original objectives at step {step}").into());
        }
        rows.push(bits);
    }
    if rows.len() != actions + 1 || rows.first() != Some(&"101101") || !rows.contains(&"011111") {
        return Err("missing original opening or liberated Westin objectives".into());
    }
    Ok(rows)
}

fn verify(inputs: &Path, stages: [&Path; 2], native_save: &Path, out: &Path) -> Result<()> {
    fs::create_dir(out)?;
    let core = fs::read(inputs.join("core/_main.cfg"))?;
    let save = wml::parse(fs::read(native_save)?)?;
    let incoming = children(&save, "carryover_sides_start").next().ok_or("missing original scenario-start carryover")?;
    if save.attribute("version") != Some("1.18.8") || save.attribute("difficulty") != Some("EASY")
        || save.attribute("campaign_type") != Some("scenario") || incoming.attribute("next_scenario") != Some(IDS[1]) {
        return Err("expected the original 1.18.8 Civilian second-scenario start save".into());
    }
    let saved = children(incoming, "side").find(|s| s.attribute("save_id") == Some("Deoran")).ok_or("missing Deoran's side")?;
    let next_seed = u32::from_str_radix(incoming.attribute("random_seed").ok_or("missing original next seed")?, 16)?;
    let mut campaign = CampaignCarryover::default();
    for (index, (&id, stage)) in IDS.iter().zip(stages).enumerate() {
        let seed = campaign.scenario_seed(5489);
        let mut doc = wml::parse(fs::read(inputs.join(format!("{id}.cfg")))?)?;
        let level = &mut doc.children.iter_mut().find(|(tag, _)| tag == "scenario").ok_or("missing scenario")?.1;
        campaign.gold.transfer_to(level);
        let mut source = Vec::new();
        wml_emit::write_config(&mut source, &doc)?;
        let bundle = Bundle::parse_with_core(&source, &core)?;
        let game = bundle.start_campaign(seed, |name| fs::read_to_string(inputs.join(name)), Some(&mut campaign))?;
        let log = fs::read_to_string(stage.join("native.stderr"))?;
        if !log.lines().any(|l| l.ends_with(&format!("info wml: RUSTNOTH_NATIVE_END {id}"))) {
            return Err(format!("{id}: missing the original campaign end").into());
        }
        if log.lines().any(|l| l.contains("error scripting/lua:") || l.contains("error ") && l.contains("[do_command]")) {
            return Err(format!("{id}: original command failed").into());
        }
        let original = fs::read_to_string(stage.join("native.stdout"))?;
        let actions = audit_actions::parse(&fs::read_to_string(stage.join("actions.tsv"))?)?;
        let draws = native_draws::for_actions(&log, &actions)?;
        let opening = native_draws::parse_opening(&log)?.ok_or("missing original opening RNG and genders")?;
        let destination = out.join(format!("{id}.tsv"));
        let objectives = (index == 1).then(|| objective_history(&log, actions.len())).transpose()?;
        let rng = match save.attribute("random_mode") {
            Some("deterministic" | "biased") => audit_replay::CaptureRng::ScenarioStream,
            _ => audit_replay::CaptureRng::CommandCheckpoints,
        };
        let options = audit_replay::ReplayOptions { all_sides_visibility: true, rng };
        let replay = audit_replay::replay_match(game, &actions, seed, fs::File::create(&destination)?, &draws, Some(&opening), options, |step, game| {
            if let Some(history) = &objectives {
                let current = game.objectives_for(1).ok_or_else(|| format!("step {step}: missing Westin objectives"))?;
                let bits: String = WESTIN_OBJECTIVES.iter().map(|label| {
                    if current.victory.iter().chain(&current.defeat).any(|entry| entry == label) { '1' } else { '0' }
                }).collect();
                if history[step] != bits { return Err(format!("step {step}: original Westin objectives {}, Rust {bits}", history[step]).into()); }
            }
            Ok(())
        });
        let trace = fs::read_to_string(destination)?;
        let comparison = audit_compare::compare(&trace, &original);
        let game = replay.map_err(|cause| match &comparison {
            Err(first) if !trace.is_empty() => format!("{id}: {cause}\n{first}"),
            _ => format!("{id}: {cause}"),
        })?;
        let frames = comparison.map_err(|e| format!("{id}: {e}"))?;
        if frames != actions.len() + 1 || game.state() != MatchState::CampaignVictory {
            return Err(format!("{id}: incomplete route or missing original victory").into());
        }
        landmarks(id, &original)?;
        let snapshot = game.audit_snapshot();
        println!("{id}: {frames} frames, {} actions, victory, remaining gold {}, retained {:?}",
            actions.len(), snapshot.gold[0], game.retained_gold(1));
        if index == 0 {
            game.store_campaign_survivors(&mut campaign);
            if campaign.scenario_seed(5489) != next_seed {
                return Err(format!("campaign seed after {id}: original {next_seed:08x}, Rust {:08x}", campaign.scenario_seed(5489)).into());
            }
            let roster = campaign.replay_roster();
            wml_emit::write_config(fs::File::create(out.join("outgoing.cfg"))?, &roster)?;
            let side = children(&roster, "side").find(|s| s.attribute("save_id") == Some("Deoran")).ok_or("missing Rust survivors")?;
            let (actual, expected) = (veterans(side)?, veterans(saved)?);
            let expected_gold: i64 = saved.attribute("gold").ok_or("missing original retained gold")?.parse()?;
            if actual != expected || game.retained_gold(1) != Some(expected_gold) {
                return Err(format!("campaign settlement differs; original {expected_gold} {expected:?}, Rust {:?} {actual:?}", game.retained_gold(1)).into());
            }
            println!("Original transfer: {expected_gold} gold, {expected:?}, next seed {next_seed:08x}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_referee_requires_both_players_vision_and_the_fort_gold_award() {
        let sight = "view\t1\t1\tV\nseen\t1\t1,1\nview\t2\t1\tV\nseen\t2\t1,1";
        let trace = format!("step\t0\tplaying\t1\t1\ngold\t2\t6\n{sight}\nstep\t1\tplaying\t1\t1\ngold\t2\t36\nunit\t10\t9\t1\tInfantry Lieutenant\t54\n{sight}\nstep\t2\tplaying\t1\t1\ngold\t2\t36\nunit\t12\t12\t1\tLongbowman\t48\n{sight}\n");
        assert!(landmarks(IDS[0], &trace).is_ok());
        assert!(landmarks(IDS[0], &trace.replacen("seen\t2", "seen\t1", 1)).is_err());
        assert!(landmarks(IDS[0], &trace.replace("gold\t2\t36", "gold\t2\t37")).is_err());
    }

    #[test]
    fn westin_requires_its_original_objective_change_and_the_ordered_roll() {
        let lines = "info wml: RUSTNOTH_WESTIN_OBJECTIVES 0 101101\ninfo wml: RUSTNOTH_WESTIN_OBJECTIVES 1 011111";
        assert!(objective_history(lines, 1).is_ok());
        assert!(objective_history(&lines.replace("011111", "101101"), 1).is_err());
        let unit = |id: &str| format!("[unit]\nid={id}\ntype=Pikeman\n[/unit]\n");
        let (first, second) = (unit("Moreth"), unit("Sir Gerrick"));
        let list = |a: &str, b: &str| veterans(&wml::parse(format!("{a}{b}")).unwrap()).unwrap();
        assert_ne!(list(&first, &second), list(&second, &first));
    }
}

fn main() -> Result<()> {
    let args: Vec<_> = env::args().skip(1).collect();
    match args.as_slice() {
        [command, inputs, output] if command == "prepare" => prepare(inputs.as_ref(), output.as_ref(), None),
        [command, inputs, output, scripts] if command == "prepare" => prepare(inputs.as_ref(), output.as_ref(), Some(scripts.as_ref())),
        [command, inputs, first, second, save, output] if command == "check" =>
            verify(inputs.as_ref(), [first.as_ref(), second.as_ref()], save.as_ref(), output.as_ref()),
        _ => Err(io::Error::other("south_guard prepare <inputs> <out> [fixed scripts dir] | check <inputs> <first capture> <second capture> <original incoming.cfg> <new output>").into()),
    }
}
