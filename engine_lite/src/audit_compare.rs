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
    let mut map_width = None;
    let mut map_height = None;
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

        let mut last_reserve = (0, 0);
        while lines.peek().is_some_and(|(_, row)| row.starts_with("reserve\t")) {
            let (line, row) = lines.next().expect("peeked reserve");
            let fields: Vec<_> = row.split('\t').collect();
            let ["reserve", side, slot, kind, hp, xp, cost, traits @ ..] = fields.as_slice() else {
                return Err(bad_row(line, "expected reserve side, slot, type, hitpoints, experience and cost"));
            };
            let (Some(side), Some(slot)) = (side_number(side), side_number(slot)) else {
                return Err(bad_row(line, "invalid reserve side or slot"));
            };
            if side > side_count || !(side == last_reserve.0 && slot.checked_sub(1) == Some(last_reserve.1)
                || side > last_reserve.0 && slot == 1)
                || kind.is_empty() || kind.contains('\r') || hp.parse::<i32>().is_err()
                || xp.parse::<i32>().is_err() || cost.parse::<i32>().is_err() || traits.iter().any(|id| id.contains('\r'))
            {
                return Err(bad_row(line, "invalid reserve or out-of-order roster slot"));
            }
            last_reserve = (side, slot);
            rows.push(row);
        }
        let mut last_unit = None;
        let mut unit_positions = std::collections::BTreeSet::new();
        let mut status_owner = None;
        let mut last_village = None;
        let mut view_side = 0;
        let mut view_height = 0;
        let mut listed_units = false;
        loop {
            let Some((line, row)) = lines.next() else {
                return Err(format!("step {step}: missing end row"));
            };
            let fields: Vec<_> = row.split('\t').collect();
            match fields.as_slice() {
                ["end", index] if index.parse::<usize>() == Ok(step) => {
                    if view_height > 0 && view_height != *map_height.get_or_insert(view_height) {
                        return Err(bad_row(line, "inconsistent view height"));
                    }
                    break;
                }
                ["view", side, y, tiles] => {
                    let side = side_number(side).filter(|&s| s >= view_side && s <= side_count)
                        .ok_or_else(|| bad_row(line, "expected view rows in roster order"))?;
                    if side != view_side {
                        if view_height > 0 && view_height != *map_height.get_or_insert(view_height) {
                            return Err(bad_row(line, "inconsistent view height"));
                        }
                        view_side = side;
                        view_height = 0;
                        listed_units = false;
                    }
                    if listed_units {
                        return Err(bad_row(line, "expected the next side after seen units"));
                    }
                    view_height += 1;
                    if side_number(y) != Some(view_height) {
                        return Err(bad_row(line, "expected consecutive view rows starting at 1"));
                    }
                    if tiles.is_empty() || !tiles.bytes().all(|c| matches!(c, b'V' | b'F' | b'S')) {
                        return Err(bad_row(line, "expected a nonempty view row of V, F or S"));
                    }
                    if tiles.len() != *map_width.get_or_insert(tiles.len()) {
                        return Err(bad_row(line, "inconsistent view width"));
                    }
                }
                ["seen", side, positions @ ..] if view_side > 0 && !listed_units
                    && side_number(side) == Some(view_side) => {
                    let mut previous = None;
                    for position in positions {
                        let valid = position.split_once(',').is_some_and(|(x, y)| {
                            next_position(x, y, &mut previous) && previous.is_some_and(|hex|
                                hex.0 as usize <= map_width.unwrap_or(0)
                                && hex.1 as usize <= view_height
                                && unit_positions.contains(&hex)
                            )
                        });
                        if !valid {
                            return Err(bad_row(line, "expected sorted on-map unit positions in seen row"));
                        }
                    }
                    listed_units = true;
                }
                _ if view_side != 0 => return Err(bad_row(line, "expected view, seen units or matching end")),
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
                        unit_positions.insert(last_unit.expect("validated unit position"));
                    }
                ["status", x, y, statuses @ ..] if last_village.is_none()
                    && valid_statuses(statuses) && status_owner.take() == Some((*x, *y)) => {}
                ["village", x, y, side]
                    if valid_side(side) && next_position(x, y, &mut last_village) => {}
                _ => {
                    return Err(bad_row(line, "expected sorted units, their status, villages, views or matching end"));
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

fn valid_statuses(statuses: &[&str]) -> bool {
    let mut order = ["slowed", "poisoned", "petrified"].into_iter();
    !statuses.is_empty() && statuses.iter().all(|status| order.any(|name| name == *status))
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
    fn compares_tile_states_at_the_side_and_row_where_they_differ() {
        let legacy = format!("{INITIAL}{AFTER_MOVE}").replace("end\t1", "village\t2\t1\t2\nend\t1");
        let native = legacy.replace("end\t1", "view\t2\t1\tVFS\nview\t2\t2\tSSV\nend\t1");
        assert_eq!(compare(&native, &native), Ok(2));
        assert!(compare(&legacy, &native).unwrap_err().starts_with("step 1 differs"));
        let rust = native.replace("SSV", "SFV");
        assert_eq!(
            compare(&rust, &native).unwrap_err(),
            "step 1 differs\n  expected native: view\t2\t2\tSSV\n  actual Rust:     view\t2\t2\tSFV",
        );
    }

    #[test]
    fn distinguishes_hidden_units_on_identical_maps_and_reads_older_views() {
        let units = "unit\t1\t2\t1\tScout\t9\t2\t1\t0\nunit\t2\t1\t2\tHider\t8\t3\t1\t0\n";
        let view = "view\t1\t1\tVV\nview\t1\t2\tVV\n";
        let legacy = INITIAL.replace("end\t0", &format!("{units}{view}end\t0"));
        let native = legacy.replace("end\t0", "seen\t1\t1,2\nend\t0");
        let leak = native.replace("seen\t1\t1,2", "seen\t1\t1,2\t2,1");
        assert_eq!(compare(&legacy, &legacy), Ok(1));
        assert_eq!(compare(&native, &native), Ok(1));
        assert!(compare(&native, &legacy).unwrap_err().starts_with("step 0 differs"));
        assert_eq!(compare(&leak, &native).unwrap_err(),
            "step 0 differs\n  expected native: seen\t1\t1,2\n  actual Rust:     seen\t1\t1,2\t2,1");
        let empty = native.replace("seen\t1\t1,2", "seen\t1");
        assert_eq!(compare(&empty, &empty), Ok(1));
        for invalid in ["seen\t2", "seen\t1\t1,1", "seen\t1\t0,2", "seen\t1\t1,3",
            "seen\t1\t2,1\t1,2", "seen\t1\t1,2\t1,2", "seen\t1\nseen\t1",
            "seen\t1\nview\t1\t3\tVV"] {
            let malformed = native.replace("seen\t1\t1,2", invalid);
            assert!(compare(&malformed, &malformed).unwrap_err().starts_with("malformed Rust trace"));
        }
    }

    #[test]
    fn rejects_incomplete_unordered_or_malformed_view_rows() {
        let views = "view\t1\t1\tVF\nview\t1\t2\tVS\nview\t2\t1\tFS\nview\t2\t2\tSF\n";
        let full = INITIAL.replace("end\t0", &format!("{views}end\t0"));
        assert_eq!(compare(&full, &full), Ok(1));
        for (row, replacement) in [
            ("view\t1\t1\tVF\n", ""),
            ("view\t2\t2\tSF\n", ""),
            ("view\t2\t1", "view\t3\t1"),
            ("view\t1\t1", "view\t0\t1"),
            ("view\t2\t2", "view\t1\t2"),
            ("view\t2\t2", "view\t2\t1"),
            ("view\t2\t1\tFS", "view\t2\t1\tF"),
            ("view\t1\t1\tVF", "view\t1\t1\t"),
            ("view\t1\t1\tVF", "view\t1\t1\tVX"),
            ("end\t0", "village\t2\t1\t2\nend\t0"),
        ] {
            let invalid = full.replace(row, replacement);
            assert!(compare(&invalid, &invalid).unwrap_err().starts_with("malformed Rust trace"), "{invalid}");
        }
        for different_map in ["view\t2\t1\tVF\n", "view\t2\t1\tV\nview\t2\t2\tF\n"] {
            let next = AFTER_MOVE.replace("end\t1", &format!("{different_map}end\t1"));
            let invalid = format!("{full}{next}");
            assert!(compare(&invalid, &invalid).unwrap_err().starts_with("malformed Rust trace: step 1"));
        }
    }

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
        for statuses in ["petrified", "poisoned\tpetrified", "slowed\tpetrified", "slowed\tpoisoned\tpetrified"] {
            let trace = with_status(statuses);
            assert_eq!(compare(&trace, &trace), Ok(2));
            assert!(compare(THREE_SIDES, &trace).unwrap_err().contains("petrified"));
        }
        for invalid in ["petrified\tslowed", "petrified\tpetrified", "poisoned\tslowed", "poisoned\tpoisoned", "slowed\tpoisoned\tpoisoned"] {
            let trace = with_status(invalid);
            assert!(compare(&trace, &trace).unwrap_err().starts_with("malformed Rust trace"));
        }
    }

    #[test]
    fn reserve_frames_detect_a_missing_veteran_and_preserve_recall_order() {
        let reserve = "reserve\t2\t1\tSwordsman\t33\t15\t13\tloyal\n";
        let frame = THREE_SIDES.replacen("unit\t", &format!("{reserve}unit\t"), 1);
        assert_eq!(compare(&frame, &frame), Ok(2));
        assert!(compare(THREE_SIDES, &frame).unwrap_err().contains("Swordsman"));
        for malformed in [frame.replace("reserve\t2\t1", "reserve\t2\t2"), frame.replace(reserve, &reserve.repeat(2))] {
            assert!(compare(&malformed, &malformed).unwrap_err().starts_with("malformed Rust trace"));
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
