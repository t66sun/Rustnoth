use std::num::NonZeroU32;

use crate::rng::ReplayRng;

pub(crate) const POISON_AMOUNT: i32 = 8;

/// Poison carried into battle, including the effect of the selected weapon.
#[derive(Clone, Copy, Debug, Default)]
pub struct Poison {
    pub inflicts: bool,
    pub immune: bool,
    pub active: bool,
}

impl Poison {
    pub(crate) fn affects(self, target: Self) -> bool {
        self.inflicts && !target.immune
    }
}

/// A participant's resolved stats for a combat exchange.
pub struct Combatant {
    pub hitpoints: u32,
    pub strikes: u32,
    pub damage: u32,
    pub slow_damage: u32,
    pub slows: bool,
    pub slowed: bool,
    pub unslowable: bool,
    pub hit_chance: u8,
    pub firststrike: bool,
    /// Maximum HP the striker can heal to with the stock drains special.
    pub drain_healing_cap: Option<u32>,
    pub undrainable: bool,
    pub poison: Poison,
}

#[derive(Debug, PartialEq, Eq)]
pub struct CombatResult {
    pub attacker_hitpoints: u32,
    pub defender_hitpoints: u32,
    pub attacker_slowed: bool,
    pub defender_slowed: bool,
    /// Attacker and defender poison at the end of the exchange.
    pub poisoned: [bool; 2],
}

/// Computes a weapon's chance to hit before specials and invulnerability.
pub fn base_hit_chance(terrain_chance: u8, accuracy: i32, parry: i32) -> u8 {
    let chance = i64::from(terrain_chance) + i64::from(accuracy) - i64::from(parry);
    chance.clamp(0, 100) as u8
}

pub(crate) fn strike_order(attacker: &Combatant, defender: &Combatant) -> [usize; 2] {
    if defender.firststrike && !attacker.firststrike {
        [1, 0]
    } else {
        [0, 1]
    }
}

/// Applies a successful strike before death checks and the next retaliation.
pub(crate) fn apply_hit(
    units: &[Combatant; 2], hp: &mut [u32; 2], slowed: &mut [bool; 2], striker: usize,
) {
    let target = 1 - striker;
    let weapon_damage = if slowed[striker] { units[striker].slow_damage } else { units[striker].damage };
    let damage = hp[target].min(weapon_damage);
    hp[target] -= damage;
    if let Some(cap) = units[striker].drain_healing_cap.filter(|_| !units[target].undrainable) {
        hp[striker] = hp[striker].saturating_add(damage / 2).min(cap.max(1));
    }
    if hp[target] > 0 && units[striker].slows && !units[target].unslowable {
        slowed[target] = true;
    }
}

/// Resolves the strike sequence for ordinary seeded combat.
///
/// A defender without retaliation has zero strikes. Call from the referee
/// using its combat random stream.
pub fn resolve_exchange(
    attacker: Combatant,
    defender: Combatant,
    rounds: NonZeroU32,
    rng: &mut ReplayRng,
) -> CombatResult {
    let order = strike_order(&attacker, &defender);
    let strike_limit = attacker.strikes.max(defender.strikes);
    let mut hp = [attacker.hitpoints, defender.hitpoints];
    let mut slowed = [attacker.slowed, defender.slowed];
    let participants = [attacker, defender];
    let mut poisoned = participants.each_ref().map(|unit| unit.poison.active);

    if strike_limit > 0 && !hp.contains(&0) {
        'fight: for _ in 0..rounds.get() {
            for strike in 0..strike_limit {
                for index in order {
                    let unit = &participants[index];
                    if strike >= unit.strikes {
                        continue;
                    }
                    if rng.next_u32() % 100 < u32::from(unit.hit_chance) {
                        poisoned[1 - index] |= unit.poison.affects(participants[1 - index].poison);
                        apply_hit(&participants, &mut hp, &mut slowed, index);
                        if hp[1 - index] == 0 {
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
        attacker_slowed: slowed[0],
        defender_slowed: slowed[1],
        poisoned,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accuracy_and_parry_modify_terrain_chance_before_clamping() {
        assert_eq!(base_hit_chance(60, 10, 15), 55);
        assert_eq!(base_hit_chance(60, 10, -5), 75);
        assert_eq!(base_hit_chance(95, 10, 15), 90);
        assert_eq!(base_hit_chance(95, 10, 2), 100);
        assert_eq!(base_hit_chance(10, -5, 20), 0);
    }

    fn unit(hitpoints: u32, strikes: u32, damage: u32, hit_chance: u8) -> Combatant {
        Combatant {
            hitpoints,
            strikes,
            damage,
            slow_damage: damage.div_ceil(2),
            slows: false,
            slowed: false,
            unslowable: false,
            hit_chance,
            firststrike: false,
            drain_healing_cap: None,
            undrainable: false,
            poison: Poison::default(),
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
    fn only_successful_hits_poison_susceptible_opponents() {
        let mut attacker = unit(12, 1, 2, 0);
        attacker.poison.inflicts = true;
        attacker.poison.immune = true;
        let mut defender = unit(12, 1, 2, 100);
        defender.poison.inflicts = true;
        let result = resolve_exchange(attacker, defender, NonZeroU32::MIN, &mut ReplayRng::new(5489));
        assert_eq!((result.attacker_hitpoints, result.defender_hitpoints), (10, 12));
        assert_eq!(result.poisoned, [false, false]);
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
    fn heals_from_actual_damage_before_retaliation_and_on_lethal_hits() {
        let attacker = Combatant { drain_healing_cap: Some(10), ..unit(4, 2, 5, 100) };
        let result = resolve_exchange(
            attacker,
            unit(8, 1, 5, 100),
            NonZeroU32::MIN,
            &mut ReplayRng::new(5489),
        );
        // 4 + floor(5/2) - 5 + floor(3/2).
        assert_eq!(result, CombatResult {
            attacker_hitpoints: 2, defender_hitpoints: 0,
            attacker_slowed: false, defender_slowed: false, poisoned: [false; 2],
        });
    }

    #[test]
    fn drains_on_retaliation_respects_max_hp_and_undrainable_targets() {
        for (undrainable, expected_hp) in [(false, 10), (true, 9)] {
            let attacker = Combatant { undrainable, ..unit(8, 1, 1, 100) };
            let defender = Combatant { drain_healing_cap: Some(10), ..unit(10, 1, 9, 100) };
            let result =
                resolve_exchange(attacker, defender, NonZeroU32::MIN, &mut ReplayRng::new(5489));
            assert_eq!(result.attacker_hitpoints, 0);
            assert_eq!(result.defender_hitpoints, expected_hp);
        }
    }

    #[test]
    fn attacker_strikes_first_when_both_weapons_have_firststrike() {
        let mut rng = ReplayRng::new(5489);
        let attacker = Combatant { firststrike: true, ..unit(4, 1, 5, 100) };
        let defender = Combatant { firststrike: true, ..unit(5, 1, 4, 100) };
        let result = resolve_exchange(attacker, defender, NonZeroU32::MIN, &mut rng);
        assert_eq!(result.attacker_hitpoints, 4);
        assert_eq!(result.defender_hitpoints, 0);
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
