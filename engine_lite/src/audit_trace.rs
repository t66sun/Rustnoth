//! Tab-separated referee frames for differential comparison.

use std::io::{self, Write};

use crate::{
    fixed_match::{MatchState, audit::Snapshot},
    hex::Hex,
};

/// Writes one frame in the unit and village order supplied by the snapshot.
pub fn write_snapshot(mut out: impl Write, step: usize, snapshot: &Snapshot) -> io::Result<()> {
    match snapshot.state {
        MatchState::Playing(turn) => {
            writeln!(out, "step\t{step}\tplaying\t{}\t{}", turn.round, turn.side)?;
        }
        MatchState::Won(side) => writeln!(out, "step\t{step}\twon\t{side}")?,
        MatchState::Draw => writeln!(out, "step\t{step}\tdraw")?,
    }
    for (index, amount) in snapshot.gold.iter().enumerate() {
        writeln!(out, "gold\t{}\t{amount}", index + 1)?;
    }
    for unit in &snapshot.units {
        let (x, y) = wml_position(unit.position)?;
        let type_id = escape_id(&unit.type_id);
        write!(
            out,
            "unit\t{x}\t{y}\t{}\t{type_id}\t{}\t{}\t{}\t{}\t{}",
            unit.side,
            unit.hitpoints,
            unit.moves_left,
            unit.attacks_left,
            u8::from(unit.can_recruit),
            unit.experience
        )?;
        for trait_id in &unit.traits {
            write!(out, "\t{}", escape_id(trait_id))?;
        }
        writeln!(out)?;
        let statuses = match (unit.slowed, unit.poisoned) {
            (true, true) => Some("slowed\tpoisoned"),
            (true, false) => Some("slowed"),
            (false, true) => Some("poisoned"),
            (false, false) => None,
        };
        if let Some(statuses) = statuses {
            writeln!(out, "status\t{x}\t{y}\t{statuses}")?;
        }
    }
    for &(position, side) in &snapshot.villages {
        let (x, y) = wml_position(position)?;
        writeln!(out, "village\t{x}\t{y}\t{side}")?;
    }
    writeln!(out, "end\t{step}")
}

fn escape_id(id: &str) -> String {
    let mut escaped = String::with_capacity(id.len());
    for character in id.chars() {
        match character {
            '\\' => escaped.push_str("\\\\"),
            '\t' => escaped.push_str("\\t"),
            '\r' => escaped.push_str("\\r"),
            '\n' => escaped.push_str("\\n"),
            _ => escaped.push(character),
        }
    }
    escaped
}

fn wml_position(hex: Hex) -> io::Result<(i32, i32)> {
    match hex.to_wml() {
        Some((x, y)) if x > 0 && y > 0 => Ok((x, y)),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("hex {hex:?} has no positive WML coordinates"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{fixed_match::audit::SnapshotUnit, turn::Turn};

    #[test]
    fn writes_a_playing_frame_with_wml_coordinates_and_escaped_type_ids() {
        let snapshot = Snapshot {
            state: MatchState::Playing(Turn { round: 7, side: 2 }),
            gold: vec![-4, 17],
            units: vec![
                SnapshotUnit {
                    position: Hex { x: 0, y: 2 },
                    side: 2,
                    type_id: "夜 Blade\\t\t\r\n".to_owned(),
                    hitpoints: 13,
                    moves_left: 4,
                    attacks_left: 2,
                    can_recruit: true,
                    experience: 9,
                    slowed: true,
                    poisoned: true,
                    traits: vec!["quick".to_owned(), "夜\tfighter".to_owned()],
                },
                SnapshotUnit {
                    position: Hex { x: 1, y: 0 },
                    side: 1,
                    type_id: "Spearman".to_owned(),
                    hitpoints: 8,
                    moves_left: 0,
                    attacks_left: 0,
                    can_recruit: false,
                    experience: 0,
                    slowed: false,
                    poisoned: true,
                    traits: Vec::new(),
                },
            ],
            villages: vec![(Hex { x: 0, y: 1 }, 1), (Hex { x: 4, y: 0 }, 2)],
        };
        let mut output = Vec::new();
        write_snapshot(&mut output, 3, &snapshot).unwrap();
        assert_eq!(
            output.as_slice(),
            concat!(
                "step\t3\tplaying\t7\t2\n",
                "gold\t1\t-4\n",
                "gold\t2\t17\n",
                "unit\t1\t3\t2\t夜 Blade\\\\t\\t\\r\\n\t13\t4\t2\t1\t9\tquick\t夜\\tfighter\n",
                "status\t1\t3\tslowed\tpoisoned\n",
                "unit\t2\t1\t1\tSpearman\t8\t0\t0\t0\t0\n",
                "status\t2\t1\tpoisoned\n",
                "village\t1\t2\t1\n",
                "village\t5\t1\t2\n",
                "end\t3\n",
            )
            .as_bytes()
        );
    }

    #[test]
    fn writes_both_terminal_headers_without_turn_fields() {
        let mut output = Vec::new();
        for (step, state) in [(0, MatchState::Won(2)), (1, MatchState::Draw)] {
            let snapshot = Snapshot {
                state,
                gold: vec![0, 9],
                units: Vec::new(),
                villages: Vec::new(),
            };
            write_snapshot(&mut output, step, &snapshot).unwrap();
        }
        assert_eq!(
            output.as_slice(),
            b"step\t0\twon\t2\ngold\t1\t0\ngold\t2\t9\nend\t0\nstep\t1\tdraw\ngold\t1\t0\ngold\t2\t9\nend\t1\n".as_slice()
        );
    }

    #[test]
    fn rejects_nonpositive_and_unrepresentable_wml_coordinates() {
        for position in [Hex { x: -1, y: 0 }, Hex { x: 0, y: i32::MAX }] {
            let snapshot = Snapshot {
                state: MatchState::Draw,
                gold: vec![0, 0],
                units: Vec::new(),
                villages: vec![(position, 1)],
            };
            assert_eq!(
                write_snapshot(Vec::new(), 0, &snapshot).unwrap_err().kind(),
                io::ErrorKind::InvalidData
            );
        }
    }
}
