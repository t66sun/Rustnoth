use std::collections::BTreeMap;

use crate::{
    board::StrikeModifiers,
    combat::{Combatant, apply_hit, strike_order},
};

pub(super) fn select(
    candidates: impl Iterator<Item = ((usize, StrikeModifiers), Combatant, Combatant)>,
) -> Option<(usize, StrikeModifiers)> {
    let mut candidates = candidates.peekable();
    let (mut best, attacker, defender) = candidates.next()?;
    if candidates.peek().is_none() {
        return Some(best);
    }

    let mut best_outcome = exchange(attacker, defender);
    for (candidate, attacker, defender) in candidates {
        let outcome = exchange(attacker, defender);
        if outcome.better_for_defender_than(&best_outcome) {
            best = candidate;
            best_outcome = outcome;
        }
    }
    Some(best)
}

#[derive(Default)]
struct Outcome {
    death: [f64; 2],
    average_hp: [f64; 2],
}

impl Outcome {
    fn add(&mut self, hitpoints: [u32; 2], probability: f64) {
        for (index, hp) in hitpoints.into_iter().enumerate() {
            if hp == 0 {
                self.death[index] += probability;
            }
            self.average_hp[index] += probability * f64::from(hp);
        }
    }

    fn better_for_defender_than(&self, other: &Self) -> bool {
        // Wesnoth 1.18.8 compares kill odds first, then the expected HP difference.
        const TOLERANCE: f64 = 0.01;
        let kill_margin = |outcome: &Self| outcome.death[0] - outcome.death[1];
        let kill_difference = kill_margin(self) - kill_margin(other);
        if kill_difference.abs() > TOLERANCE {
            return kill_difference > 0.0;
        }
        let health_margin = |outcome: &Self| outcome.average_hp[1] - outcome.average_hp[0];
        let health_difference = health_margin(self) - health_margin(other);
        if health_difference.abs() > TOLERANCE {
            return health_difference > 0.0;
        }
        self.average_hp[0] < other.average_hp[0]
    }
}

fn exchange(attacker: Combatant, defender: Combatant) -> Outcome {
    let mut outcome = Outcome::default();
    let starting_hp = [attacker.hitpoints, defender.hitpoints];
    if starting_hp.contains(&0) {
        outcome.add(starting_hp, 1.0);
        return outcome;
    }

    let order = strike_order(&attacker, &defender);
    let units = [attacker, defender];
    let strikes = units.each_ref().map(|unit| {
        if unit.damage == 0 || unit.hit_chance == 0 { 0 } else { unit.strikes }
    });
    let mut ongoing = BTreeMap::from([(starting_hp, 1.0)]);
    for strike in 0..strikes[0].max(strikes[1]) {
        if ongoing.is_empty() {
            break;
        }
        for striker in order {
            if strike >= strikes[striker] {
                continue;
            }
            let unit = &units[striker];
            let target = 1 - striker;
            let chance = f64::from(unit.hit_chance) / 100.0;
            let mut next = BTreeMap::new();
            for (hitpoints, probability) in ongoing {
                if chance < 1.0 {
                    *next.entry(hitpoints).or_default() += probability * (1.0 - chance);
                }
                let mut after_hit = hitpoints;
                apply_hit(&units, &mut after_hit, striker);
                if after_hit[target] == 0 {
                    outcome.add(after_hit, probability * chance);
                } else {
                    *next.entry(after_hit).or_default() += probability * chance;
                }
            }
            ongoing = next;
        }
    }
    for (hitpoints, probability) in ongoing {
        outcome.add(hitpoints, probability);
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    fn combatant(hitpoints: u32, strikes: u32, damage: u32, hit_chance: u8) -> Combatant {
        Combatant {
            hitpoints,
            strikes,
            damage,
            hit_chance,
            firststrike: false,
            drain_healing_cap: None,
            undrainable: false,
        }
    }

    #[test]
    fn favors_a_chance_to_kill_over_more_expected_damage() {
        let steady = exchange(combatant(10, 1, 1, 100), combatant(20, 3, 3, 100));
        let lethal = exchange(combatant(10, 1, 1, 100), combatant(20, 1, 10, 20));
        assert_eq!(steady.average_hp[0], 1.0);
        assert_eq!(lethal.average_hp[0], 8.0);
        assert_eq!(lethal.death[0], 0.2);
        assert!(lethal.better_for_defender_than(&steady));
    }

    #[test]
    fn favors_draining_retaliation_when_healing_outweighs_extra_damage() {
        let draining = exchange(
            combatant(20, 1, 6, 100),
            Combatant { drain_healing_cap: Some(20), ..combatant(20, 1, 6, 100) },
        );
        let plain = exchange(combatant(20, 1, 6, 100), combatant(20, 1, 7, 100));
        assert_eq!(draining.average_hp, [14.0, 17.0]);
        assert!(draining.better_for_defender_than(&plain));
    }

    #[test]
    fn only_surviving_defenders_can_retaliate() {
        let outcome = exchange(combatant(10, 1, 10, 75), combatant(10, 1, 10, 100));
        assert_eq!(outcome.death, [0.25, 0.75]);
        assert_eq!(outcome.average_hp, [7.5, 2.5]);
    }
}
