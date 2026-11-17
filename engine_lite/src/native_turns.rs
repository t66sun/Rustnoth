//! Groups an audit script by the native turn refresh event that runs it.

use std::{error::Error, fmt};

use crate::{fixed_match::Action, turn::Turn};

#[derive(Debug, Eq, PartialEq)]
pub struct TurnGroup<'a> {
    pub round: i32,
    pub side: usize,
    /// Total actions completed when this side turn starts, including end turns.
    pub before_step: usize,
    pub actions: &'a [Action],
    pub end_turn: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TurnOverflow;

impl fmt::Display for TurnOverflow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("audit script exceeds the supported round range")
    }
}

impl Error for TurnOverflow {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PartitionError {
    InvalidActiveSides,
    RoundOverflow,
}

impl fmt::Display for PartitionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidActiveSides => {
                f.write_str("audit active sides must be nonempty, positive, and strictly increasing")
            }
            Self::RoundOverflow => fmt::Display::fmt(&TurnOverflow, f),
        }
    }
}

impl Error for PartitionError {}

/// Start with side 1 and keep a group after every end turn, even if it has no actions.
pub fn partition(actions: &[Action], first_round: i32) -> Result<Vec<TurnGroup<'_>>, TurnOverflow> {
    partition_ordered(actions, first_round, 1, &[1, 2])
}

/// Start with the first active side and keep a group after every end turn.
pub fn partition_for_sides<'a>(
    actions: &'a [Action],
    first_round: i32,
    sides: &[usize],
) -> Result<Vec<TurnGroup<'a>>, PartitionError> {
    let Some(&first_side) = sides.first() else {
        return Err(PartitionError::InvalidActiveSides);
    };
    if first_side == 0 || sides.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(PartitionError::InvalidActiveSides);
    }
    partition_ordered(actions, first_round, first_side, sides)
        .map_err(|_| PartitionError::RoundOverflow)
}

fn partition_ordered<'a>(
    actions: &'a [Action],
    first_round: i32,
    first_side: usize,
    sides: &[usize],
) -> Result<Vec<TurnGroup<'a>>, TurnOverflow> {
    let mut groups = Vec::new();
    let mut turn = Turn {
        round: first_round,
        side: first_side,
    };
    let mut before_step = 0;

    for turn_actions in actions.split(|action| matches!(action, Action::EndTurn)) {
        let end_step = before_step + turn_actions.len();
        let end_turn = end_step < actions.len();
        groups.push(TurnGroup {
            round: turn.round,
            side: turn.side,
            before_step,
            actions: turn_actions,
            end_turn,
        });
        if end_turn {
            turn = turn.next(sides, None).ok_or(TurnOverflow)?;
            before_step = end_step + 1;
        }
    }
    Ok(groups)
}

#[cfg(test)]
mod tests {
    use super::{PartitionError, TurnGroup, TurnOverflow, partition, partition_for_sides};
    use crate::{fixed_match::Action, hex::Hex};

    #[test]
    fn empty_script_still_has_the_initial_snapshot() {
        assert_eq!(
            partition(&[], 7).unwrap(),
            [TurnGroup {
                round: 7,
                side: 1,
                before_step: 0,
                actions: &[],
                end_turn: false,
            }]
        );
    }

    #[test]
    fn keeps_consecutive_and_trailing_end_turns_as_snapshot_boundaries() {
        let actions = [
            Action::EndTurn,
            Action::EndTurn,
            Action::Move {
                from: Hex { x: 0, y: 0 },
                to: Hex { x: 1, y: 0 },
            },
            Action::EndTurn,
        ];
        assert_eq!(
            partition(&actions, 7).unwrap(),
            [
                TurnGroup {
                    round: 7,
                    side: 1,
                    before_step: 0,
                    actions: &[],
                    end_turn: true,
                },
                TurnGroup {
                    round: 7,
                    side: 2,
                    before_step: 1,
                    actions: &[],
                    end_turn: true,
                },
                TurnGroup {
                    round: 8,
                    side: 1,
                    before_step: 2,
                    actions: &actions[2..3],
                    end_turn: true,
                },
                TurnGroup {
                    round: 8,
                    side: 2,
                    before_step: 4,
                    actions: &[],
                    end_turn: false,
                },
            ]
        );
    }

    #[test]
    fn reports_round_overflow_when_side_two_ends() {
        assert_eq!(
            partition(&[Action::EndTurn, Action::EndTurn], i32::MAX),
            Err(TurnOverflow)
        );
    }

    #[test]
    fn advances_through_three_active_sides_before_wrapping_the_round() {
        let actions = [Action::EndTurn, Action::EndTurn, Action::EndTurn];
        let groups = partition_for_sides(&actions, 7, &[2, 4, 7]).unwrap();
        let checkpoints: Vec<_> =
            groups.iter().map(|group| (group.round, group.side, group.before_step)).collect();
        assert_eq!(checkpoints, [(7, 2, 0), (7, 4, 1), (7, 7, 2), (8, 2, 3)]);
        assert_eq!(
            partition_for_sides(&actions, i32::MAX, &[2, 4, 7]),
            Err(PartitionError::RoundOverflow),
        );
    }

    #[test]
    fn rejects_invalid_active_side_order() {
        for sides in [&[][..], &[0, 1][..], &[2, 1][..], &[1, 1][..]] {
            assert_eq!(
                partition_for_sides(&[], 1, sides),
                Err(PartitionError::InvalidActiveSides),
            );
        }
    }
}
