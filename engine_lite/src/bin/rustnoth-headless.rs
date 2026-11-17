use std::{
    collections::BTreeMap, env, error::Error, fs, io, num::NonZeroUsize, path::Path,
    process::ExitCode,
};

use engine_lite::{
    fixed_match::{Action, FixedMatch, MatchState, Rules},
    hex::Hex,
    movement_types::read_movement_types,
    observation::Observation,
    resistance::DamageTable,
    scenario_start,
    terrain_aliases::TerrainAliases,
    unit_type::read_unit_types,
    wml::{self, Node},
};

const USAGE: &str = "rustnoth-headless <preprocessed-bundle.cfg> <seed> <action-limit>\n\
    The bundle contains top-level [terrain_type] definitions, one [units] block and one [scenario] block.";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if let [flag] = args.as_slice()
        && (flag == "--help" || flag == "-h")
    {
        println!("{USAGE}");
        return Ok(());
    }
    let [bundle, seed, action_limit] = args.as_slice() else {
        return Err(USAGE.into());
    };
    let seed = seed
        .to_str()
        .and_then(|seed| seed.parse::<u32>().ok())
        .ok_or("seed must be an unsigned 32-bit integer")?;
    let action_limit = action_limit
        .to_str()
        .and_then(|limit| limit.parse::<NonZeroUsize>().ok())
        .ok_or("action-limit must be a positive integer")?;
    let bundle = Path::new(bundle);
    let directory = bundle.parent().unwrap_or_else(|| Path::new("."));
    let data =
        fs::read(bundle).map_err(|error| format!("could not read {} ({error})", bundle.display()))?;
    let state = play_bundle(&data, seed, action_limit, |name| {
        fs::read_to_string(directory.join(name))
    })?;
    match state {
        MatchState::Won(side) => println!("Side {side} won."),
        MatchState::Draw => println!("Draw."),
        MatchState::Playing(_) => unreachable!("play_bundle returns a terminal match"),
    }
    Ok(())
}

fn play_bundle(
    data: &[u8],
    seed: u32,
    action_limit: NonZeroUsize,
    read_map_file: impl FnOnce(&str) -> io::Result<String>,
) -> Result<MatchState, Box<dyn Error>> {
    let bundle = wml::parse(data)?;
    let terrain = TerrainAliases::from_wml(&bundle);
    let mut units = None;
    let mut scenario = None;
    for (tag, node) in bundle.children {
        let slot = match tag.as_str() {
            "units" => &mut units,
            "scenario" => &mut scenario,
            _ => continue,
        };
        if slot.replace(node).is_some() {
            return Err(format!("expected exactly one [{tag}] block").into());
        }
    }
    let units = units.ok_or("missing [units] block")?;
    let scenario = scenario.ok_or("missing [scenario] block")?;
    let movement = read_movement_types(&units);
    let resistance = read_resistance(&units);
    let unit_types = read_unit_types(units);
    let start = scenario_start::load(&scenario, &unit_types, read_map_file)?;
    let rules = Rules {
        terrain: &terrain,
        movement: &movement,
        resistance: &resistance,
    };
    let mut game = FixedMatch::new(start, rules, seed)?;
    for _ in 0..action_limit.get() {
        let Some((observation, actions)) = game.decision() else {
            break;
        };
        game.step(choose_action(&observation, actions))?;
    }
    match game.state() {
        MatchState::Playing(_) => {
            Err(format!("action limit of {action_limit} reached before the match ended").into())
        }
        state => Ok(state),
    }
}

fn read_resistance(units: &Node) -> BTreeMap<String, DamageTable> {
    let mut tables = BTreeMap::new();
    for (tag, node) in &units.children {
        if tag == "movetype" {
            let name = node.attribute("name").unwrap_or_default().to_owned();
            tables.entry(name).or_insert_with(|| {
                node.children
                    .iter()
                    .find(|(tag, _)| tag == "resistance")
                    .map(|(_, table)| DamageTable::from_wml(table))
                    .unwrap_or_default()
            });
        }
    }
    tables
}

fn choose_action(observation: &Observation, actions: Vec<Action>) -> Action {
    let nearest_enemy = |hex: Hex| {
        observation
            .units
            .iter()
            .filter(|unit| unit.side != observation.side)
            .map(|unit| hex.distance(unit.position))
            .min()
    };
    let mut approach = None;
    for action in actions {
        match &action {
            Action::Attack(_) => return action,
            Action::Move { from, to } if approach.is_none() => {
                if let (Some(before), Some(after)) = (nearest_enemy(*from), nearest_enemy(*to))
                    && after < before
                {
                    approach = Some(action);
                }
            }
            _ => {}
        }
    }
    approach.unwrap_or(Action::EndTurn)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_match_finishes_at_its_action_limit_and_rejects_a_shorter_limit() {
        let bundle = br#"
            [terrain_type]
                string=Gg
                id=flat
            [/terrain_type]
            [units]
                [movetype]
                    name=foot
                    [movement_costs]
                        flat=1
                    [/movement_costs]
                    [defense]
                        flat=100
                    [/defense]
                    [resistance]
                        blade=200
                    [/resistance]
                [/movetype]
                [unit_type]
                    id=Fighter
                    hitpoints=10
                    movement=3
                    movement_type=foot
                    [attack]
                        name=sword
                        type=blade
                        range=melee
                        damage=6
                        number=1
                    [/attack]
                [/unit_type]
            [/units]
            [scenario]
                map_file=smoke.map
                [side]
                    side=1
                    type=Fighter
                [/side]
                [side]
                    side=2
                    type=Fighter
                [/side]
            [/scenario]
        "#;
        let read_map = |name: &str| {
            assert_eq!(name, "smoke.map");
            Ok("Xv,Xv,Xv,Xv,Xv\nXv,1 Gg,Gg,2 Gg,Xv\nXv,Xv,Xv,Xv,Xv".to_owned())
        };
        assert_eq!(
            play_bundle(bundle, 5489, NonZeroUsize::new(2).unwrap(), read_map).unwrap(),
            MatchState::Won(1)
        );
        assert_eq!(
            play_bundle(bundle, 5489, NonZeroUsize::MIN, read_map)
                .unwrap_err()
                .to_string(),
            "action limit of 1 reached before the match ended"
        );
    }
}
