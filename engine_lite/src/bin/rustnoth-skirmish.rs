use std::{env, error::Error, ffi::OsString, fs, io, path::Path, process::ExitCode};

use engine_lite::{
    skirmish::{self, Faction, Visibility},
    wml_emit,
};

const USAGE: &str = "rustnoth-skirmish <map> <turns> [--visibility open|fog|shroud|both] <faction.cfg> [--leader <unit-id>] <faction.cfg> [--leader <unit-id>] ...\nWrites a skirmish scenario bundle to standard output. Visibility defaults to open.";

fn main() -> ExitCode {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if let [flag] = args.as_slice() && (flag == "-h" || flag == "--help") {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[OsString]) -> Result<(), Box<dyn Error>> {
    let [map, turns, choices @ ..] = args else { return Err(USAGE.into()) };
    let turns = turns.to_str().and_then(|s| s.parse::<i32>().ok()).ok_or(USAGE)?;
    let map = Path::new(map);
    let map_data = fs::read_to_string(map).map_err(|e| format!("{}: {e}", map.display()))?;
    let title = map.file_stem().ok_or("map needs a filename")?.to_string_lossy();
    let mut factions = Vec::new();
    let mut visibility = None;
    let mut choices = choices.iter().peekable();
    while let Some(path) = choices.next() {
        if path == "--visibility" {
            if visibility.is_some() { return Err("--visibility may only be specified once".into()); }
            let value = choices.next().and_then(|arg| arg.to_str()).ok_or(USAGE)?;
            visibility = Some(value.parse::<Visibility>()?);
            continue;
        }
        if path == "--leader" {
            return Err(USAGE.into());
        }
        let path = Path::new(path);
        let bytes = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let faction = Faction::parse(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        let leader = if choices.next_if(|&arg| arg == "--leader").is_some() {
            Some(choices.next().and_then(|id| id.to_str()).ok_or(USAGE)?)
        } else {
            None
        };
        factions.push((faction, leader));
    }
    let sides: Vec<_> = factions.iter().map(|(faction, leader)| (faction, *leader)).collect();
    let document = skirmish::scenario_with_visibility(
        &title, &map_data, turns, &sides, visibility.unwrap_or_default(),
    )?;
    Ok(wml_emit::write_config(io::stdout().lock(), &document)?)
}
