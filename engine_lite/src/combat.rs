use std::num::NonZeroU32;

use crate::rng::ReplayRng;

/// A participant's resolved stats for a combat exchange.
pub struct Combatant {
    pub hitpoints: u32,
    pub strikes: u32,
    pub damage: u32,
    pub hit_chance: u8,
    pub firststrike: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub struct CombatResult {
    pub attacker_hitpoints: u32,
    pub defender_hitpoints: u32,
}

/// Resolves the strike sequence for ordinary seeded combat.
///
/// Both participants' damage and chance to hit are fixed for the exchange.
/// A defender without retaliation has zero strikes. Call from the referee
/// using its combat random stream.
pub fn resolve_exchange(
    attacker: Combatant,
    defender: Combatant,
    rounds: NonZeroU32,
    rng: &mut ReplayRng,
) -> CombatResult {
    let order = if defender.firststrike && !attacker.firststrike {
        [1, 0]
    } else {
        [0, 1]
    };
    let strike_limit = attacker.strikes.max(defender.strikes);
    let mut hp = [attacker.hitpoints, defender.hitpoints];
    let participants = [attacker, defender];

    if strike_limit > 0 && !hp.contains(&0) {
        'fight: for _ in 0..rounds.get() {
            for strike in 0..strike_limit {
                for index in order {
                    let unit = &participants[index];
                    if strike >= unit.strikes {
                        continue;
                    }
                    if rng.next_u32() % 100 < u32::from(unit.hit_chance) {
                        let target = &mut hp[1 - index];
                        *target = target.saturating_sub(unit.damage);
                        if *target == 0 {
                            break 'fight;
                        }
                    }
                }
            }
        }
    }

    CombatResult {
        attacker_hitpoints: hp[0],
        defender_hitpoints: hp[1],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(hitpoints: u32, strikes: u32, damage: u32, hit_chance: u8) -> Combatant {
        Combatant {
            hitpoints,
            strikes,
            damage,
            hit_chance,
            firststrike: false,
        }
    }

    #[test]
    fn alternates_and_checks_each_roll_against_the_striker_chance() {
        let mut rng = ReplayRng::new(5489);
        let result = resolve_exchange(
            unit(20, 2, 5, 12),
            unit(20, 1, 6, 100),
            NonZeroU32::MIN,
            &mut rng,
        );
        assert_eq!(result.attacker_hitpoints, 14);
        assert_eq!(result.defender_hitpoints, 20);
        assert_eq!(rng.seed_and_calls(), (5489, 3));
    }

    #[test]
    fn firststrike_can_end_the_fight_before_retaliation() {
        let mut rng = ReplayRng::new(5489);
        let attacker = unit(4, 1, 5, 100);
        let defender = Combatant { firststrike: true, ..unit(5, 1, 4, 100) };
        let result = resolve_exchange(attacker, defender, NonZeroU32::MIN, &mut rng);
        assert_eq!(result.attacker_hitpoints, 0);
        assert_eq!(result.defender_hitpoints, 5);
        assert_eq!(rng.seed_and_calls(), (5489, 1));
    }

    #[test]
    fn each_round_refreshes_the_strike_counts() {
        let mut rng = ReplayRng::new(5489);
        let result = resolve_exchange(
            unit(10, 1, 3, 100),
            unit(10, 0, 0, 0),
            NonZeroU32::new(2).unwrap(),
            &mut rng,
        );
        assert_eq!(result.attacker_hitpoints, 10);
        assert_eq!(result.defender_hitpoints, 4);
        assert_eq!(rng.seed_and_calls(), (5489, 2));
    }
}
