//! Global and regional scenario time-of-day cycles.

use std::{collections::BTreeSet, ops::RangeInclusive};

use crate::hex::Hex;

/// A scheduled time and its base lawful combat bonus, in percent.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TimeOfDay {
    pub id: String,
    pub lawful_bonus: i32,
}

/// A unit's response to time-of-day combat bonuses.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Alignment {
    Lawful,
    Neutral,
    Chaotic,
    Liminal,
}

#[derive(Clone, Debug)]
struct TimeCycle {
    entries: Vec<TimeOfDay>,
    turn_offset: i64,
}

impl TimeCycle {
    fn index_at_turn(&self, turn: i32) -> usize {
        (i64::from(turn) + self.turn_offset).rem_euclid(self.entries.len() as i64) as usize
    }

    fn at_turn(&self, turn: i32) -> &TimeOfDay {
        &self.entries[self.index_at_turn(turn)]
    }
}

pub(crate) type WmlRectangle = (RangeInclusive<i32>, RangeInclusive<i32>);

#[derive(Clone, Debug)]
struct TimeArea {
    locations: Vec<WmlRectangle>,
    cycle: TimeCycle,
}

/// The scenario's global cycle and its location-specific overrides.
#[derive(Clone, Debug)]
pub struct TimeSchedule {
    global: TimeCycle,
    areas: Vec<TimeArea>,
    liminal_bonus: i64,
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
            liminal_bonus: default_liminal_bonus(&entries),
            global: TimeCycle {
                entries,
                turn_offset: i64::from(current_time) - i64::from(turn_at),
            },
            areas: Vec::new(),
        }
    }

    /// Returns the zero-based schedule index for an absolute turn number.
    /// Turn zero is the slot immediately before turn one.
    pub fn index_at_turn(&self, turn: i32) -> usize {
        self.global.index_at_turn(turn)
    }

    /// Returns the scheduled time for an absolute turn number.
    pub fn at_turn(&self, turn: i32) -> &TimeOfDay {
        self.global.at_turn(turn)
    }

    pub(crate) fn replace_global(&mut self, entries: Vec<TimeOfDay>, turn: i32, current_time: i32) {
        self.global = TimeCycle { entries, turn_offset: i64::from(current_time) - i64::from(turn) };
    }

    pub(crate) fn add_area(
        &mut self,
        locations: Vec<WmlRectangle>,
        entries: Vec<TimeOfDay>,
        turn_at: i32,
        current_time: i32,
    ) {
        if !entries.is_empty() && !locations.is_empty() {
            self.areas.push(TimeArea {
                locations,
                cycle: TimeCycle {
                    entries,
                    turn_offset: i64::from(current_time) - i64::from(turn_at),
                },
            });
        }
    }

    /// Returns the last matching area's time or the global time at this turn.
    pub fn at_hex(&self, turn: i32, position: Hex) -> &TimeOfDay {
        if let Some((x, y)) = position.to_wml()
            && let Some(area) = self.areas.iter().rev().find(|area|
                area.locations.iter().any(|(xs, ys)| xs.contains(&x) && ys.contains(&y))
            )
        {
            return area.cycle.at_turn(turn);
        }
        self.at_turn(turn)
    }

    /// Sets the scenario's explicit liminal bonus instead of the schedule default.
    pub fn with_liminal_bonus(mut self, bonus: i32) -> Self {
        self.liminal_bonus = i64::from(bonus);
        self
    }

    /// Returns the damage adjustment in percentage points.
    ///
    /// Pass the lawful bonus effective at the unit's location. The global
    /// schedule supplies the liminal bonus even when the local bonus differs.
    pub fn combat_modifier(
        &self,
        lawful_bonus: i32,
        alignment: Alignment,
        fearless: bool,
    ) -> i64 {
        let lawful = i64::from(lawful_bonus);
        let modifier = match alignment {
            Alignment::Lawful => lawful,
            Alignment::Neutral => 0,
            Alignment::Chaotic => -lawful,
            Alignment::Liminal => self.liminal_bonus - lawful.abs(),
        };

        if fearless { modifier.max(0) } else { modifier }
    }
}

fn default_liminal_bonus(entries: &[TimeOfDay]) -> i64 {
    let mut lawful_total = 0;
    let mut chaotic_total = 0;
    let mut candidates = BTreeSet::new();
    for time in entries {
        let bonus = i64::from(time.lawful_bonus);
        lawful_total += bonus.max(0);
        chaotic_total += (-bonus).max(0);
        candidates.insert(bonus.abs());
    }

    let target = lawful_total.max(chaotic_total);
    let mut closest_distance = target;
    let mut chosen_bonus = 0;
    // The liminal total is the candidate applied to each slot, less the
    // sum of the slots' absolute lawful bonuses.
    let magnitude_sum = lawful_total + chaotic_total;
    for candidate in candidates {
        let total = candidate * entries.len() as i64 - magnitude_sum;
        let distance = (target - total).abs();
        if distance < closest_distance {
            closest_distance = distance;
            chosen_bonus = candidate;
        }
    }
    chosen_bonus.max(25)
}

#[cfg(test)]
mod tests {
    use super::{Alignment, TimeOfDay, TimeSchedule};

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

    #[test]
    fn alignment_and_fearless_use_the_effective_lawful_bonus() {
        let schedule = TimeSchedule::new(default_entries(), 1, 0);
        assert_eq!(schedule.combat_modifier(25, Alignment::Lawful, false), 25);
        assert_eq!(schedule.combat_modifier(-25, Alignment::Lawful, false), -25);
        assert_eq!(schedule.combat_modifier(-25, Alignment::Lawful, true), 0);
        assert_eq!(schedule.combat_modifier(25, Alignment::Chaotic, false), -25);
        assert_eq!(schedule.combat_modifier(25, Alignment::Chaotic, true), 0);
        assert_eq!(schedule.combat_modifier(-25, Alignment::Chaotic, true), 25);
        assert_eq!(schedule.combat_modifier(25, Alignment::Neutral, false), 0);
        assert_eq!(schedule.combat_modifier(35, Alignment::Liminal, false), -10);
        assert_eq!(schedule.combat_modifier(-35, Alignment::Liminal, true), 0);
    }

    #[test]
    fn liminal_default_chooses_a_schedule_magnitude_with_a_floor_of_25() {
        let schedule = |bonuses: &[i32]| {
            let entries = bonuses
                .iter()
                .map(|&lawful_bonus| TimeOfDay {
                    id: String::new(),
                    lawful_bonus,
                })
                .collect();
            TimeSchedule::new(entries, 1, 0)
        };
        assert_eq!(
            schedule(&[-40, 0, 25]).combat_modifier(0, Alignment::Liminal, false),
            40
        );
        assert_eq!(
            schedule(&[-10, 0, 10]).combat_modifier(0, Alignment::Liminal, false),
            25
        );
        assert_eq!(
            schedule(&[-50, 50]).combat_modifier(0, Alignment::Liminal, false),
            25
        );
    }

    #[test]
    fn explicit_liminal_bonus_overrides_the_default_floor() {
        let schedule = TimeSchedule::new(default_entries(), 1, 0).with_liminal_bonus(15);
        assert_eq!(schedule.combat_modifier(-10, Alignment::Liminal, false), 5);
        assert_eq!(schedule.combat_modifier(20, Alignment::Liminal, false), -5);
    }
}
