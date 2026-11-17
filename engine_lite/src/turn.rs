/// The round and one-based side whose turn is in progress.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Turn {
    pub round: i32,
    pub side: usize,
}

impl Turn {
    /// Advances to the next participating side in roster order.
    ///
    /// Supply the side numbers that still take turns. Return `None` when that
    /// roster is empty or the next side would exceed the inclusive round limit.
    /// Use `None` for a scenario with unlimited rounds.
    pub fn next(self, active_sides: &[usize], last_round: Option<i32>) -> Option<Self> {
        let next_side = active_sides
            .iter()
            .copied()
            .filter(|&side| side > self.side)
            .min();
        let (round, side) = match next_side {
            Some(side) => (self.round, side),
            None => (self.round.checked_add(1)?, *active_sides.iter().min()?),
        };
        if last_round.is_some_and(|limit| round > limit) {
            return None;
        }
        Some(Self { round, side })
    }
}

#[cfg(test)]
mod tests {
    use super::Turn;

    #[test]
    fn skips_empty_sides_and_advances_round_only_on_wrap() {
        let first = Turn { round: 3, side: 2 };
        let second = Turn { round: 3, side: 4 };
        assert_eq!(first.next(&[4, 2], None), Some(second));
        assert_eq!(second.next(&[4, 2], None), Some(Turn { round: 4, side: 2 }));
    }

    #[test]
    fn every_active_side_can_play_the_final_round() {
        let first = Turn { round: 12, side: 1 };
        let last = first.next(&[1, 3], Some(12)).unwrap();
        assert_eq!(last, Turn { round: 12, side: 3 });
        assert_eq!(last.next(&[1, 3], Some(12)), None);
        assert_eq!(last.next(&[], None), None);
    }
}
