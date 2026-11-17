use std::{env, error::Error, fs, path::Path, process::ExitCode};

use engine_lite::audit_compare::compare;

const USAGE: &str = "rustnoth-trace-diff <rust.tsv> <native.tsv>";

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
    let [rust, native] = args.as_slice() else {
        return Err(USAGE.into());
    };
    let read = |path: &Path| {
        fs::read_to_string(path)
            .map_err(|error| format!("could not read {} ({error})", path.display()))
    };
    let rust = read(Path::new(rust))?;
    let native = read(Path::new(native))?;
    let count = compare(&rust, &native)?;
    println!("Matched {count} step(s).");
    Ok(())
}
