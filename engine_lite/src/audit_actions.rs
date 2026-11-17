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
        ["end_turn"] => Ok(Action::EndTurn),
        ["move", ..] => Err("move requires four arguments"),
        ["attack", ..] => Err("attack requires five arguments"),
        ["recruit", ..] => Err("recruit requires three arguments"),
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
