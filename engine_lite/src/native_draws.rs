//! Read the native referee's per-attack random draws from its audit log.

use std::collections::BTreeMap;

const ATTACK_MARKER: &str = "RUSTNOTH_AUDIT_ATTACK_";
const RANDOM_DRAW: &str = "pulled user random";

#[derive(Debug, Default, Eq, PartialEq)]
pub struct AttackDraws {
    /// Seed and number of draws consumed before the attack, if it drew any.
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
            let (word, seed, calls_before) =
                parse_draw(draw).ok_or_else(|| error("malformed random draw"))?;
            let (previous_seed, start) = *draws.checkpoint.get_or_insert((seed, calls_before));
            if seed != previous_seed
                || u64::from(calls_before) != u64::from(start) + draws.words.len() as u64
            {
                return Err(error("random draws changed seed or skipped a call"));
            }
            draws.words.push(word);
        }
    }

    if let Some((step, _)) = active {
        return Err(format!("native RNG log is missing the end of attack step {step}"));
    }
    Ok(attacks)
}

fn parse_marker(marker: &str) -> Option<(&str, usize)> {
    let (kind, step) = marker.split_once(' ')?;
    let step = step.parse().ok()?;
    (step > 0).then_some((kind, step))
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
}
