use std::{
    env, error::Error, fs, io,
    path::{Path, PathBuf},
    process::ExitCode,
    time::Duration,
};

use engine_lite::{
    audit_actions, audit_compare, audit_replay, native_addon, native_process,
    scenario_bundle::Bundle,
};

const USAGE: &str = "rustnoth-parity <wesnoth-executable> <bundle.cfg> <actions.tsv> <seed> <new-run-dir>";
const RUST_TRACE: &str = "rust.tsv";

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
    let [executable, bundle, actions, seed, run_dir] = args.as_slice() else {
        return Err(USAGE.into());
    };
    let seed = seed
        .to_str()
        .and_then(|value| value.parse::<u32>().ok())
        .ok_or("seed must be an unsigned 32-bit integer")?;
    let bundle = Path::new(bundle);
    let run_dir = Path::new(run_dir);
    let native = Path::new(executable);
    let matched = compare_referees(native, bundle, Path::new(actions), seed, run_dir)?;
    println!("Matched {matched} step(s). Captures are in {}.", run_dir.display());
    Ok(())
}

fn compare_referees(
    executable: &Path,
    bundle_path: &Path,
    actions_path: &Path,
    seed: u32,
    run_dir: &Path,
) -> Result<usize, Box<dyn Error>> {
    let source = fs::read(bundle_path)?;
    let bundle = Bundle::parse(&source)?;
    let actions = audit_actions::parse(&fs::read_to_string(actions_path)?)?;
    native_process::check_version(executable)?;
    fs::create_dir(run_dir)?;

    let directory = bundle_path.parent().unwrap_or_else(|| Path::new("."));
    let mut external_map = None;
    native_addon::install(run_dir, &source, &actions, seed, |name| {
        let contents = fs::read_to_string(directory.join(name))?;
        external_map = Some((PathBuf::from(name), contents.clone()));
        Ok(contents)
    })?;

    let rust_file = run_dir.join(RUST_TRACE);
    let rust_result = audit_replay::run(
        &bundle,
        &actions,
        seed,
        |name| match external_map {
            Some((loaded, contents)) if loaded == Path::new(name) => Ok(contents),
            _ => Err(io::Error::other("the referees selected different map inputs")),
        },
        fs::File::create_new(&rust_file)?,
    );
    let native_status =
        native_process::run(executable, run_dir, seed, Duration::from_secs(45))?;
    let native_error_path = run_dir.join(native_process::STDERR_FILE);
    let native_log = fs::read_to_string(&native_error_path)?;
    if !native_status.success() || native_command_error(&native_log) {
        let rust_error = rust_result
            .err()
            .map_or_else(String::new, |error| format!("\nRust referee: {error}"));
        return Err(format!(
            "original referee failed ({native_status}); see {}{rust_error}",
            native_error_path.display()
        )
        .into());
    }

    let rust_trace = fs::read_to_string(&rust_file)?;
    let native_trace = fs::read_to_string(run_dir.join(native_process::STDOUT_FILE))?;
    let comparison = audit_compare::compare(&rust_trace, &native_trace);
    if let Err(error) = rust_result {
        return match comparison {
            Err(difference) => Err(format!("{error}\n{difference}").into()),
            Ok(_) => Err(error),
        };
    }
    let count = comparison?;
    require_every_step(count, actions.len())?;
    Ok(count)
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
}
