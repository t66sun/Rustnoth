use std::{env, error::Error, fs, io, num::NonZeroUsize, path::Path, process::ExitCode};

use engine_lite::{
    fixed_match::{Action, MatchState},
    opponent::attack_or_approach,
    scenario_bundle::Bundle,
};

const USAGE: &str = "rustnoth-headless <preprocessed-bundle.cfg> <seed> <action-limit> [--core-config <preprocessed-core.cfg>]\n\
    The bundle contains one [scenario] block and can omit [units] when a core configuration is supplied.";

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
    let [bundle, seed, action_limit, options @ ..] = args.as_slice() else {
        return Err(USAGE.into());
    };
    let core_config = match options {
        [] => None,
        [flag, path] if flag == "--core-config" => Some(Path::new(path)),
        _ => return Err(USAGE.into()),
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
    let read_config = |path: &Path| {
        fs::read(path).map_err(|error| format!("could not read {} ({error})", path.display()))
    };
    let data = read_config(bundle)?;
    let core = core_config.map(read_config).transpose()?;
    let state = play_bundle(&data, core.as_deref(), seed, action_limit, |name| {
        fs::read_to_string(directory.join(name))
    })?;
    match state {
        MatchState::Won(side) => println!("Side {side} won."),
        MatchState::AlliedVictory(sides) => {
            let winners = sides.iter().map(usize::to_string).collect::<Vec<_>>().join(", ");
            println!("Sides {winners} won.");
        }
        MatchState::CampaignVictory => println!("Campaign victory."),
        MatchState::CampaignDefeat => println!("Campaign defeat."),
        MatchState::Draw => println!("Draw."),
        MatchState::Playing(_) => unreachable!("play_bundle returns a terminal match"),
    }
    Ok(())
}

fn play_bundle(
    data: &[u8],
    core: Option<&[u8]>,
    seed: u32,
    action_limit: NonZeroUsize,
    read_map_file: impl FnOnce(&str) -> io::Result<String>,
) -> Result<MatchState, Box<dyn Error>> {
    let bundle = match core {
        Some(core) => Bundle::parse_with_core(data, core)?,
        None => Bundle::parse(data)?,
    };
    let mut game = bundle.start(seed, read_map_file)?;
    for _ in 0..action_limit.get() {
        let Some((observation, actions)) = game.decision() else {
            break;
        };
        let action = attack_or_approach(&observation, &actions).cloned().unwrap_or(Action::EndTurn);
        game.step(action)?;
    }
    match game.state() {
        MatchState::Playing(_) => {
            Err(format!("action limit of {action_limit} reached before the match ended").into())
        }
        state => Ok(state),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RULES: &str = r#"
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
        "#;
    const SCENARIO: &str = r#"
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

    fn read_smoke_map(name: &str) -> io::Result<String> {
        assert_eq!(name, "smoke.map");
        Ok("Xv,Xv,Xv,Xv,Xv\nXv,1 Gg,Gg,2 Gg,Xv\nXv,Xv,Xv,Xv,Xv".to_owned())
    }

    #[test]
    fn bundled_match_finishes_at_its_action_limit_and_rejects_a_shorter_limit() {
        let bundle = format!("{RULES}{SCENARIO}");
        let limit = NonZeroUsize::new(2).unwrap();
        assert_eq!(
            play_bundle(bundle.as_bytes(), None, 5489, limit, read_smoke_map).unwrap(),
            MatchState::Won(1)
        );
        assert_eq!(
            play_bundle(bundle.as_bytes(), None, 5489, NonZeroUsize::MIN, read_smoke_map)
                .unwrap_err()
                .to_string(),
            "action limit of 1 reached before the match ended"
        );
    }

    #[test]
    fn scenario_only_match_loads_its_unit_and_terrain_rules_from_core() {
        let limit = NonZeroUsize::new(2).unwrap();
        let state = play_bundle(SCENARIO.as_bytes(), Some(RULES.as_bytes()), 5489, limit, read_smoke_map)
            .unwrap();
        assert_eq!(state, MatchState::Won(1));
    }
}
