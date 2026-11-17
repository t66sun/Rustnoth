use std::{env, error::Error, fs, path::Path, process::ExitCode};

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

fn compare(rust: &str, native: &str) -> Result<usize, String> {
    let rust = parse_trace(rust).map_err(|error| format!("malformed Rust trace: {error}"))?;
    let native = parse_trace(native).map_err(|error| format!("malformed native trace: {error}"))?;
    for step in 0..rust.len().max(native.len()) {
        let actual = rust.get(step).map_or(&[][..], Vec::as_slice);
        let expected = native.get(step).map_or(&[][..], Vec::as_slice);
        for row in 0..actual.len().max(expected.len()) {
            let actual = actual.get(row).copied();
            let expected = expected.get(row).copied();
            if actual != expected {
                let actual = actual.unwrap_or("<missing>");
                let expected = expected.unwrap_or("<missing>");
                return Err(format!(
                    "step {step} differs\n  expected native: {expected}\n  actual Rust:     {actual}"
                ));
            }
        }
    }
    Ok(rust.len())
}

fn parse_trace(text: &str) -> Result<Vec<Vec<&str>>, String> {
    let mut lines = text.lines().enumerate();
    let mut frames = Vec::new();
    while let Some((line, header)) = lines.next() {
        let step = frames.len();
        let bad_row = |line, message| format!("step {step}, line {}: {message}", line + 1);
        let fields: Vec<_> = header.split('\t').collect();
        let valid_header = match fields.as_slice() {
            ["step", index, "playing", round, side] => {
                index.parse::<usize>() == Ok(step)
                    && round.parse::<i32>().is_ok()
                    && valid_side(side)
            }
            ["step", index, "won", winner] => {
                index.parse::<usize>() == Ok(step) && valid_side(winner)
            }
            ["step", index, "draw"] => index.parse::<usize>() == Ok(step),
            _ => false,
        };
        if !valid_header {
            return Err(bad_row(line, "expected a step header with the next index"));
        }

        let mut rows = vec![header];
        for side in [1, 2] {
            let Some((line, row)) = lines.next() else {
                return Err(format!("step {step}: missing gold row for side {side}"));
            };
            let fields: Vec<_> = row.split('\t').collect();
            if !matches!(fields.as_slice(), ["gold", owner, amount]
                if owner.parse::<u8>() == Ok(side) && amount.parse::<i64>().is_ok())
            {
                return Err(bad_row(line, "expected gold rows in side order"));
            }
            rows.push(row);
        }

        let mut last_unit = None;
        let mut last_village = None;
        loop {
            let Some((line, row)) = lines.next() else {
                return Err(format!("step {step}: missing end row"));
            };
            let fields: Vec<_> = row.split('\t').collect();
            match fields.as_slice() {
                ["end", index] if index.parse::<usize>() == Ok(step) => break,
                ["unit", x, y, side, kind, hp, mp, attacks, "0" | "1"]
                    if last_village.is_none()
                        && valid_side(side)
                        && !kind.is_empty()
                        && !kind.contains('\r')
                        && hp.parse::<i32>().is_ok()
                        && mp.parse::<i32>().is_ok()
                        && attacks.parse::<i32>().is_ok()
                        && next_position(x, y, &mut last_unit) => {}
                ["village", x, y, side]
                    if valid_side(side) && next_position(x, y, &mut last_village) => {}
                _ => {
                    return Err(bad_row(line, "expected sorted unit/village rows or matching end"));
                }
            }
            rows.push(row);
        }
        frames.push(rows);
    }
    if frames.is_empty() {
        return Err("missing step 0".into());
    }
    Ok(frames)
}

fn valid_side(side: &str) -> bool {
    matches!(side, "1" | "2")
}

fn next_position(x: &str, y: &str, last: &mut Option<(i32, i32)>) -> bool {
    let (Ok(x), Ok(y)) = (x.parse::<i32>(), y.parse::<i32>()) else {
        return false;
    };
    let position = (x, y);
    if x <= 0 || y <= 0 || last.is_some_and(|previous| previous >= position) {
        return false;
    }
    *last = Some(position);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    const INITIAL: &str = "step\t0\tplaying\t1\t1\ngold\t1\t100\ngold\t2\t100\nend\t0\n";
    const AFTER_MOVE: &str = "step\t1\tplaying\t1\t1\ngold\t1\t100\ngold\t2\t100\n\
        unit\t2\t1\t1\tFootman\t12\t3\t1\t1\nend\t1\n";

    #[test]
    fn reports_first_unequal_row_at_its_step() {
        let native = format!("{INITIAL}{AFTER_MOVE}");
        let rust = native.replace("Footman\t12\t3", "Footman\t11\t3");
        assert_eq!(compare(&native, &native), Ok(2));
        assert_eq!(
            compare(&rust, &native).unwrap_err(),
            "step 1 differs\n  expected native: unit\t2\t1\t1\tFootman\t12\t3\t1\t1\n  \
             actual Rust:     unit\t2\t1\t1\tFootman\t11\t3\t1\t1"
        );
    }

    #[test]
    fn reports_missing_steps_or_rows() {
        let native = format!("{INITIAL}{AFTER_MOVE}");
        assert_eq!(
            compare(INITIAL, &native).unwrap_err(),
            "step 1 differs\n  expected native: step\t1\tplaying\t1\t1\n  actual Rust:     <missing>"
        );
        let rust = native.replace("unit\t2\t1\t1\tFootman\t12\t3\t1\t1\n", "");
        assert_eq!(
            compare(&rust, &native).unwrap_err(),
            "step 1 differs\n  expected native: unit\t2\t1\t1\tFootman\t12\t3\t1\t1\n  \
             actual Rust:     <missing>"
        );
    }

    #[test]
    fn rejects_empty_truncated_and_malformed_captures_even_when_identical() {
        assert_eq!(
            compare("", "").unwrap_err(),
            "malformed Rust trace: missing step 0"
        );
        let truncated = format!("{INITIAL}{}", AFTER_MOVE.trim_end_matches("end\t1\n"));
        assert_eq!(
            compare(&truncated, &truncated).unwrap_err(),
            "malformed Rust trace: step 1: missing end row"
        );
        let malformed = INITIAL.replace("gold\t2\t100\n", "");
        assert_eq!(
            compare(INITIAL, &malformed).unwrap_err(),
            "malformed native trace: step 0, line 3: expected gold rows in side order"
        );
    }
}
