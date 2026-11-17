//! Apply ordinary recruit trait effects to base unit statistics.

use crate::{scenario_time::integer, unit_type::UnitType, wml::Node};

/// A recruit's effective maximum stats and weapon damage.
#[derive(Debug, PartialEq, Eq)]
pub struct RecruitStats {
    pub max_hitpoints: i32,
    pub max_movement: i32,
    pub max_experience: i32,
    /// Damage indexed in the same order as the unit type's attacks.
    pub attack_damage: Vec<i32>,
}

impl RecruitStats {
    /// Applies direct HP, movement, experience and attack damage effects in trait order.
    pub fn from_traits(unit: &UnitType, traits: &[&Node]) -> Self {
        Self::from_traits_with_base_experience(unit, traits, unit.experience)
    }

    pub(crate) fn from_traits_with_base_experience(
        unit: &UnitType,
        traits: &[&Node],
        base_max_experience: i32,
    ) -> Self {
        let mut stats = Self {
            max_hitpoints: unit.hitpoints,
            max_movement: unit.movement,
            max_experience: base_max_experience,
            attack_damage: unit.attacks.iter().map(|attack| attack.damage).collect(),
        };
        for trait_ in traits {
            for (tag, effect) in &trait_.children {
                if tag != "effect" || !effect.children.is_empty() {
                    continue;
                }
                let repetitions = if effect.attribute("times") == Some("per level") {
                    unit.level
                } else {
                    integer(effect, "times").unwrap_or(1)
                };
                for _ in 0..repetitions {
                    stats.apply_effect(unit, effect);
                }
            }
        }
        stats
    }

    fn apply_effect(&mut self, unit: &UnitType, effect: &Node) {
        match effect.attribute("apply_to") {
            Some("hitpoints") => {
                let base = set_relative(self.max_hitpoints, effect, "set_total");
                self.max_hitpoints = increase(base, effect, "increase_total", 1).max(1);
            }
            Some("movement") => {
                let increased = increase(self.max_movement, effect, "increase", 1);
                self.max_movement = integer(effect, "set").unwrap_or(increased);
            }
            Some("max_experience") => {
                let base = set_relative(self.max_experience, effect, "set");
                self.max_experience = increase(base, effect, "increase", 1);
            }
            Some("attack") => {
                for (weapon, damage) in unit.attacks.iter().zip(&mut self.attack_damage) {
                    if matches_filter(effect, "range", &weapon.range)
                        && matches_filter(effect, "name", &weapon.id)
                        && matches_filter(effect, "type", &weapon.damage_type)
                    {
                        *damage = increase(*damage, effect, "increase_damage", 0);
                    }
                }
            }
            _ => {}
        }
    }
}

fn set_relative(current: i32, effect: &Node, key: &str) -> i32 {
    let Some(amount) = integer(effect, key) else {
        return current;
    };
    if effect.attribute(key).is_some_and(|value| value.ends_with('%')) {
        let value = i64::from(current) * i64::from(amount) / 100;
        value.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
    } else {
        amount
    }
}

fn increase(current: i32, effect: &Node, key: &str, minimum: i32) -> i32 {
    let Some(raw) = effect.attribute(key).filter(|value| !value.is_empty()) else {
        return current;
    };
    let mut amount = i64::from(integer(effect, key).unwrap_or_default());
    if raw.ends_with('%') {
        let product = i64::from(current) * amount;
        // Wesnoth's div100rounded rounds halfway values away from zero.
        amount = (product + 50 * product.signum()) / 100;
    }
    (i64::from(current) + amount).clamp(i64::from(minimum), i64::from(i32::MAX)) as i32
}

fn matches_filter(effect: &Node, key: &str, actual: &str) -> bool {
    let mut allowed = effect
        .attribute(key)
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .peekable();
    allowed.peek().is_none() || allowed.any(|value| value == actual)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wml;

    #[test]
    fn calculates_native_bowman_stats_from_selected_traits() {
        let unit = UnitType::from_wml(
            wml::parse(
                r#"
                id=Bowman
                level=1
                hitpoints=33
                movement=5
                experience=35
                [attack]
                    name=sword
                    type=blade
                    range=melee
                    damage=5
                [/attack]
                [attack]
                    name=bow
                    type=pierce
                    range=ranged
                    damage=6
                [/attack]
                "#,
            )
            .unwrap(),
        );
        let resilient = wml::parse(
            "[effect]\napply_to=hitpoints\nincrease_total=4\n[/effect]\n\
             [effect]\napply_to=hitpoints\ntimes=per level\nincrease_total=1\n[/effect]",
        )
        .unwrap();
        let quick = wml::parse(
            "[effect]\napply_to=movement\nincrease=1\n[/effect]\n\
             [effect]\napply_to=hitpoints\nincrease_total=-5%\n[/effect]",
        )
        .unwrap();
        let strong = wml::parse(
            "[effect]\napply_to=attack\nrange=melee\nincrease_damage=1\n[/effect]\n\
             [effect]\napply_to=hitpoints\nincrease_total=1\n[/effect]",
        )
        .unwrap();
        let intelligent =
            wml::parse("[effect]\napply_to=max_experience\nincrease=-20%\n[/effect]").unwrap();

        let durable = RecruitStats::from_traits(&unit, &[&resilient, &strong]);
        assert_eq!((durable.max_hitpoints, durable.max_movement), (39, 5));
        assert_eq!(durable.attack_damage, [6, 6]);
        let mobile = RecruitStats::from_traits(&unit, &[&quick, &strong]);
        assert_eq!((mobile.max_hitpoints, mobile.max_movement), (32, 6));
        assert_eq!(mobile.attack_damage, [6, 6]);
        assert_eq!(
            RecruitStats::from_traits(&unit, &[&intelligent]).max_experience,
            28
        );
    }

    #[test]
    fn combines_set_and_increase_in_native_order_and_uses_all_weapon_filters() {
        let unit = UnitType::from_wml(
            wml::parse(concat!(
                "level=2\nhitpoints=10\nmovement=5\nexperience=35\n",
                "[attack]\nname=sword\ntype=blade\nrange=melee\ndamage=5\n[/attack]\n",
                "[attack]\nname=knife\ntype=blade\nrange=melee\ndamage=5\n[/attack]\n",
                "[attack]\nname=sword\ntype=pierce\nrange=melee\ndamage=5\n[/attack]\n",
            ))
            .unwrap(),
        );
        let trait_ = wml::parse(
            r#"
            [effect]
                apply_to=hitpoints
                times=per level
                set_total=50%
                increase_total=1
            [/effect]
            [effect]
                apply_to=movement
                increase=3
                set=7
            [/effect]
            [effect]
                apply_to=max_experience
                set=50%
                increase=-50%
            [/effect]
            [effect]
                apply_to=attack
                name=axe,sword
                type=blade
                range=melee
                increase_damage=10%
            [/effect]
            "#,
        )
        .unwrap();

        let stats = RecruitStats::from_traits(&unit, &[&trait_]);
        assert_eq!(
            (stats.max_hitpoints, stats.max_movement, stats.max_experience),
            (4, 7, 8)
        );
        assert_eq!(stats.attack_damage, [6, 5, 5]);
    }
}
