use std::{env, error::Error, fs, io, path::Path, process::ExitCode};

use engine_lite::{audit_actions, audit_replay, scenario_bundle::Bundle};

const USAGE: &str = "rustnoth-step-trace <bundle.cfg> <actions.tsv> <seed>";

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
    let [bundle_path, actions_path, seed] = args.as_slice() else {
        return Err(USAGE.into());
    };
    let seed = seed
        .to_str()
        .and_then(|seed| seed.parse::<u32>().ok())
        .ok_or("seed must be an unsigned 32-bit integer")?;
    let bundle_path = Path::new(bundle_path);
    let actions_path = Path::new(actions_path);
    let directory = bundle_path.parent().unwrap_or_else(|| Path::new("."));
    let bytes = fs::read(bundle_path)
        .map_err(|error| format!("could not read {} ({error})", bundle_path.display()))?;
    let script = fs::read_to_string(actions_path)
        .map_err(|error| format!("could not read {} ({error})", actions_path.display()))?;
    let bundle = Bundle::parse(&bytes)?;
    let actions = audit_actions::parse(&script)?;
    audit_replay::run(
        &bundle,
        &actions,
        seed,
        |name| fs::read_to_string(directory.join(name)),
        io::stdout().lock(),
    )
}
