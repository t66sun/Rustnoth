use std::{error, fmt, num::NonZeroU32};

use crate::{
    combat::{CombatResult, Combatant, base_hit_chance, resolve_exchange},
    hex::Hex,
    rng::ReplayRng,
    strike_damage::strike_damage,
    weapon::WeaponProfile,
};

use super::{Battlefield, UnitState};

/// A weapon choice against an enemy on a fully visible battlefield.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AttackChoice {
    pub from: Hex,
    pub to: Hex,
    pub weapon: usize,
}

/// An attack with the rules inputs resolved by the referee.
pub struct Attack {
    pub from: Hex,
    pub to: Hex,
    pub weapon: usize,
    pub modifiers: StrikeModifiers,
    pub retaliation: Option<(usize, StrikeModifiers)>,
}

pub struct StrikeModifiers {
    /// Terrain chance to be hit of this strike's target.
    pub target_hit_chance: u8,
    /// Combined alignment and resistance multiplier in ten-thousandths.
    pub damage_multiplier: i32,
}

impl Battlefield<'_> {
    /// Lists available attacks in origin, target, then weapon-index order.
    ///
    /// Call only for a fully visible battlefield. The returned choices
    /// contain the inputs the policy can select.
    pub fn attack_choices(
        &self,
        active_side: usize,
        is_enemy: impl Fn(usize) -> bool,
    ) -> Vec<AttackChoice> {
        let mut choices = Vec::new();
        for (&from, attacker) in &self.units {
            if attacker.side != active_side || attacker.attacks_left <= 0 {
                continue;
            }
            for (&to, defender) in &self.units {
                if defender.side == active_side || !is_enemy(defender.side) {
                    continue;
                }
                let distance = from.distance(to) as i64;
                for (weapon, profile) in attacker.unit_type.attacks.iter().enumerate() {
                    if within_range(profile, distance) {
                        choices.push(AttackChoice { from, to, weapon });
                    }
                }
            }
        }
        choices
    }

    /// Resolves one ordinary attack and applies its damage and action costs.
    ///
    /// The caller chooses the retaliation weapon and supplies the modifiers
    /// for both sides. This path covers basic combat with fixed strike stats.
    pub fn apply_attack(
        &mut self,
        active_side: usize,
        action: Attack,
        is_enemy: impl Fn(usize) -> bool,
        rng: &mut ReplayRng,
    ) -> Result<CombatResult, AttackError> {
        let from = action.from;
        let to = action.to;
        let attacker = self.units.get(&from).ok_or(AttackError::MissingAttacker)?;
        if attacker.side != active_side || attacker.attacks_left <= 0 {
            return Err(AttackError::AttackerUnavailable);
        }
        let defender = self.units.get(&to).ok_or(AttackError::MissingDefender)?;
        if defender.side == active_side || !is_enemy(defender.side) {
            return Err(AttackError::FriendlyTarget);
        }
        let distance = from.distance(to) as i64;
        let weapon = attacker
            .unit_type
            .attacks
            .get(action.weapon)
            .filter(|weapon| within_range(weapon, distance))
            .ok_or(AttackError::UnavailableWeapon)?;
        let retaliation = match &action.retaliation {
            Some((index, modifiers)) => {
                let response = defender
                    .unit_type
                    .attacks
                    .get(*index)
                    .filter(|response| response.range == weapon.range && within_range(response, distance))
                    .ok_or(AttackError::UnavailableRetaliation)?;
                Some((response, modifiers))
            }
            None => None,
        };
        let attacker_stats = combatant(
            attacker,
            weapon,
            &action.modifiers,
            retaliation.map_or(0, |(weapon, _)| weapon.parry),
        );
        let defender_stats = retaliation.map_or(
            Combatant {
                hitpoints: defender.hitpoints.max(0) as u32,
                strikes: 0,
                damage: 0,
                hit_chance: 0,
                firststrike: false,
            },
            |(response, modifiers)| combatant(defender, response, modifiers, weapon.parry),
        );
        let movement_used = weapon.movement_used;
        let attacks_used = weapon.attacks_used;
        let result = resolve_exchange(attacker_stats, defender_stats, NonZeroU32::MIN, rng);

        self.resting.remove(&from);
        self.resting.remove(&to);
        if result.attacker_hitpoints == 0 {
            self.units.remove(&from);
        } else {
            let unit = self.units.get_mut(&from).expect("validated attacker");
            unit.hitpoints = result.attacker_hitpoints as i32;
            unit.moves_left = unit.moves_left.saturating_sub(movement_used).max(0);
            unit.attacks_left = unit.attacks_left.saturating_sub(attacks_used).max(0);
        }
        if result.defender_hitpoints == 0 {
            self.units.remove(&to);
        } else {
            self.units.get_mut(&to).expect("validated defender").hitpoints =
                result.defender_hitpoints as i32;
        }
        Ok(result)
    }
}

pub(crate) fn within_range(weapon: &WeaponProfile, distance: i64) -> bool {
    (i64::from(weapon.min_range)..=i64::from(weapon.max_range)).contains(&distance)
}

pub(crate) fn combatant(
    unit: &UnitState<'_>,
    weapon: &WeaponProfile,
    modifiers: &StrikeModifiers,
    opposing_parry: i32,
) -> Combatant {
    let damage = strike_damage(weapon.damage, modifiers.damage_multiplier, false);
    Combatant {
        hitpoints: unit.hitpoints.max(0) as u32,
        strikes: weapon.strikes.max(0) as u32,
        damage: u32::try_from(damage).unwrap_or(u32::MAX),
        hit_chance: base_hit_chance(modifiers.target_hit_chance, weapon.accuracy, opposing_parry),
        firststrike: false,
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum AttackError {
    MissingAttacker,
    AttackerUnavailable,
    MissingDefender,
    FriendlyTarget,
    UnavailableWeapon,
    UnavailableRetaliation,
}

impl fmt::Display for AttackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::MissingAttacker => "attack origin has no unit",
            Self::AttackerUnavailable => "attack origin cannot act for this side",
            Self::MissingDefender => "attack target has no unit",
            Self::FriendlyTarget => "attack target is friendly",
            Self::UnavailableWeapon => "attacking weapon is missing or out of range",
            Self::UnavailableRetaliation => "retaliation weapon is missing or has the wrong range",
        };
        f.write_str(message)
    }
}

impl error::Error for AttackError {}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::{map::ScenarioMap, starting_units::read_starting_units, unit_type::UnitType, wml};

    use super::*;

    const ATTACKER: Hex = Hex { x: 0, y: 0 };
    const DEFENDER: Hex = Hex { x: 1, y: 0 };

    fn unit_types() -> BTreeMap<String, UnitType> {
        let types = wml::parse(
            "[unit_type]\nid=Fighter\nhitpoints=10\nmovement=5\n\
             [attack]\nname=sword\nrange=melee\ndamage=10\nnumber=1\nmovement_used=2\n[/attack]\n[/unit_type]",
        )
        .unwrap();
        let unit = UnitType::from_wml(types.children.into_iter().next().unwrap().1);
        BTreeMap::from([(unit.id.clone(), unit)])
    }

    fn board(types: &BTreeMap<String, UnitType>) -> Battlefield<'_> {
        let map = ScenarioMap::parse("Xv,Xv,Xv,Xv\nXv,1 Kh,2 Kh,Xv\nXv,Xv,Xv,Xv").unwrap();
        let scenario = wml::parse("[side]\ntype=Fighter\n[/side]\n[side]\ntype=Fighter\n[/side]").unwrap();
        let starts = read_starting_units(&scenario, &map).unwrap();
        Battlefield::new(map, starts, types).unwrap()
    }

    fn attack(weapon: usize) -> Attack {
        Attack {
            from: ATTACKER,
            to: DEFENDER,
            weapon,
            modifiers: StrikeModifiers { target_hit_chance: 100, damage_multiplier: 10_000 },
            retaliation: Some((0, StrikeModifiers { target_hit_chance: 100, damage_multiplier: 10_000 })),
        }
    }

    #[test]
    fn a_lethal_strike_removes_the_defender_and_spends_the_attackers_actions() {
        let types = unit_types();
        let mut board = board(&types);
        let mut rng = ReplayRng::new(5489);
        let result = board.apply_attack(1, attack(0), |side| side == 2, &mut rng).unwrap();
        assert_eq!(result.attacker_hitpoints, 10);
        assert_eq!(result.defender_hitpoints, 0);
        assert!(board.unit_at(DEFENDER).is_none());
        let attacker = board.unit_at(ATTACKER).unwrap();
        assert_eq!((attacker.moves_left, attacker.attacks_left), (3, 0));
        assert_eq!(rng.seed_and_calls(), (5489, 1));
    }

    #[test]
    fn invalid_attacks_do_not_spend_actions_or_randomness() {
        let types = unit_types();
        let mut board = board(&types);
        let mut rng = ReplayRng::new(5489);
        assert_eq!(
            board.apply_attack(1, attack(1), |side| side == 2, &mut rng),
            Err(AttackError::UnavailableWeapon)
        );
        assert_eq!(
            board.apply_attack(1, attack(0), |_| false, &mut rng),
            Err(AttackError::FriendlyTarget)
        );
        assert_eq!(board.unit_at(DEFENDER).unwrap().hitpoints, 10);
        assert_eq!(board.unit_at(ATTACKER).unwrap().attacks_left, 1);
        assert_eq!(rng.seed_and_calls(), (5489, 0));
    }

    #[test]
    fn lists_only_in_range_enemy_targets_for_units_with_attacks_left() {
        let mut types = unit_types();
        let attacks = &mut types.get_mut("Fighter").unwrap().attacks;
        attacks.push(WeaponProfile::from_wml(
            wml::parse("name=longbow\nmin_range=2\nmax_range=3").unwrap(),
        ));
        let mut board = board(&types);

        assert_eq!(
            board.attack_choices(1, |side| side == 2),
            [AttackChoice {
                from: ATTACKER,
                to: DEFENDER,
                weapon: 0,
            }]
        );
        assert!(board.attack_choices(1, |_| false).is_empty());

        board.units.get_mut(&ATTACKER).unwrap().attacks_left = 0;
        assert!(board.attack_choices(1, |side| side == 2).is_empty());
    }
}
