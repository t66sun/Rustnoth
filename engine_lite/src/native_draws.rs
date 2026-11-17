//! Read the native referee's random draws from its audit log.

use std::collections::BTreeMap;

const ATTACK_MARKER: &str = "RUSTNOTH_AUDIT_ATTACK_";
const RECRUIT_MARKER: &str = "RUSTNOTH_AUDIT_RECRUIT_";
const RANDOM_DRAW: &str = "pulled user random";

#[derive(Debug, Default, Eq, PartialEq)]
pub struct AttackDraws {
    /// Seed and number of draws consumed before the first recorded draw.
    pub checkpoint: Option<(u32, u32)>,
    pub words: Vec<u32>,
}

/// Read completed attack windows keyed by one-based action step.
pub fn parse(log: &str) -> Result<BTreeMap<usize, AttackDraws>, String> {
    let mut attacks = BTreeMap::new();
    let mut active: Option<(usize, AttackDraws)> = None;

    for (index, line) in log.lines().enumerate() {
        let Some((category, message)) = line.split_once(": ") else {
            continue;
        };
        let error = |message| format!("native RNG log line {}: {message}", index + 1);

        if (category == "info wml" || category.ends_with(" info wml"))
            && let Some(marker) = message.strip_prefix(ATTACK_MARKER)
        {
            let (kind, step) = parse_marker(marker).ok_or_else(|| error("malformed attack marker"))?;
            match kind {
                "BEGIN" => {
                    if active.is_some() {
                        return Err(error("nested attack beginning"));
                    }
                    if attacks.last_key_value().is_some_and(|(&last_step, _)| step <= last_step) {
                        return Err(error("attack steps must increase"));
                    }
                    active = Some((step, AttackDraws::default()));
                }
                "END" => {
                    let Some((begin, draws)) = active.take() else {
                        return Err(error("attack end without a beginning"));
                    };
                    if step != begin {
                        return Err(error("attack end does not match its beginning"));
                    }
                    attacks.insert(step, draws);
                }
                _ => return Err(error("malformed attack marker")),
            }
        } else if let Some((_, draws)) = &mut active
            && (category == "debug random" || category.ends_with(" debug random"))
            && let Some(draw) = message.strip_prefix(RANDOM_DRAW)
        {
            append_draw(draws, draw).map_err(error)?;
        }
    }

    if let Some((step, _)) = active {
        return Err(format!("native RNG log is missing the end of attack step {step}"));
    }
    Ok(attacks)
}

struct PendingRecruit<'a> {
    step: usize,
    selecting_traits: bool,
    draws: Vec<(usize, &'a str)>,
}

/// Read the trait draws of completed recruits, keyed by one-based action step.
pub fn parse_recruits(log: &str) -> Result<BTreeMap<usize, AttackDraws>, String> {
    let mut recruits = BTreeMap::new();
    let mut active: Option<PendingRecruit<'_>> = None;

    for (index, line) in log.lines().enumerate() {
        let Some((category, message)) = line.split_once(": ") else {
            continue;
        };
        let line_number = index + 1;
        let error = |message| format!("native RNG log line {line_number}: {message}");

        if (category == "info wml" || category.ends_with(" info wml"))
            && let Some(marker) = message.strip_prefix(RECRUIT_MARKER)
        {
            match marker.split_once(' ') {
                Some(("BEGIN", step)) => {
                    let step =
                        parse_step(step).ok_or_else(|| error("malformed recruit beginning"))?;
                    if active.is_some() {
                        return Err(error("nested recruit beginning"));
                    }
                    if recruits.last_key_value().is_some_and(|(&last_step, _)| step <= last_step) {
                        return Err(error("recruit steps must increase"));
                    }
                    active = Some(PendingRecruit {
                        step,
                        selecting_traits: false,
                        draws: Vec::new(),
                    });
                }
                Some(("END", fields)) => {
                    let (step, count) = fields
                        .split_once(' ')
                        .and_then(|(step, count)| {
                            Some((parse_step(step)?, count.parse::<usize>().ok()?))
                        })
                        .ok_or_else(|| error("malformed recruit end"))?;
                    let pending =
                        active.take().ok_or_else(|| error("recruit end without a beginning"))?;
                    if step != pending.step {
                        return Err(error("recruit end does not match its beginning"));
                    }
                    let prefix = pending.draws.get(..count).ok_or_else(|| {
                        format!(
                            "native RNG log line {line_number}: recruit step {step} reports {count} trait draws but only {} follow the trait selection marker",
                            pending.draws.len()
                        )
                    })?;
                    let mut draws = AttackDraws::default();
                    for &(line, draw) in prefix {
                        append_draw(&mut draws, draw)
                            .map_err(|cause| format!("native RNG log line {line}: {cause}"))?;
                    }
                    recruits.insert(step, draws);
                }
                _ => return Err(error("malformed recruit marker")),
            }
        } else if let Some(recruit) = &mut active {
            if (category == "info unit" || category.ends_with(" info unit"))
                && message.starts_with("Generating a trait for unit type ")
                && message.ends_with(" with must_have_only 0")
            {
                recruit.selecting_traits = true;
            } else if recruit.selecting_traits
                && (category == "debug random" || category.ends_with(" debug random"))
                && let Some(draw) = message.strip_prefix(RANDOM_DRAW)
            {
                // Name generation follows trait selection in the same recruit window.
                // The end marker gives the length of the trait prefix to validate.
                recruit.draws.push((line_number, draw));
            }
        }
    }

    if let Some(recruit) = active {
        return Err(format!(
            "native RNG log is missing the end of recruit step {}",
            recruit.step
        ));
    }
    Ok(recruits)
}

fn parse_marker(marker: &str) -> Option<(&str, usize)> {
    let (kind, step) = marker.split_once(' ')?;
    Some((kind, parse_step(step)?))
}

fn parse_step(step: &str) -> Option<usize> {
    let step = step.parse().ok()?;
    (step > 0).then_some(step)
}

fn append_draw(draws: &mut AttackDraws, draw: &str) -> Result<(), &'static str> {
    let (word, seed, calls_before) = parse_draw(draw).ok_or("malformed random draw")?;
    let (previous_seed, start) = *draws.checkpoint.get_or_insert((seed, calls_before));
    if seed != previous_seed
        || u64::from(calls_before) != u64::from(start) + draws.words.len() as u64
    {
        return Err("random draws changed seed or skipped a call");
    }
    draws.words.push(word);
    Ok(())
}

fn parse_draw(draw: &str) -> Option<(u32, u32, u32)> {
    let (word, tail) = draw.strip_prefix(' ')?.split_once(" for call ")?;
    let (call, seed) = tail.split_once(" with seed ")?;
    let calls_before = call.parse::<u32>().ok()?.checked_sub(1)?;
    Some((
        word.parse().ok()?,
        u32::from_str_radix(seed, 16).ok()?,
        calls_before,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const BEGIN: &str = "info wml: RUSTNOTH_AUDIT_ATTACK_BEGIN 5";
    const END: &str = "info wml: RUSTNOTH_AUDIT_ATTACK_END 5";
    const DRAW: &str = "debug random: pulled user random 800589114 for call 8 with seed 3d204a68";

    #[test]
    fn reads_only_windowed_draws_with_their_initial_offset() {
        let log = concat!(
            "20261118 00:34:43 debug random: pulled user random 3499211612 for call 1 with seed 1571\r\n",
            "20261118 00:34:45 info wml: RUSTNOTH_AUDIT_ATTACK_BEGIN 3\r\n",
            "20261118 00:34:45 info wml: RUSTNOTH_AUDIT_ATTACK_END 3\r\n",
            "20261118 00:34:45 info wml: RUSTNOTH_AUDIT_ATTACK_BEGIN 5\r\n",
            "20261118 00:34:45 debug random: pulled user random 800589114 for call 8 with seed 3d204a68\r\n",
            "20261118 00:34:45 info random: randomness::rng::next_random_impl returned 800589114\r\n",
            "20261118 00:34:45 debug random: pulled user random 3120994320 for call 9 with seed 3d204a68\r\n",
            "20261118 00:34:45 info wml: RUSTNOTH_AUDIT_ATTACK_END 5\r\n",
        );
        let expected = BTreeMap::from([
            (3, AttackDraws::default()),
            (
                5,
                AttackDraws {
                    checkpoint: Some((0x3d20_4a68, 7)),
                    words: vec![800_589_114, 3_120_994_320],
                },
            ),
        ]);
        assert_eq!(parse(log), Ok(expected));
    }

    #[test]
    fn rejects_broken_or_out_of_order_windows() {
        for log in [
            END.to_owned(),
            BEGIN.to_owned(),
            format!("{BEGIN}\n{BEGIN}\n{END}"),
            format!("{BEGIN}\n{}", END.replace("5", "4")),
            format!("{BEGIN}\n{END}\n{BEGIN}\n{END}"),
            format!("{BEGIN}\n{END}\n{}\n{}", BEGIN.replace("5", "4"), END.replace("5", "4")),
            format!("{}\n{}", BEGIN.replace("5", "0"), END.replace("5", "0")),
            format!("{BEGIN} extra\n{END}"),
        ] {
            assert!(parse(&log).is_err(), "{log}");
        }
    }

    #[test]
    fn rejects_malformed_draws_and_changes_in_the_stream() {
        for draw in [
            DRAW,
            "debug random: pulled user random 1 for call 10 with seed 3d204a68",
            "debug random: pulled user random 1 for call 9 with seed 1571",
            "debug random: pulled user random 1 for call 0 with seed 3d204a68",
            "debug random: pulled user random 1 for call 9 with seed gg",
            "debug random: pulled user random",
        ] {
            let log = format!("{BEGIN}\n{DRAW}\n{draw}\n{END}");
            assert!(parse(&log).is_err(), "{log}");
        }
        assert_eq!(parse(DRAW), Ok(BTreeMap::new()));
    }

    const RECRUIT_BEGIN: &str = "info wml: RUSTNOTH_AUDIT_RECRUIT_BEGIN 1";
    const RECRUIT_END: &str = "info wml: RUSTNOTH_AUDIT_RECRUIT_END 1 2";
    const SELECT_TRAITS: &str =
        "info unit: Generating a trait for unit type Bowman with must_have_only 0";

    #[test]
    fn recruit_draws_begin_with_optional_selection_and_end_before_name_generation() {
        let log = format!(
            "{DRAW}\r\n\
             20261118 00:53:17 {RECRUIT_BEGIN}\r\n\
             info unit: Generating a trait for unit type Bowman with must_have_only 1\r\n\
             {DRAW}\r\n\
             20261118 00:53:17 {SELECT_TRAITS}\r\n\
             20261118 00:53:17 {DRAW}\r\n\
             debug random: pulled user random 17 for call 9 with seed 3d204a68\r\n\
             debug random: pulled user random 21 for call 10 with seed 3d204a68\r\n\
             20261118 00:53:17 {RECRUIT_END}\r\n\
             {BEGIN}\r\n{DRAW}\r\n{END}\r\n\
             info wml: RUSTNOTH_AUDIT_RECRUIT_BEGIN 7\r\n\
             {SELECT_TRAITS}\r\n{DRAW}\r\n\
             info wml: RUSTNOTH_AUDIT_RECRUIT_END 7 0\r\n"
        );
        let expected = BTreeMap::from([
            (
                1,
                AttackDraws {
                    checkpoint: Some((0x3d20_4a68, 7)),
                    words: vec![800_589_114, 17],
                },
            ),
            (7, AttackDraws::default()),
        ]);
        assert_eq!(parse_recruits(&log), Ok(expected));
    }

    #[test]
    fn rejects_malformed_or_incomplete_recruit_windows() {
        let empty_recruit = format!("{RECRUIT_BEGIN}\ninfo wml: RUSTNOTH_AUDIT_RECRUIT_END 1 0");
        for log in [
            RECRUIT_END.to_owned(),
            RECRUIT_BEGIN.to_owned(),
            format!("{RECRUIT_BEGIN}\n{RECRUIT_BEGIN}"),
            format!("{empty_recruit}\n{empty_recruit}"),
            format!("{RECRUIT_BEGIN}\ninfo wml: RUSTNOTH_AUDIT_RECRUIT_END 2 0"),
            format!("{RECRUIT_BEGIN}\ninfo wml: RUSTNOTH_AUDIT_RECRUIT_END 1"),
            format!("{RECRUIT_BEGIN}\n{DRAW}\n{DRAW}\n{RECRUIT_END}"),
            format!("{RECRUIT_BEGIN}\n{SELECT_TRAITS}\n{DRAW}\n{RECRUIT_END}"),
        ] {
            assert!(parse_recruits(&log).is_err(), "{log}");
        }
    }

    #[test]
    fn validates_the_recruit_trait_prefix_at_its_original_lines() {
        for (draw, cause) in [
            ("debug random: pulled user random bad", "malformed random draw"),
            (
                "debug random: pulled user random 17 for call 9 with seed 42",
                "random draws changed seed or skipped a call",
            ),
            (
                "debug random: pulled user random 17 for call 10 with seed 3d204a68",
                "random draws changed seed or skipped a call",
            ),
        ] {
            let log = format!("{RECRUIT_BEGIN}\n{SELECT_TRAITS}\n{DRAW}\n{draw}\n{RECRUIT_END}");
            assert_eq!(
                parse_recruits(&log).unwrap_err(),
                format!("native RNG log line 4: {cause}")
            );
        }
    }
}
