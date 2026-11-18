/// Compares validated traces and returns the number of matching frames, including step zero.
pub fn compare(rust: &str, native: &str) -> Result<usize, String> {
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
    let mut lines = text.lines().enumerate().peekable();
    let mut frames = Vec::new();
    let mut roster_size = None;
    while let Some((line, header)) = lines.next() {
        let step = frames.len();
        let bad_row = |line, message| format!("step {step}, line {}: {message}", line + 1);
        let bad_header = || bad_row(line, "expected a step header with the next index");
        let fields: Vec<_> = header.split('\t').collect();
        let highest_header_side = match fields.as_slice() {
            ["step", index, "playing", round, side]
                if index.parse::<usize>() == Ok(step) && round.parse::<i32>().is_ok() =>
            {
                Some(side_number(side).ok_or_else(&bad_header)?)
            }
            ["step", index, "won", winners @ ..]
                if index.parse::<usize>() == Ok(step) && !winners.is_empty() =>
            {
                let mut last = 0;
                for winner in winners {
                    last = side_number(winner).filter(|&side| side > last).ok_or_else(&bad_header)?;
                }
                Some(last)
            }
            ["step", index, "draw"] if index.parse::<usize>() == Ok(step) => None,
            _ => return Err(bad_header()),
        };

        let mut rows = vec![header];
        let mut side_count = 0;
        while side_count < roster_size.unwrap_or(2)
            || (roster_size.is_none()
                && lines.peek().is_some_and(|(_, row)| row.starts_with("gold\t")))
        {
            side_count += 1;
            let Some((line, row)) = lines.next() else {
                return Err(format!("step {step}: missing gold row for side {side_count}"));
            };
            let fields: Vec<_> = row.split('\t').collect();
            if !matches!(fields.as_slice(), ["gold", owner, amount]
                if owner.parse::<usize>() == Ok(side_count) && amount.parse::<i64>().is_ok())
            {
                return Err(bad_row(line, "expected gold rows in side order"));
            }
            rows.push(row);
        }
        roster_size.get_or_insert(side_count);
        if highest_header_side.is_some_and(|side| side > side_count) {
            return Err(bad_header());
        }
        let valid_side = |side: &str| side_number(side).is_some_and(|number| number <= side_count);

        let mut last_unit = None;
        let mut status_owner = None;
        let mut last_village = None;
        loop {
            let Some((line, row)) = lines.next() else {
                return Err(format!("step {step}: missing end row"));
            };
            let fields: Vec<_> = row.split('\t').collect();
            match fields.as_slice() {
                ["end", index] if index.parse::<usize>() == Ok(step) => break,
                ["unit", x, y, side, kind, hp, mp, attacks, "0" | "1", extra @ ..]
                    if last_village.is_none()
                        && valid_side(side)
                        && !kind.is_empty()
                        && !kind.contains('\r')
                        && hp.parse::<i32>().is_ok()
                        && mp.parse::<i32>().is_ok()
                        && attacks.parse::<i32>().is_ok()
                        && valid_unit_details(extra)
                        && next_position(x, y, &mut last_unit) => {
                        status_owner = Some((*x, *y));
                    }
                ["status", x, y, "slowed" | "poisoned"] | ["status", x, y, "slowed", "poisoned"]
                    if last_village.is_none() && status_owner.take() == Some((*x, *y)) => {}
                ["village", x, y, side]
                    if valid_side(side) && next_position(x, y, &mut last_village) => {}
                _ => {
                    return Err(bad_row(line, "expected sorted units, their status, villages or matching end"));
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

fn side_number(side: &str) -> Option<usize> {
    if !matches!(side.as_bytes().first(), Some(b'1'..=b'9')) {
        return None;
    }
    side.parse().ok()
}

fn valid_unit_details(fields: &[&str]) -> bool {
    fields.split_first().is_none_or(|(experience, traits)| {
        experience.parse::<i32>().is_ok() && traits.iter().all(|id| !id.contains('\r'))
    })
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
    const THREE_SIDES: &str = concat!(
        "step\t0\tplaying\t1\t3\ngold\t1\t34\ngold\t2\t38\ngold\t3\t42\n",
        "unit\t2\t1\t3\tBowman\t33\t5\t1\t1\t0\nvillage\t2\t1\t3\nend\t0\n",
        "step\t1\twon\t3\ngold\t1\t34\ngold\t2\t38\ngold\t3\t42\nend\t1\n",
    );

    #[test]
    fn compares_all_three_sides() {
        assert_eq!(compare(THREE_SIDES, THREE_SIDES), Ok(2));
        let rust = THREE_SIDES.replacen("gold\t3\t42", "gold\t3\t41", 1);
        assert_eq!(
            compare(&rust, THREE_SIDES).unwrap_err(),
            "step 0 differs\n  expected native: gold\t3\t42\n  actual Rust:     gold\t3\t41",
        );
    }

    #[test]
    fn compares_all_winning_sides_in_roster_order() {
        let joint = THREE_SIDES.replace("won\t3", "won\t1\t3");
        assert_eq!(compare(&joint, &joint), Ok(2));
        assert!(compare(THREE_SIDES, &joint).unwrap_err().starts_with("step 1 differs"));
        for header in ["won", "won\t3\t1", "won\t1\t1", "won\t1\t4", "won\t0\t3"] {
            let malformed = THREE_SIDES.replace("won\t3", header);
            assert!(compare(&malformed, &malformed).unwrap_err().starts_with("malformed Rust trace"));
        }
    }

    #[test]
    fn rejects_unlisted_sides_and_inconsistent_gold_rows() {
        for (old, new) in [
            ("playing\t1\t3", "playing\t1\t4"),
            ("won\t3", "won\t4"),
            ("\t3\tBowman", "\t4\tBowman"),
            ("village\t2\t1\t3", "village\t2\t1\t4"),
            ("gold\t2\t38\ngold\t3\t42", "gold\t3\t42\ngold\t2\t38"),
            ("gold\t3\t42\nend\t1", "end\t1"),
            ("gold\t3\t42\nend\t1", "gold\t3\t42\ngold\t4\t43\nend\t1"),
        ] {
            let malformed = THREE_SIDES.replacen(old, new, 1);
            assert!(
                compare(&malformed, THREE_SIDES).unwrap_err().starts_with("malformed Rust trace"),
                "{malformed}",
            );
        }
    }

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
    fn compares_numeric_unit_experience_and_keeps_legacy_traces_readable() {
        let legacy = format!("{INITIAL}{AFTER_MOVE}");
        assert_eq!(compare(&legacy, &legacy), Ok(2));

        let native = legacy.replace("Footman\t12\t3\t1\t1", "Footman\t12\t3\t1\t1\t8");
        assert_eq!(compare(&native, &native), Ok(2));
        let rust = native.replace("\t1\t8\n", "\t1\t1\n");
        assert!(compare(&rust, &native).unwrap_err().starts_with("step 1 differs"));
        let malformed = native.replace("\t1\t8\n", "\t1\tunknown\n");
        assert!(compare(&malformed, &malformed).unwrap_err().starts_with("malformed Rust trace"));
    }

    #[test]
    fn compares_slowed_only_as_a_status_of_the_preceding_unit() {
        let row = "status\t2\t1\tslowed\n";
        let tagged = THREE_SIDES.replacen("village\t", &format!("{row}village\t"), 1);
        assert_eq!(compare(&tagged, &tagged), Ok(2));
        assert!(compare(THREE_SIDES, &tagged).unwrap_err().starts_with("step 0 differs"));
        let trait_only = THREE_SIDES.replace("\t1\t0\nvillage", "\t1\t0\tslowed\nvillage");
        assert!(compare(&trait_only, &tagged).unwrap_err().starts_with("step 0 differs"));

        for malformed in [
            tagged.replace(row, &format!("{row}{row}")),
            tagged.replace(row, "status\t1\t1\tslowed\n"),
            tagged.replace(row, "status\t2\t1\tunknown\n"),
            THREE_SIDES.replacen("unit\t", &format!("{row}unit\t"), 1),
            THREE_SIDES.replacen("end\t0", &format!("{row}end\t0"), 1),
        ] {
            assert!(compare(&malformed, &malformed).unwrap_err().starts_with("malformed Rust trace"));
        }
    }

    #[test]
    fn compares_poison_on_its_own_and_alongside_slow() {
        let with_status = |status| THREE_SIDES.replacen(
            "village\t", &format!("status\t2\t1\t{status}\nvillage\t"), 1,
        );
        let poisoned = with_status("poisoned");
        let slowed = with_status("slowed");
        let both = with_status("slowed\tpoisoned");
        assert_eq!(compare(&poisoned, &poisoned), Ok(2));
        assert_eq!(compare(&both, &both), Ok(2));
        for (rust, native) in [(THREE_SIDES, &poisoned), (slowed.as_str(), &both)] {
            let difference = compare(rust, native).unwrap_err();
            assert!(difference.starts_with("step 0 differs"));
            assert!(difference.contains("poisoned"));
        }
        for invalid in ["poisoned\tslowed", "poisoned\tpoisoned", "slowed\tpoisoned\tpoisoned"] {
            let trace = with_status(invalid);
            assert!(compare(&trace, &trace).unwrap_err().starts_with("malformed Rust trace"));
        }
    }

    #[test]
    fn compares_trait_ids_and_their_effect_order() {
        let legacy = format!("{INITIAL}{AFTER_MOVE}");
        let native = legacy.replace("Footman\t12\t3\t1\t1", "Footman\t12\t3\t1\t1\t8\tquick\tstrong");
        assert_eq!(compare(&native, &native), Ok(2));
        let rust = native.replace("quick\tstrong", "strong\tquick");
        assert!(compare(&rust, &native).unwrap_err().starts_with("step 1 differs"));
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
