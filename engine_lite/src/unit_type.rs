use std::collections::BTreeMap;

use crate::movement_cost::MovementCosts;
use crate::time_of_day::Alignment;
use crate::weapon::WeaponProfile;
use crate::wml::Node;

/// Base rules for a preprocessed Wesnoth 1.18.8 `[unit_type]`.
#[derive(Debug, PartialEq, Eq)]
pub struct UnitType {
    pub id: String,
    pub level: i32,
    pub hitpoints: i32,
    pub movement: i32,
    pub max_attacks: i32,
    /// Base experience requirement before applying the scenario's XP modifier.
    pub experience: i32,
    pub cost: i32,
    pub alignment: Alignment,
    pub movement_type: String,
    /// Terrain costs that this unit type defines over its inherited movement type.
    pub movement_cost_overrides: MovementCosts,
    pub emits_zoc: bool,
    pub advances_to: Vec<String>,
    pub attacks: Vec<WeaponProfile>,
}

impl UnitType {
    /// Consumes one flat `[unit_type]` after any inheritance has been resolved.
    pub fn from_wml(unit: Node) -> Self {
        let text = |key| unit.attribute(key).unwrap_or_default();
        let int = |key, default| {
            unit
                .attributes
                .get(key)
                .filter(|value| value.translations.is_empty())
                .and_then(|value| value.text.parse().ok())
                .unwrap_or(default)
        };
        let level = int("level", 0);
        let emits_zoc = unit
            .attributes
            .get("zoc")
            .filter(|value| value.translations.is_empty())
            .and_then(|value| match value.text.as_str() {
                "yes" | "true" => Some(true),
                "no" | "false" => Some(false),
                _ => None,
            })
            .unwrap_or(level > 0);
        let alignment = match text("alignment") {
            "lawful" => Alignment::Lawful,
            "chaotic" => Alignment::Chaotic,
            "liminal" => Alignment::Liminal,
            _ => Alignment::Neutral,
        };
        let advances_to = match text("advances_to") {
            "null" => Vec::new(),
            targets => targets
                .split(',')
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .map(str::to_owned)
                .collect(),
        };
        let movement_cost_overrides = MovementCosts::from_wml_tables(
            unit.children
                .iter()
                .filter(|(tag, _)| tag == "movement_costs")
                .map(|(_, table)| table),
        );
        Self {
            id: text("id").to_owned(),
            level,
            hitpoints: int("hitpoints", 1),
            movement: int("movement", 1),
            max_attacks: int("attacks", 1),
            experience: int("experience", 500),
            cost: int("cost", 1),
            alignment,
            movement_type: text("movement_type").to_owned(),
            movement_cost_overrides,
            emits_zoc,
            advances_to,
            attacks: unit
                .children
                .into_iter()
                .filter(|(tag, _)| tag == "attack")
                .map(|(_, child)| WeaponProfile::from_wml(child))
                .collect(),
        }
    }
}

/// Indexes direct, already-flat `[unit_type]` definitions from a preprocessed `[units]` body.
///
/// Definitions with a missing or empty ID are ignored. The first definition
/// with a given ID supplies the base rules.
pub fn read_unit_types(units: Node) -> BTreeMap<String, UnitType> {
    let mut types = BTreeMap::new();
    for (tag, unit) in units.children {
        if tag != "unit_type" {
            continue;
        }
        let Some(id) = unit.attribute("id").filter(|id| !id.is_empty()) else {
            continue;
        };
        types
            .entry(id.to_owned())
            .or_insert_with(|| UnitType::from_wml(unit));
    }
    types
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wml;

    #[test]
    fn indexes_flat_unit_types_by_id_and_keeps_the_first_definition() {
        let units = wml::parse(
            r#"
            [unit_type]
                id=Spearman
                hitpoints=36
                movement_type=smallfoot
                [attack]
                    name=spear
                [/attack]
            [/unit_type]
            [unit_type]
                id=Elvish Scout
                movement=9
            [/unit_type]
            [unit_type]
                id=Spearman
                hitpoints=99
            [/unit_type]
            [unit_type]
                id=""
                hitpoints=50
            [/unit_type]
            [unit_type]
                hitpoints=100
            [/unit_type]
            [race]
                id=elf
            [/race]
            "#,
        )
        .unwrap();
        let types = read_unit_types(units);

        assert_eq!(
            types.keys().map(String::as_str).collect::<Vec<_>>(),
            ["Elvish Scout", "Spearman"]
        );
        assert_eq!(types["Elvish Scout"].movement, 9);
        let spearman = &types["Spearman"];
        assert_eq!(spearman.hitpoints, 36);
        assert_eq!(spearman.movement_type, "smallfoot");
        assert_eq!(spearman.attacks[0].id, "spear");
    }

    #[test]
    fn loads_spearman_base_stats_and_ordered_attacks() {
        // The rules fields of the 1.18.8 Loyalist Spearman.
        let data = wml::parse(
            r#"
            [unit_type]
                id=Spearman
                hitpoints=36
                movement_type=smallfoot
                movement=5
                experience=42
                level=1
                alignment=lawful
                advances_to=Swordsman, Pikeman, Javelineer
                cost=14
                [attack]
                    name=spear
                    type=pierce
                    range=melee
                    damage=7
                    number=3
                [/attack]
                [attack_anim]
                [/attack_anim]
                [attack]
                    name=javelin
                    type=pierce
                    range=ranged
                    damage=6
                    number=1
                [/attack]
            [/unit_type]
            "#,
        )
        .unwrap();
        let (_, config) = data.children.into_iter().next().unwrap();
        let unit = UnitType::from_wml(config);

        assert_eq!(unit.id, "Spearman");
        assert_eq!((unit.level, unit.hitpoints, unit.movement), (1, 36, 5));
        assert_eq!((unit.max_attacks, unit.experience, unit.cost), (1, 42, 14));
        assert_eq!(unit.alignment, Alignment::Lawful);
        assert_eq!(unit.movement_type, "smallfoot");
        assert!(unit.emits_zoc);
        assert_eq!(unit.advances_to, ["Swordsman", "Pikeman", "Javelineer"]);
        let attacks: Vec<_> = unit
            .attacks
            .iter()
            .map(|weapon| (weapon.id.as_str(), weapon.damage, weapon.strikes))
            .collect();
        assert_eq!(attacks, [("spear", 7, 3), ("javelin", 6, 1)]);
    }

    #[test]
    fn defaults_missing_stats_and_applies_explicit_zone_of_control() {
        let minimal =
            UnitType::from_wml(wml::parse("alignment=unrecognized\nadvances_to=null\n").unwrap());
        assert_eq!(
            (minimal.level, minimal.hitpoints, minimal.movement),
            (0, 1, 1)
        );
        assert_eq!(
            (minimal.max_attacks, minimal.experience, minimal.cost),
            (1, 500, 1)
        );
        assert_eq!(minimal.alignment, Alignment::Neutral);
        assert!(!minimal.emits_zoc);
        assert!(minimal.advances_to.is_empty());
        assert!(minimal.attacks.is_empty());

        let enabled = UnitType::from_wml(wml::parse("level=0\nzoc=yes\nattacks=2\n").unwrap());
        assert!(enabled.emits_zoc);
        assert_eq!(enabled.max_attacks, 2);
        let disabled = UnitType::from_wml(wml::parse("level=1\nzoc=false\n").unwrap());
        assert!(!disabled.emits_zoc);
    }
}
