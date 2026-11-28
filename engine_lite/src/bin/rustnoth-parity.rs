use std::{
    collections::BTreeMap,
    env, error::Error, fs, io,
    path::{Path, PathBuf},
    process::ExitCode,
    time::Duration,
};

use engine_lite::{
    audit_actions, audit_compare, audit_replay, native_actions, native_addon, native_draws, native_process,
    fixed_match::Action,
    scenario_bundle::Bundle,
};

const USAGE: &str = "rustnoth-parity <wesnoth-executable> <bundle.cfg> <actions.tsv> <seed> <new-run-dir> [--core-config <preprocessed-core.cfg>]\n\
rustnoth-parity --explore <wesnoth-executable> <bundle.cfg> <max-actions> <seed> <new-run-dir> [--core-config <preprocessed-core.cfg>]\n\
rustnoth-parity --replay-capture <capture-dir> <bundle.cfg> <actions.tsv> <seed> <new-run-dir> [--core-config <preprocessed-core.cfg>]";
const RUST_TRACE: &str = "rust.tsv";

enum Commands {
    Script(Vec<Action>),
    Explore(usize),
}

#[derive(Clone, Copy)]
enum NativeSource<'a> {
    Executable(&'a Path),
    Capture(&'a Path),
}

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
        && (flag == "-h" || flag == "--help")
    {
        println!("{USAGE}");
        return Ok(());
    }
    let (native, explore, inputs) = match args.as_slice() {
        [flag, capture, inputs @ ..] if flag == "--replay-capture" => {
            (NativeSource::Capture(Path::new(capture)), false, inputs)
        }
        [flag, executable, inputs @ ..] if flag == "--explore" => {
            (NativeSource::Executable(Path::new(executable)), true, inputs)
        }
        [executable, inputs @ ..] => (NativeSource::Executable(Path::new(executable)), false, inputs),
        _ => return Err(USAGE.into()),
    };
    let [bundle, actions, seed, run_dir, options @ ..] = inputs else { return Err(USAGE.into()) };
    let core_config = match options {
        [] => None,
        [flag, path] if flag == "--core-config" => Some(Path::new(path)),
        _ => return Err(USAGE.into()),
    };
    let seed = seed
        .to_str()
        .and_then(|value| value.parse::<u32>().ok())
        .ok_or("seed must be an unsigned 32-bit integer")?;
    let bundle = Path::new(bundle);
    let run_dir = Path::new(run_dir);
    let commands = if explore {
        let limit = actions.to_str().and_then(|arg| arg.parse::<usize>().ok())
            .ok_or("max-actions must be a nonnegative integer")?;
        Commands::Explore(limit)
    } else {
        Commands::Script(audit_actions::parse(&fs::read_to_string(actions)?)?)
    };
    let matched = compare_referees(native, bundle, commands, seed, run_dir, core_config)?;
    println!("Matched {matched} step(s). Rust trace is in {}.", run_dir.display());
    Ok(())
}

fn compare_referees(
    native: NativeSource<'_>,
    bundle_path: &Path,
    commands: Commands,
    seed: u32,
    run_dir: &Path,
    core_config: Option<&Path>,
) -> Result<usize, Box<dyn Error>> {
    let source = fs::read(bundle_path)?;
    let bundle = match core_config {
        Some(path) => Bundle::parse_with_core(&source, &fs::read(path)?)?,
        None => Bundle::parse(&source)?,
    };
    let directory = bundle_path.parent().unwrap_or_else(|| Path::new("."));
    let mut external_map = None;
    let capture = match native {
        NativeSource::Executable(executable) => {
            native_process::check_version(executable)?;
            fs::create_dir(run_dir)?;
            let read_map = |name: &str| {
                let contents = fs::read_to_string(directory.join(name))?;
                external_map = Some((PathBuf::from(name), contents.clone()));
                Ok(contents)
            };
            match &commands {
                Commands::Script(actions) => native_addon::install(run_dir, &source, actions, seed, read_map)?,
                Commands::Explore(limit) => native_addon::install_exploration(
                    run_dir, &source, *limit, seed, native_actions::EXPLORE_CHOICE_SOURCE, read_map,
                )?,
            }
            let status = native_process::run(executable, run_dir, seed, Duration::from_secs(45))?;
            if !status.success() {
                return Err(format!("original referee failed ({status}); see {}",
                    run_dir.join(native_process::STDERR_FILE).display()).into());
            }
            run_dir
        }
        NativeSource::Capture(path) => {
            fs::create_dir(run_dir)?;
            path
        }
    };
    let native_error_path = capture.join(native_process::STDERR_FILE);
    let native_log = fs::read_to_string(&native_error_path)?;
    if native_command_error(&native_log) {
        return Err(format!("original referee rejected a command; see {}", native_error_path.display()).into());
    }
    if !native_log.lines().any(|line| line.trim() == "PASS TEST (0): rustnoth_trace") {
        return Err(format!("capture lacks a successful rustnoth_trace result; see {}", native_error_path.display()).into());
    }

    let actions = match commands {
        Commands::Script(actions) => actions,
        Commands::Explore(limit) => {
            let script = audit_actions::explored_script(&native_log, limit)?;
            fs::write(run_dir.join("actions.tsv"), &script)?;
            audit_actions::parse(&script)?
        }
    };
    let draws = original_draws(&native_log, &actions)
        .map_err(|error| format!("{error}; see {}", native_error_path.display()))?;
    let rust_file = run_dir.join(RUST_TRACE);
    let opening = native_draws::parse_opening(&native_log)?;
    let rust_result = audit_replay::run_with_capture(
        &bundle,
        &actions,
        seed,
        |name| match (native, external_map) {
            (NativeSource::Capture(_), _) => fs::read_to_string(directory.join(name)),
            (_, Some((loaded, contents))) if loaded == Path::new(name) => Ok(contents),
            _ => Err(io::Error::other("the referees selected different map inputs")),
        },
        fs::File::create_new(&rust_file)?,
        &draws,
        opening.as_ref(),
    );
    let rust_trace = fs::read_to_string(&rust_file)?;
    let native_trace = fs::read_to_string(capture.join(native_process::STDOUT_FILE))?;
    let comparison = audit_compare::compare(&rust_trace, &native_trace);
    if let Err(error) = rust_result {
        if rust_trace.is_empty() { return Err(error); }
        return match comparison {
            Err(difference) => Err(format!("{error}\n{difference}").into()),
            Ok(_) => Err(error),
        };
    }
    let count = comparison?;
    require_every_step(count, actions.len())?;
    Ok(count)
}

fn original_draws(log: &str, actions: &[Action]) -> Result<BTreeMap<usize, native_draws::AttackDraws>, String> {
    let mut draws = native_draws::parse(log)?;
    let mut recruits = native_draws::parse_recruits(log)?;
    for (index, action) in actions.iter().enumerate() {
        let step = index + 1;
        if matches!(action, Action::Recruit { .. }) {
            let recruit = recruits
                .remove(&step)
                .ok_or_else(|| format!("native recruit trait RNG is missing at step {step}"))?;
            if draws.insert(step, recruit).is_some() {
                return Err(format!("native attack RNG was also recorded at recruit step {step}"));
            }
        }
    }
    if let Some((&step, _)) = recruits.first_key_value() {
        return Err(format!("native recruit trait RNG recorded at non-recruit step {step}"));
    }
    Ok(draws)
}

fn native_command_error(log: &str) -> bool {
    log.lines().any(|line| {
        line.contains("error scripting/lua:")
            || (line.contains("error ") && line.contains("[do_command]"))
    })
}

fn require_every_step(frame_count: usize, action_count: usize) -> io::Result<()> {
    if frame_count != action_count + 1 {
        return Err(io::Error::other(format!(
            "matched {frame_count} frames for {action_count} actions; expected the initial frame and every action"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_lite::hex::Hex;

    #[test]
    fn replays_an_existing_native_capture_with_its_recorded_attack_rolls() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let root = env::temp_dir().join(format!("rustnoth-capture-{}-{nonce}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let source = NativeSource::Capture(&root);
        let bundle = root.join("fight.cfg");
        let actions = root.join("actions.tsv");
        fs::write(&bundle, concat!(
            "[terrain_type]\nstring=Gg\nid=flat\n[/terrain_type]\n[units]\n",
            "[movetype]\nname=foot\n[defense]\nflat=30\n[/defense]\n[/movetype]\n",
            "[unit_type]\nid=Fighter\nlevel=1\nhitpoints=10\nmovement_type=foot\n",
            "[attack]\nrange=melee\ndamage=5\nnumber=1\n[/attack]\n[/unit_type]\n[/units]\n",
            "[scenario]\nmap_file=fight.map\n[side]\nside=1\ntype=Fighter\n[/side]\n",
            "[side]\nside=2\ntype=Fighter\n[/side]\n[/scenario]\n",
        )).unwrap();
        fs::write(root.join("fight.map"), "Xv,Xv,Xv,Xv\nXv,1 Gg,2 Gg,Xv\nXv,Xv,Xv,Xv").unwrap();
        fs::write(&actions, "attack\t1\t1\t2\t1\t0\n").unwrap();
        let trace = concat!(
            "step\t0\tplaying\t1\t1\ngold\t1\t100\ngold\t2\t100\n",
            "unit\t1\t1\t1\tFighter\t10\t1\t1\t1\t0\nunit\t2\t1\t2\tFighter\t10\t1\t1\t1\t0\nend\t0\n",
            "step\t1\tplaying\t1\t1\ngold\t1\t100\ngold\t2\t100\n",
            "unit\t1\t1\t1\tFighter\t5\t0\t0\t1\t1\nunit\t2\t1\t2\tFighter\t5\t1\t1\t1\t1\nend\t1\n",
        );
        let log = concat!(
            "info wml: RUSTNOTH_AUDIT_COMMAND 1\tattack\t1\t1\t2\t1\t0\n",
            "info wml: RUSTNOTH_AUDIT_ATTACK_BEGIN 1\n",
            "debug random: pulled user random 3499211612 for call 1 with seed 1571\n",
            "debug random: pulled user random 581869302 for call 2 with seed 1571\n",
            "info wml: RUSTNOTH_AUDIT_ATTACK_END 1\nPASS TEST (0): rustnoth_trace\n",
        );
        fs::write(root.join(native_process::STDOUT_FILE), trace).unwrap();
        let stderr = root.join(native_process::STDERR_FILE);
        fs::write(&stderr, log).unwrap();
        fs::write(root.join(RUST_TRACE), "old Rust trace").unwrap();
        let output = root.join("replay");
        let compare = |destination: &Path| {
            let script = audit_actions::parse(&fs::read_to_string(&actions).unwrap()).unwrap();
            compare_referees(source, &bundle, Commands::Script(script), 0, destination, None)
        };
        assert_eq!(compare(&output).unwrap(), 2);
        assert_eq!(fs::read_to_string(output.join(RUST_TRACE)).unwrap(), trace);
        let explored = root.join("explored");
        assert_eq!(compare_referees(source, &bundle, Commands::Explore(4), 0, &explored, None).unwrap(), 2);
        assert_eq!(fs::read_to_string(explored.join("actions.tsv")).unwrap(), fs::read_to_string(&actions).unwrap());
        assert_eq!(fs::read_to_string(explored.join(RUST_TRACE)).unwrap(), trace);
        assert!(compare(&root).is_err());
        assert_eq!(fs::read_to_string(root.join(RUST_TRACE)).unwrap(), "old Rust trace");
        fs::write(&stderr, log.replace("PASS TEST (0)", "FAIL TEST (1)")).unwrap();
        assert!(compare(&root.join("failed")).unwrap_err().to_string().contains("successful rustnoth_trace"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_matching_prefix_cannot_stand_in_for_the_full_script() {
        assert!(require_every_step(3, 2).is_ok());
        assert!(require_every_step(2, 2).is_err());
        assert!(require_every_step(1, 0).is_ok());
    }

    #[test]
    fn a_rejected_native_command_cannot_pass_as_an_unchanged_frame() {
        assert!(native_command_error("error engine: Error via [do_command]:"));
        assert!(native_command_error("error scripting/lua: stack traceback"));
        assert!(!native_command_error("PASS TEST (0): rustnoth_trace"));
    }

    #[test]
    fn requires_a_native_recruit_checkpoint_even_when_no_traits_are_rolled() {
        let action = Action::Recruit { unit_type: "Fighter".to_owned(), to: Hex { x: 1, y: 0 } };
        assert_eq!(
            original_draws("", &[action.clone()]).unwrap_err(),
            "native recruit trait RNG is missing at step 1",
        );
        let log = concat!(
            "info wml: RUSTNOTH_AUDIT_RECRUIT_BEGIN 1\n",
            "info wml: RUSTNOTH_AUDIT_RECRUIT_END 1 0\n",
        );
        assert_eq!(original_draws(log, &[action]).unwrap()[&1], native_draws::AttackDraws::default());
        assert_eq!(
            original_draws(log, &[Action::EndTurn]).unwrap_err(),
            "native recruit trait RNG recorded at non-recruit step 1",
        );
    }
}
