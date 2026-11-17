//! Scenario-wide time-of-day cycles.

/// A scheduled time and its base lawful combat bonus, in percent.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TimeOfDay {
    pub id: String,
    pub lawful_bonus: i32,
}

/// The global schedule anchored to the scenario's turn and time index.
#[derive(Clone, Debug)]
pub struct TimeSchedule {
    entries: Vec<TimeOfDay>,
    turn_offset: i64,
}

impl TimeSchedule {
    /// Builds a cycle in the supplied order.
    ///
    /// `current_time` is a zero-based index at `turn_at`. Wesnoth defaults
    /// these scenario values to 0 and 1 respectively. Indices wrap in both
    /// directions. A scenario without entries uses the neutral `nulltod`.
    pub fn new(mut entries: Vec<TimeOfDay>, turn_at: i32, current_time: i32) -> Self {
        if entries.is_empty() {
            entries.push(TimeOfDay {
                id: String::from("nulltod"),
                lawful_bonus: 0,
            });
        }

        Self {
            entries,
            turn_offset: i64::from(current_time) - i64::from(turn_at),
        }
    }

    /// Returns the zero-based schedule index for an absolute turn number.
    /// Turn zero is the slot immediately before turn one.
    pub fn index_at_turn(&self, turn: i32) -> usize {
        let index = i64::from(turn) + self.turn_offset;
        index.rem_euclid(self.entries.len() as i64) as usize
    }

    /// Returns the scheduled time for an absolute turn number.
    pub fn at_turn(&self, turn: i32) -> &TimeOfDay {
        &self.entries[self.index_at_turn(turn)]
    }
}

#[cfg(test)]
mod tests {
    use super::{TimeOfDay, TimeSchedule};

    fn default_entries() -> Vec<TimeOfDay> {
        [
            ("dawn", 0),
            ("morning", 25),
            ("afternoon", 25),
            ("dusk", 0),
            ("first_watch", -25),
            ("second_watch", -25),
        ]
        .into_iter()
        .map(|(id, lawful_bonus)| TimeOfDay {
            id: id.into(),
            lawful_bonus,
        })
        .collect()
    }

    #[test]
    fn preserves_cycle_order_and_wraps_in_both_directions() {
        let schedule = TimeSchedule::new(default_entries(), 1, 0);
        let times = (0..=7)
            .map(|turn| {
                let time = schedule.at_turn(turn);
                (time.id.as_str(), time.lawful_bonus)
            })
            .collect::<Vec<_>>();

        assert_eq!(
            times,
            [
                ("second_watch", -25),
                ("dawn", 0),
                ("morning", 25),
                ("afternoon", 25),
                ("dusk", 0),
                ("first_watch", -25),
                ("second_watch", -25),
                ("dawn", 0),
            ]
        );
    }

    #[test]
    fn anchors_the_explicit_index_to_the_scenario_turn() {
        let schedule = TimeSchedule::new(default_entries(), 9, 1);
        assert_eq!(schedule.at_turn(8).id, "dawn");
        assert_eq!(schedule.at_turn(9).id, "morning");
        assert_eq!(schedule.at_turn(10).id, "afternoon");
        assert_eq!(schedule.at_turn(15).id, "morning");
    }

    #[test]
    fn wraps_out_of_range_current_time() {
        let early = TimeSchedule::new(default_entries(), 1, -1);
        let late = TimeSchedule::new(default_entries(), 1, 7);
        assert_eq!(early.index_at_turn(1), 5);
        assert_eq!(late.index_at_turn(1), 1);
    }

    #[test]
    fn empty_schedule_resolves_to_neutral_time() {
        let schedule = TimeSchedule::new(Vec::new(), 9, -1);
        let time = schedule.at_turn(12);
        assert_eq!(schedule.index_at_turn(12), 0);
        assert_eq!(time.id, "nulltod");
        assert_eq!(time.lawful_bonus, 0);
    }
}
