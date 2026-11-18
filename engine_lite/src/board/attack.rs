use std::{error, fmt, num::NonZeroU32};

use crate::{
    combat::{CombatResult, Combatant, Poison, base_hit_chance, resolve_exchange},
    hex::{Direction, Hex},
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

#[derive(Clone, Copy)]
pub(crate) enum StrikeRole<'a> {
    Attack { backstab: bool },
    Retaliate(&'a WeaponProfile),
}

impl Battlefield<'_> {
    pub(crate) fn backstabs(
        &self,
        from: Hex,
        to: Hex,
        weapon: &WeaponProfile,
        enemy_of_defender: impl Fn(usize) -> bool,
    ) -> bool {
        weapon.has_backstab()
            && Direction::CLOCKWISE.into_iter()
                .find(|&direction| from.neighbor(direction) == Some(to))
                .and_then(|direction| to.neighbor(direction))
                .and_then(|opposite| self.unit_at(opposite))
                .is_some_and(|flanker| enemy_of_defender(flanker.side))
    }

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

    /// Resolves one ordinary attack and applies damage, experience, and action costs.
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
        self.apply_attack_with_relations(active_side, action, |a, b| {
            if a == active_side { is_enemy(b) } else { a != b }
        }, rng)
    }

    /// Uses the defender's own enemy relations to resolve the flanking unit's allegiance.
    pub fn apply_attack_with_relations(
        &mut self,
        active_side: usize,
        action: Attack,
        sides_are_enemies: impl Fn(usize, usize) -> bool,
        rng: &mut ReplayRng,
    ) -> Result<CombatResult, AttackError> {
        let from = action.from;
        let to = action.to;
        let attacker = self.units.get(&from).ok_or(AttackError::MissingAttacker)?;
        if attacker.side != active_side || attacker.attacks_left <= 0 {
            return Err(AttackError::AttackerUnavailable);
        }
        let defender = self.units.get(&to).ok_or(AttackError::MissingDefender)?;
        if defender.side == active_side || !sides_are_enemies(active_side, defender.side) {
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
                Some((*index, response, modifiers))
            }
            None => None,
        };
        let attacker_stats = combatant(
            attacker,
            action.weapon,
            &action.modifiers,
            retaliation.map_or(0, |(_, response, _)| response.parry),
            StrikeRole::Attack {
                backstab: self.backstabs(from, to, weapon, |side| sides_are_enemies(defender.side, side)),
            },
        );
        let defender_stats = retaliation.map_or(
            Combatant {
                hitpoints: defender.hitpoints.max(0) as u32,
                strikes: 0,
                damage: 0,
                slow_damage: 0,
                slows: false,
                slowed: defender.slowed,
                unslowable: defender.is_unslowable(),
                petrifies: false,
                unpetrifiable: defender.is_unpetrifiable(),
                hit_chance: 0,
                firststrike: false,
                poison: defender.poison_in_combat(false),
                drain_healing_cap: None,
                undrainable: defender.is_undrainable(),
            },
            |(index, _, modifiers)| {
                combatant(defender, index, modifiers, weapon.parry, StrikeRole::Retaliate(weapon))
            },
        );
        let movement_used = weapon.movement_used;
        let attacks_used = weapon.attacks_used;
        let rounds = weapon.combat_rounds().max(
            retaliation.map_or(NonZeroU32::MIN, |(_, response, _)| response.combat_rounds()),
        );
        let result = resolve_exchange(attacker_stats, defender_stats, rounds, rng);
        let attacker_experience =
            combat_experience(defender.unit_type.level, result.defender_hitpoints == 0);
        let defender_experience =
            combat_experience(attacker.unit_type.level, result.attacker_hitpoints == 0);

        self.resting.remove(&from);
        self.resting.remove(&to);
        if result.attacker_hitpoints == 0 {
            self.units.remove(&from);
        } else {
            let unit = self.units.get_mut(&from).expect("validated attacker");
            unit.uncovered = true;
            unit.hitpoints = result.attacker_hitpoints as i32;
            unit.slowed = result.attacker_slowed;
            unit.poisoned = result.poisoned[0];
            unit.petrified |= result.petrified[0];
            unit.experience = unit.experience.saturating_add(attacker_experience);
            unit.moves_left = unit.moves_left.saturating_sub(movement_used).max(0);
            unit.attacks_left = unit.attacks_left.saturating_sub(attacks_used).max(0);
        }
        if result.defender_hitpoints == 0 {
            self.units.remove(&to);
        } else {
            let unit = self.units.get_mut(&to).expect("validated defender");
            unit.hitpoints = result.defender_hitpoints as i32;
            unit.slowed = result.defender_slowed;
            unit.poisoned = result.poisoned[1];
            unit.petrified |= result.petrified[1];
            unit.experience = unit.experience.saturating_add(defender_experience);
        }
        Ok(result)
    }
}

fn combat_experience(opponent_level: i32, opponent_defeated: bool) -> i32 {
    const KILL_EXPERIENCE: i32 = 8;
    match (opponent_defeated, opponent_level) {
        (true, 0) => KILL_EXPERIENCE / 2,
        (true, level) => level.saturating_mul(KILL_EXPERIENCE),
        (false, level) => level,
    }
}

impl UnitState<'_> {
    fn is_unslowable(&self) -> bool {
        self.traits.as_ref().is_some_and(|traits| traits.stats.unslowable)
    }

    fn is_unpetrifiable(&self) -> bool {
        self.unpetrifiable || self.traits.as_ref().is_some_and(|traits| traits.stats.unpetrifiable)
    }

    fn is_undrainable(&self) -> bool {
        self.traits.as_ref().is_some_and(|traits| traits.stats.undrainable)
    }
}

pub(crate) fn within_range(weapon: &WeaponProfile, distance: i64) -> bool {
    (i64::from(weapon.min_range)..=i64::from(weapon.max_range)).contains(&distance)
}

impl UnitState<'_> {
    fn poison_in_combat(&self, inflicts: bool) -> Poison {
        Poison {
            active: self.poisoned,
            immune: self.traits.as_ref().is_some_and(|traits| traits.stats.unpoisonable),
            inflicts,
        }
    }
}

pub(crate) fn combatant(
    unit: &UnitState<'_>,
    weapon_index: usize,
    modifiers: &StrikeModifiers,
    opposing_parry: i32,
    role: StrikeRole<'_>,
) -> Combatant {
    let weapon = &unit.unit_type.attacks[weapon_index];
    let factor = match role {
        StrikeRole::Attack { backstab } => {
            (if weapon.has_charge() { 2 } else { 1 }) * (if backstab { 2 } else { 1 })
        }
        StrikeRole::Retaliate(attacking_weapon) if attacking_weapon.has_charge() => 2,
        StrikeRole::Retaliate(_) => 1,
    };
    let base_damage = unit.attack_damage(weapon_index).saturating_mul(factor);
    let damage = |slowed| {
        u32::try_from(strike_damage(base_damage, modifiers.damage_multiplier, slowed)).unwrap_or(u32::MAX)
    };
    let base_chance = base_hit_chance(modifiers.target_hit_chance, weapon.accuracy, opposing_parry);
    let hit_chance = weapon.hit_chance(base_chance, matches!(role, StrikeRole::Attack { .. }));
    Combatant {
        hitpoints: unit.hitpoints.max(0) as u32,
        strikes: weapon.strikes_at_hp(unit.hitpoints, unit.max_hitpoints()),
        damage: damage(false),
        slow_damage: damage(true),
        slows: weapon.has_slow(),
        slowed: unit.slowed,
        unslowable: unit.is_unslowable(),
        petrifies: weapon.has_petrifies(),
        unpetrifiable: unit.is_unpetrifiable(),
        hit_chance,
        firststrike: weapon.has_firststrike(),
        drain_healing_cap: weapon.has_drains().then(|| unit.max_hitpoints().max(0) as u32),
        undrainable: unit.is_undrainable(),
        poison: unit.poison_in_combat(weapon.has_poison()),
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
    fn backstab_doubles_only_the_attack_with_an_enemy_behind_the_defender() {
        let special = r#"[specials]
            [damage]
                id=backstab
                multiply=2
                active_on=offense
                [filter_opponent]
                    formula="
                        enemy_of(self, flanker) and not flanker.petrified
                    where
                        flanker = unit_at(direction_from(loc, other.facing))
                    "
                [/filter_opponent]
            [/damage]
        [/specials]"#;
        let mut types = unit_types();
        let weapon = |special: &str| WeaponProfile::from_wml(
            wml::parse(format!("range=melee\ndamage=3\nnumber=1\n{special}")).unwrap(),
        );
        assert!(!weapon(&special.replace("enemy_of", "allied_with")).supports_specials());
        assert!(weapon(special).supports_specials());
        types.get_mut("Fighter").unwrap().attacks[0] = weapon(special);
        let from = Hex { x: 0, y: 1 };
        let to = Hex { x: 0, y: 2 };
        for (opposite_side, defender_hp) in [
            (None, 7), (Some(2), 7), (Some(1), 4), (Some(3), 4), (Some(4), 7),
        ] {
            let map = ScenarioMap::parse(
                "Xv,Xv,Xv\nXv,Gg,Xv\nXv,1 Gg,Xv\nXv,2 Gg,Xv\nXv,Gg,Xv\nXv,Xv,Xv",
            ).unwrap();
            let scenario = wml::parse("[side]\ntype=Fighter\n[/side]\n[side]\ntype=Fighter\n[/side]").unwrap();
            let starts = read_starting_units(&scenario, &map).unwrap();
            let mut board = Battlefield::new(map, starts, &types).unwrap();
            // A defender with an ally behind the attacker still retaliates for base damage.
            board.place_recruit(2, "Fighter", Hex { x: 0, y: 0 }).unwrap();
            if let Some(side) = opposite_side {
                board.place_recruit(side, "Fighter", Hex { x: 0, y: 3 }).unwrap();
            }
            let result = board.apply_attack_with_relations(
                1, Attack { from, to, ..attack(0) },
                |a, b| a != b && !matches!((a, b), (2, 4) | (4, 2)),
                &mut ReplayRng::new(0),
            ).unwrap();
            assert_eq!((result.attacker_hitpoints, result.defender_hitpoints), (7, defender_hp));
        }
    }

    #[test]
    fn marksman_applies_after_parry_on_the_attack_only() {
        let mut types = unit_types();
        let profile = wml::parse(
            "range=melee\ndamage=1\nnumber=1\nparry=100\n\
             [specials]\n[chance_to_hit]\nid=marksman\nvalue=60\ncumulative=yes\n\
             active_on=offense\n[/chance_to_hit]\n[/specials]",
        ).unwrap();
        types.get_mut("Fighter").unwrap().attacks[0] = WeaponProfile::from_wml(profile);
        let mut board = board(&types);
        let mut rng = ReplayRng::new(5489);
        // Native reference rolls are 12 and 2. The attack has 60% and retaliation has 0%.
        let result = board.apply_attack(1, attack(0), |side| side == 2, &mut rng).unwrap();
        assert_eq!((result.attacker_hitpoints, result.defender_hitpoints), (10, 9));
        assert_eq!(rng.seed_and_calls(), (5489, 2));
    }

    #[test]
    fn slows_on_hit_before_retaliation_and_keeps_both_survivors_slowed() {
        let mut types = unit_types();
        let weapon = WeaponProfile::from_wml(wml::parse(
            "range=melee\ndamage=5\nnumber=1\nmovement_used=2\n\
             [specials]\n[slow]\nid=slow\n[/slow]\n[/specials]",
        ).unwrap());
        assert!(weapon.supports_specials());
        types.get_mut("Fighter").unwrap().attacks[0] = weapon;
        let mut board = board(&types);
        let mut rng = ReplayRng::new(5489);
        board.apply_attack(1, attack(0), |side| side == 2, &mut rng).unwrap();
        let attacker = board.unit_at(ATTACKER).unwrap();
        let defender = board.unit_at(DEFENDER).unwrap();
        assert_eq!((attacker.hitpoints, defender.hitpoints), (7, 5));
        assert_eq!((attacker.moves_left, defender.moves_left), (3, 5));
        assert!(attacker.slowed && defender.slowed);
        assert_eq!(rng.seed_and_calls(), (5489, 2));
    }

    #[test]
    fn swarm_fixes_each_sides_strike_count_from_its_starting_health() {
        let mut types = unit_types();
        let fighter = types.get_mut("Fighter").unwrap();
        fighter.hitpoints = 20;
        fighter.attacks[0] = WeaponProfile::from_wml(wml::parse(
            "range=melee\ndamage=1\nnumber=10\n[specials]\n[swarm]\nid=swarm\n[/swarm]\n[/specials]",
        ).unwrap());
        let swarm = &fighter.attacks[0];
        assert!(swarm.supports_specials());
        assert_eq!(swarm.strikes_at_hp(1, 20), 0);
        assert_eq!(swarm.strikes_at_hp(22, 20), 10);

        let mut board = board(&types);
        board.units.get_mut(&ATTACKER).unwrap().hitpoints = 13;
        board.units.get_mut(&DEFENDER).unwrap().hitpoints = 11;
        let mut rng = ReplayRng::new(5489);
        let result = board.apply_attack(1, attack(0), |side| side == 2, &mut rng).unwrap();
        assert_eq!((result.attacker_hitpoints, result.defender_hitpoints), (8, 5));
        assert_eq!(rng.seed_and_calls(), (5489, 11));
    }

    #[test]
    fn poison_from_attacks_and_replies_ticks_on_the_patients_turn_until_cured() {
        let mut types = unit_types();
        let fighter = types.get_mut("Fighter").unwrap();
        fighter.hitpoints = 30;
        fighter.attacks[0] = WeaponProfile::from_wml(wml::parse(
            "range=melee\ndamage=2\nnumber=1\n[specials]\n[poison]\nid=poison\n[/poison]\n[/specials]",
        ).unwrap());
        assert!(fighter.attacks[0].supports_specials());
        let mut board = board(&types);
        let result = board.apply_attack(1, attack(0), |side| side == 2, &mut ReplayRng::new(5489)).unwrap();
        assert_eq!(result.poisoned, [true, true]);
        let terrain = |heals| crate::terrain_aliases::TerrainAliases::from_wml(&wml::parse(format!(
            "[terrain_type]\nid=keep\nstring=Kh\nheals={heals}\n[/terrain_type]"
        )).unwrap());
        for expected in [20, 14, 8, 2, 1, 1] {
            board.heal_at_turn_start(2, &terrain(0), |a, b| a != b);
            assert_eq!(board.unit_at(DEFENDER).unwrap().hitpoints, expected);
            assert!(board.unit_at(DEFENDER).unwrap().poisoned);
            assert_eq!(board.unit_at(ATTACKER).unwrap().hitpoints, 28);
        }
        board.heal_at_turn_start(2, &terrain(8), |a, b| a != b);
        assert_eq!(board.unit_at(DEFENDER).unwrap().hitpoints, 3);
        assert!(!board.unit_at(DEFENDER).unwrap().poisoned);
        assert!(board.unit_at(ATTACKER).unwrap().poisoned);
    }

    #[test]
    fn stone_strikes_persist_on_survivors_and_respect_authored_immunity() {
        let mut types = unit_types();
        let fighter = types.get_mut("Fighter").unwrap();
        fighter.level = 2;
        fighter.attacks[0] = WeaponProfile::from_wml(wml::parse(
            "range=melee\ndamage=2\nnumber=3\n[specials]\n[petrifies]\n[/petrifies]\n[/specials]",
        ).unwrap());
        assert!(fighter.attacks[0].supports_specials());
        let grant = "[effect]\napply_to=status\nadd=unpetrifiable\n[/effect]";
        let remove = "[effect]\napply_to=status\nremove=unpetrifiable\n[/effect]";
        let trait_effects = |effects| format!("[modifications]\n[trait]\nid=ward\n{effects}\n[/trait]\n[/modifications]");
        for (defender_cfg, immune) in [
            (String::new(), false),
            ("[status]\nunpetrifiable=yes\n[/status]".into(), true),
            (trait_effects(grant.to_string()), true),
            (trait_effects(format!("{grant}\n{remove}")), false),
        ] {
            let map = ScenarioMap::parse("Xv,Xv,Xv,Xv\nXv,1 Kh,2 Kh,Xv\nXv,Xv,Xv,Xv").unwrap();
            let scenario = wml::parse(format!(
                "[side]\ntype=Fighter\n[/side]\n[side]\ntype=Fighter\n{defender_cfg}\n[/side]"
            )).unwrap();
            let starts = crate::starting_units::read_authored_starting_units(&scenario, &map).unwrap();
            let mut board = Battlefield::from_authored_starting_units(map, starts, &types, 100).unwrap();
            let mut rng = ReplayRng::new(5489);
            let result = board.apply_attack(1, attack(0), |side| side == 2, &mut rng).unwrap();
            let attacker = board.unit_at(ATTACKER).unwrap();
            let defender = board.unit_at(DEFENDER).unwrap();
            assert_eq!((attacker.petrified, defender.petrified), (immune, !immune));
            assert_eq!(result.petrified, [immune, !immune]);
            assert_eq!((attacker.hitpoints, defender.hitpoints), (if immune { 8 } else { 10 }, 8));
            assert_eq!((attacker.experience, defender.experience), (2, 2));
            assert_eq!(rng.seed_and_calls(), (5489, if immune { 2 } else { 1 }));
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
        assert_eq!(attacker.experience, 4);
        assert_eq!(rng.seed_and_calls(), (5489, 1));
    }

    #[test]
    fn awards_combat_experience_to_both_survivors_without_retaliation() {
        let mut types = unit_types();
        let fighter = types.get_mut("Fighter").unwrap();
        fighter.level = 2;
        fighter.hitpoints = 20;
        let mut board = board(&types);
        let mut rng = ReplayRng::new(5489);

        let result = board
            .apply_attack(1, Attack { retaliation: None, ..attack(0) }, |side| side == 2, &mut rng)
            .unwrap();
        assert_eq!((result.attacker_hitpoints, result.defender_hitpoints), (20, 10));
        assert_eq!(board.unit_at(ATTACKER).unwrap().experience, 2);
        assert_eq!(board.unit_at(DEFENDER).unwrap().experience, 2);
    }

    #[test]
    fn a_defender_who_kills_gets_kill_experience() {
        let mut types = unit_types();
        types.get_mut("Fighter").unwrap().level = 2;
        let mut board = board(&types);
        let mut rng = ReplayRng::new(5489);

        let mut missed_attack = attack(0);
        missed_attack.modifiers.target_hit_chance = 0;
        let result = board.apply_attack(1, missed_attack, |side| side == 2, &mut rng).unwrap();
        assert_eq!((result.attacker_hitpoints, result.defender_hitpoints), (0, 10));
        assert!(board.unit_at(ATTACKER).is_none());
        assert_eq!(board.unit_at(DEFENDER).unwrap().experience, 16);
    }

    #[test]
    fn a_draining_attack_checks_the_actual_traits_of_an_unarmed_target() {
        for (status_change, expected) in [
            ("add=undrainable,unpoisonable", 3),
            ("remove=undrainable", 7),
        ] {
            let mut types = unit_types();
            let fighter = types.get_mut("Fighter").unwrap();
            fighter.attacks[0] = WeaponProfile::from_wml(
                wml::parse(
                    "range=melee\ndamage=10\nnumber=1\n\
                     [specials]\n[drains]\nid=drains\n[/drains]\n[/specials]",
                )
                .unwrap(),
            );
            let trait_ = wml::parse(format!(
                "id=test\navailability=musthave\n[effect]\napply_to=status\nadd=undrainable\n[/effect]\n\
                 [effect]\napply_to=status\n{status_change}\n[/effect]"
            ))
            .unwrap();
            fighter.traits.available.push(trait_.into());

            let mut board = board(&types);
            let mut rng = ReplayRng::new(5489);
            board.units.get_mut(&ATTACKER).unwrap().hitpoints = 3;
            board.units.remove(&DEFENDER);
            board.place_random_recruit(2, "Fighter", DEFENDER, &mut rng).unwrap();
            board.units.get_mut(&DEFENDER).unwrap().hitpoints = 9;
            let result = board
                .apply_attack(1, Attack { retaliation: None, ..attack(0) }, |side| side == 2, &mut rng)
                .unwrap();
            assert_eq!(result.attacker_hitpoints, expected);
            assert_eq!(result.defender_hitpoints, 0);
            assert_eq!(rng.seed_and_calls(), (5489, 1));
        }
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
        assert_eq!(board.unit_at(ATTACKER).unwrap().experience, 0);
        assert_eq!(board.unit_at(DEFENDER).unwrap().experience, 0);
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
