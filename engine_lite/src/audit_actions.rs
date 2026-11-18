//! Scripted actions for replay and differential audit.

use std::{error::Error, fmt, str::FromStr};

use crate::{board::AttackChoice, fixed_match::Action, hex::Hex};

#[derive(Debug, Eq, PartialEq)]
pub struct ParseError {
    pub line: usize,
    reason: &'static str,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.reason)
    }
}

impl Error for ParseError {}

/// Reads one tab-separated action per nonblank line.
///
/// A line beginning with `#` is a comment. Coordinates are one-based WML
/// integers and the attacking weapon index is zero-based.
pub fn parse(input: &str) -> Result<Vec<Action>, ParseError> {
    let mut actions = Vec::new();
    for (index, line) in input.lines().enumerate() {
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let action = parse_action(line).map_err(|reason| ParseError {
            line: index + 1,
            reason,
        })?;
        actions.push(action);
    }
    Ok(actions)
}

/// Extracts the original referee's consecutively numbered commands as a replay script.
pub fn explored_script(native_log: &str, max_commands: usize) -> Result<String, ParseError> {
    let mut script = String::new();
    let mut count = 0;
    for (index, line) in native_log.lines().enumerate() {
        let Some((category, message)) = line.split_once(": ") else { continue };
        if category != "info wml" && !category.ends_with(" info wml") {
            continue;
        }
        let (tag, fields) = message.split_at(message.find(char::is_whitespace).unwrap_or(message.len()));
        if tag != "RUSTNOTH_AUDIT_COMMAND" {
            continue;
        }
        let error = |reason| ParseError { line: index + 1, reason };
        let (step, command) = fields.strip_prefix(' ').and_then(|rest| rest.split_once('\t'))
            .ok_or_else(|| error("malformed exploration command marker"))?;
        if decimal::<usize>(step) != Some(count + 1) {
            return Err(error("exploration command steps must be consecutive starting at 1"));
        }
        if count == max_commands {
            return Err(error("exploration command limit exceeded"));
        }
        parse_action(command).map_err(error)?;
        script.push_str(command);
        script.push('\n');
        count += 1;
    }
    Ok(script)
}

fn parse_action(line: &str) -> Result<Action, &'static str> {
    let fields: Vec<_> = line.split('\t').collect();
    match fields.as_slice() {
        ["move", x, y, to_x, to_y] => Ok(Action::Move {
            from: parse_hex(x, y)?,
            to: parse_hex(to_x, to_y)?,
        }),
        ["attack", x, y, to_x, to_y, weapon] => Ok(Action::Attack(AttackChoice {
            from: parse_hex(x, y)?,
            to: parse_hex(to_x, to_y)?,
            weapon: decimal(weapon).ok_or("invalid weapon index")?,
        })),
        ["recruit", x, y, unit_type] => {
            if unit_type.trim().is_empty() {
                return Err("missing recruit type ID");
            }
            Ok(Action::Recruit {
                unit_type: (*unit_type).to_owned(),
                to: parse_hex(x, y)?,
            })
        }
        ["recall", x, y, unit_id] => {
            if unit_id.trim().is_empty() {
                return Err("missing recall unit ID");
            }
            Ok(Action::Recall { unit_id: (*unit_id).to_owned(), to: parse_hex(x, y)? })
        }
        ["end_turn"] => Ok(Action::EndTurn),
        ["move", ..] => Err("move requires four arguments"),
        ["attack", ..] => Err("attack requires five arguments"),
        ["recruit", ..] => Err("recruit requires three arguments"),
        ["recall", ..] => Err("recall requires three arguments"),
        ["end_turn", ..] => Err("end_turn takes no arguments"),
        _ => Err("unknown action"),
    }
}

fn parse_hex(x: &str, y: &str) -> Result<Hex, &'static str> {
    let coordinate = |field| {
        decimal::<i32>(field)
            .filter(|&value| value > 0)
            .ok_or("coordinates must be positive 32-bit integers")
    };
    Hex::from_wml(coordinate(x)?, coordinate(y)?).ok_or("invalid map coordinates")
}

fn decimal<T: FromStr>(field: &str) -> Option<T> {
    if field.bytes().all(|byte| byte.is_ascii_digit()) {
        field.parse().ok()
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_only_commands_logged_by_the_native_wml_driver() {
        let log = concat!(
            "info wml: log mentions RUSTNOTH_AUDIT_COMMAND 9\tend_turn\n",
            "debug wml: RUSTNOTH_AUDIT_COMMAND 9\tend_turn\n",
            "info wml: RUSTNOTH_AUDIT_COMMAND_SUMMARY ignored\n",
            "20261118 12:13:14 info wml: RUSTNOTH_AUDIT_COMMAND 1\tmove\t1\t2\t2\t2\r\n",
            "info wml: RUSTNOTH_AUDIT_COMMAND 2\tattack\t2\t2\t2\t3\t1\n",
            "info wml: RUSTNOTH_AUDIT_COMMAND 3\trecruit\t3\t2\tHeavy Infantryman\n",
            "info wml: RUSTNOTH_AUDIT_COMMAND 4\tend_turn",
        );
        assert_eq!(explored_script(log, 4).unwrap(), concat!(
            "move\t1\t2\t2\t2\nattack\t2\t2\t2\t3\t1\n",
            "recruit\t3\t2\tHeavy Infantryman\nend_turn\n",
        ));
        assert_eq!(explored_script("PASS TEST (0): rustnoth_trace", 0).unwrap(), "");
    }

    #[test]
    fn rejects_corrupt_or_out_of_budget_exploration_commands_at_the_log_line() {
        for (suffix, limit, reason) in [
            ("", 2, "malformed exploration command marker"),
            ("\t2\tend_turn", 2, "malformed exploration command marker"),
            (" 2 end_turn", 2, "malformed exploration command marker"),
            (" 1\tend_turn", 2, "exploration command steps must be consecutive starting at 1"),
            (" 3\tend_turn", 3, "exploration command steps must be consecutive starting at 1"),
            (" 2\tmove\t1\t1\t0\t2", 2, "coordinates must be positive 32-bit integers"),
            (" 2\tend_turn", 1, "exploration command limit exceeded"),
        ] {
            let log = format!("info wml: RUSTNOTH_AUDIT_COMMAND 1\tend_turn\nnoise\ninfo wml: RUSTNOTH_AUDIT_COMMAND{suffix}");
            assert_eq!(explored_script(&log, limit), Err(ParseError { line: 3, reason }));
        }
        assert!(explored_script("info wml: RUSTNOTH_AUDIT_COMMAND 2\tend_turn", 3).is_err());
    }

    #[test]
    fn reads_actions_in_order_with_wml_coordinates() {
        let actions = parse(concat!(
            "# trace actions\r\n",
            " \t\r\n",
            "move\t1\t2\t3\t4\r\n",
            "attack\t3\t4\t5\t6\t0\r\n",
            "recruit\t7\t8\tElvish Fighter\r\n",
            "end_turn",
        ))
        .unwrap();
        assert_eq!(
            actions,
            [
                Action::Move {
                    from: Hex { x: 0, y: 1 },
                    to: Hex { x: 2, y: 3 },
                },
                Action::Attack(AttackChoice {
                    from: Hex { x: 2, y: 3 },
                    to: Hex { x: 4, y: 5 },
                    weapon: 0,
                }),
                Action::Recruit {
                    unit_type: "Elvish Fighter".to_owned(),
                    to: Hex { x: 6, y: 7 },
                },
                Action::EndTurn,
            ],
        );
    }

    #[test]
    fn rejects_invalid_actions_at_the_original_input_line() {
        for (line, reason) in [
            ("move\t1\t2\t3", "move requires four arguments"),
            ("move\t1\t0\t3\t4", "coordinates must be positive 32-bit integers"),
            ("move\t1\t2\t2147483648\t4", "coordinates must be positive 32-bit integers"),
            ("attack\t1\t2\t3\t4\t-1", "invalid weapon index"),
            ("recruit\t1\t2\t ", "missing recruit type ID"),
            ("end_turn\t", "end_turn takes no arguments"),
            ("skip", "unknown action"),
        ] {
            let input = format!("# header\nend_turn\n\n{line}");
            assert_eq!(parse(&input).unwrap_err(), ParseError { line: 4, reason });
        }
    }
}
