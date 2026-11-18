use std::{env, error::Error, ffi::OsString, fs, io, path::Path, process::ExitCode};

use engine_lite::{campaign, wml, wml_emit};

const USAGE: &str = "rustnoth-campaign --list <preprocessed-campaign.cfg>\n\
rustnoth-campaign <preprocessed-campaign.cfg> <scenario-id> <campaign-maps-directory>\n\
Writes the selected scenario and its campaign definitions to standard output.";

fn run(args: &[OsString]) -> Result<(), Box<dyn Error>> {
    match args {
        [flag] if flag == "--help" || flag == "-h" => println!("{USAGE}"),
        [flag, input] if flag == "--list" => {
            let campaign = wml::parse(fs::read(input)?)?;
            for (id, name) in campaign::scenarios(&campaign) {
                println!("{id}\t{name}");
            }
        }
        [input, id, maps] => {
            let id = id.to_str().ok_or("scenario id must be UTF-8")?;
            let campaign = wml::parse(fs::read(input)?)?;
            let bundle = campaign::select(&campaign, id, |file| fs::read_to_string(Path::new(maps).join(file)))?;
            wml_emit::write_config(io::stdout().lock(), &bundle)?;
        }
        _ => return Err(USAGE.into()),
    }
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<_> = env::args_os().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => { eprintln!("{error}"); ExitCode::FAILURE }
    }
}
