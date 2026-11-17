use std::{env, error::Error, fs, io, path::Path, process::ExitCode};

use engine_lite::{audit_actions, audit_replay, scenario_bundle::Bundle};

const USAGE: &str =
    "rustnoth-step-trace <bundle.cfg> <actions.tsv> <seed> [--core-config <preprocessed-core.cfg>]";

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
    let [bundle_path, actions_path, seed, options @ ..] = args.as_slice() else {
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
    let bundle_path = Path::new(bundle_path);
    let actions_path = Path::new(actions_path);
    let directory = bundle_path.parent().unwrap_or_else(|| Path::new("."));
    let read_config = |path: &Path| {
        fs::read(path).map_err(|error| format!("could not read {} ({error})", path.display()))
    };
    let bytes = read_config(bundle_path)?;
    let core = core_config.map(read_config).transpose()?;
    let script = fs::read_to_string(actions_path)
        .map_err(|error| format!("could not read {} ({error})", actions_path.display()))?;
    trace_bundle(
        &bytes,
        core.as_deref(),
        &script,
        seed,
        |name| fs::read_to_string(directory.join(name)),
        io::stdout().lock(),
    )
}

fn trace_bundle(
    source: &[u8],
    core: Option<&[u8]>,
    script: &str,
    seed: u32,
    read_map_file: impl FnOnce(&str) -> io::Result<String>,
    out: impl io::Write,
) -> Result<(), Box<dyn Error>> {
    let bundle = match core {
        Some(core) => Bundle::parse_with_core(source, core)?,
        None => Bundle::parse(source)?,
    };
    let actions = audit_actions::parse(script)?;
    audit_replay::run(&bundle, &actions, seed, read_map_file, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traces_a_scenario_only_bundle_using_core_units_and_terrain() {
        let core = br#"
            [terrain_type]
                string=Gg
                id=flat
            [/terrain_type]
            [units]
                [movetype]
                    name=smallfoot
                    [movement_costs]
                        flat=1
                    [/movement_costs]
                [/movetype]
                [unit_type]
                    id=Bowman
                    hitpoints=33
                    movement=5
                    movement_type=smallfoot
                [/unit_type]
            [/units]
        "#;
        let scenario = br#"
            [scenario]
                map_file=field.map
                [side]
                    side=1
                    type=Bowman
                [/side]
                [side]
                    side=2
                    type=Bowman
                [/side]
            [/scenario]
        "#;
        let mut trace = Vec::new();
        trace_bundle(
            scenario,
            Some(core),
            "move\t1\t1\t2\t1\nend_turn\n",
            5489,
            |name| {
                assert_eq!(name, "field.map");
                Ok("Xv,Xv,Xv,Xv,Xv,Xv\nXv,1 Gg,Gg,Gg,2 Gg,Xv\nXv,Xv,Xv,Xv,Xv,Xv".to_owned())
            },
            &mut trace,
        )
        .unwrap();
        let trace = String::from_utf8(trace).unwrap();
        assert!(trace.contains("unit\t2\t1\t1\tBowman\t33\t4\t1\t1\t0\n"));
        assert!(trace.contains("step\t2\tplaying\t1\t2\n"));
    }
}
