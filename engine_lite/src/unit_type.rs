use std::collections::BTreeMap;
use std::sync::Arc;

use crate::movement_cost::MovementCosts;
use crate::resistance::DamageTable;
use crate::terrain_defense::TerrainDefense;
use crate::time_of_day::Alignment;
use crate::weapon::WeaponProfile;
use crate::wml::Node;

mod traits;
use traits::TraitCatalog;
pub use traits::TraitPool;

/// Base rules for a preprocessed Wesnoth 1.18.8 `[unit_type]`.
#[derive(Debug, PartialEq, Eq)]
pub struct UnitType {
    pub id: String,
    pub level: i32,
    pub hitpoints: i32,
    pub movement: i32,
    pub max_attacks: i32,
    /// HP restored to adjacent friendly units at the start of this unit's turn.
    pub adjacent_healing: i32,
    /// Base experience requirement before applying the scenario's XP modifier.
    pub experience: i32,
    pub cost: i32,
    pub alignment: Alignment,
    pub movement_type: String,
    /// Terrain costs that this unit type defines over its inherited movement type.
    pub movement_cost_overrides: MovementCosts,
    /// Terrain defense that this unit type defines over its inherited movement type.
    pub defense_overrides: TerrainDefense,
    pub emits_zoc: bool,
    /// HP restored each turn by the stock self-regeneration ability.
    pub regeneration: i32,
    pub(crate) skirmisher: bool,
    pub advances_to: Vec<String>,
    pub attacks: Vec<WeaponProfile>,
    /// Damage percentages that this unit type defines over its inherited movement type.
    pub resistance_overrides: DamageTable,
    pub traits: TraitPool,
}

impl UnitType {
    /// Consumes one flat `[unit_type]` and its local traits.
    pub fn from_wml(unit: Node) -> Self {
        Self::from_catalog(unit, &TraitCatalog::default())
    }

    fn from_catalog(unit: Node, catalog: &TraitCatalog) -> Self {
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
        let defense_overrides = TerrainDefense::from_wml_tables(
            unit.children
                .iter()
                .filter(|(tag, _)| tag == "defense")
                .map(|(_, table)| table),
        );
        let resistance_overrides = DamageTable::from_wml_tables(
            unit.children
                .iter()
                .filter(|(tag, _)| tag == "resistance")
                .map(|(_, table)| table),
        );
        let mut result = Self {
            id: text("id").to_owned(),
            level,
            hitpoints: int("hitpoints", 1),
            movement: int("movement", 1),
            max_attacks: int("attacks", 1),
            adjacent_healing: stock_adjacent_healing(&unit),
            experience: int("experience", 500),
            cost: int("cost", 1),
            alignment,
            movement_type: text("movement_type").to_owned(),
            movement_cost_overrides,
            defense_overrides,
            resistance_overrides,
            emits_zoc,
            regeneration: 0,
            skirmisher: false,
            advances_to,
            attacks: Vec::new(),
            traits: catalog.inherited_traits(&unit, alignment),
        };
        for (tag, child) in unit.children {
            match tag.as_str() {
                "abilities" => {
                    result.skirmisher |= child.children.iter().any(|(tag, config)| {
                        tag == "skirmisher" && is_stock_skirmisher(config)
                    });
                    for (ability, cfg) in &child.children {
                        if ability == "regenerate"
                            && cfg.children.is_empty()
                            && cfg.attribute("id") == Some("regenerates")
                            && cfg.attribute("affect_self") == Some("yes")
                            && cfg.attribute("poison") == Some("cured")
                            && cfg.attributes.keys().all(|k| matches!(k.as_str(),
                                "id" | "value" | "affect_self" | "poison" | "name" |
                                "female_name" | "description" | "special_note"
                            ))
                            && let Some(value) = crate::scenario_time::integer(cfg, "value")
                        {
                            result.regeneration = result.regeneration.max(value);
                        }
                    }
                }
                "attack" => result.attacks.push(WeaponProfile::from_wml(child)),
                "trait" => result.traits.available.push(Arc::new(child)),

                _ => {}
            }
        }
        result
    }
}

fn is_stock_skirmisher(ability: &Node) -> bool {
    ability.attribute("id") == Some("skirmisher")
        && ability.attribute("affect_self") == Some("yes")
        && ability.children.is_empty()
        && ability.attributes.keys().all(|key| matches!(
            key.as_str(),
            "id" | "affect_self" | "name" | "female_name" | "description" | "special_note"
        ))
}

fn stock_adjacent_healing(unit: &Node) -> i32 {
    unit.children
        .iter()
        .filter(|(tag, _)| tag == "abilities")
        .flat_map(|(_, abilities)| &abilities.children)
        .filter_map(|(tag, heal)| {
            if tag != "heals"
                || heal.attribute("id") != Some("healing")
                || heal.attribute("affect_allies") != Some("yes")
                || heal.attribute("affect_self") != Some("no")
                || heal.attribute("poison") != Some("slowed")
                || !heal.attributes.keys().all(|key| {
                    matches!(key.as_str(),
                        "id" | "value" | "affect_allies" | "affect_self" | "poison"
                        | "name" | "female_name" | "description" | "special_note")
                })
            {
                return None;
            }
            let [(tag, adjacent)] = heal.children.as_slice() else {
                return None;
            };
            if tag != "affect_adjacent"
                || !adjacent.attributes.is_empty()
                || !adjacent.children.is_empty()
            {
                return None;
            }
            match heal.attribute("value") {
                Some("4") => Some(4),
                Some("8") => Some(8),
                _ => None,
            }
        })
        .max()
        .unwrap_or(0)
}

/// Indexes flat unit types and resolves their inherited traits from a preprocessed `[units]` body.
///
/// Definitions with a missing or empty ID are ignored. The first definition
/// with a given ID supplies the base rules.
pub fn read_unit_types(units: Node) -> BTreeMap<String, UnitType> {
    let (catalog, unit_definitions) = TraitCatalog::read(units);
    let mut types = BTreeMap::new();
    for unit in unit_definitions {
        let Some(id) = unit.attribute("id").filter(|id| !id.is_empty()) else {
            continue;
        };
        types
            .entry(id.to_owned())
            .or_insert_with(|| UnitType::from_catalog(unit, &catalog));
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
    fn loads_global_racial_and_local_traits_in_native_order() {
        let units = wml::parse(
            r#"
            [trait]
                id=strong
            [/trait]
            [unit_type]
                id=Bowman
                race=human
                alignment=lawful
                [trait]
                    id=veteran
                    availability=musthave
                    require_traits=strong,healthy
                    exclude_traits=quick
                    [effect]
                        apply_to=hitpoints
                        increase_total=2
                    [/effect]
                [/trait]
            [/unit_type]
            [trait]
                id=quick
            [/trait]
            [race]
                id=human
                num_traits=2
                [trait]
                    id=fearless
                [/trait]
                [trait]
                    id=healthy
                [/trait]
            [/race]
            [race]
                id=human
                num_traits=7
            [/race]
            [race]
                id=bat
                num_traits=1
                ignore_global_traits=true
                [trait]
                    id=feral
                [/trait]
            [/race]
            [unit_type]
                id=Neutral
                race=human
                num_traits=1
            [/unit_type]
            [unit_type]
                id=Independent
                race=human
                ignore_race_traits=yes
                [trait]
                    id=loyal
                [/trait]
            [/unit_type]
            [unit_type]
                id=Bat
                race=bat
            [/unit_type]
            [unit_type]
                id=Unknown
                race=unknown
                ignore_race_traits=yes
            [/unit_type]
            "#,
        )
        .unwrap();
        let types = read_unit_types(units);
        for (unit, count, expected) in [
            ("Bowman", 2, &["strong", "quick", "fearless", "healthy", "veteran"][..]),
            ("Neutral", 1, &["strong", "quick", "healthy"][..]),
            ("Independent", 2, &["loyal"][..]),
            ("Bat", 1, &["feral"][..]),
            ("Unknown", 0, &["strong", "quick"][..]),
        ] {
            let traits = &types[unit].traits;
            let ids: Vec<_> = traits
                .available
                .iter()
                .map(|trait_| trait_.attribute("id").unwrap())
                .collect();
            assert_eq!((traits.count, ids.as_slice()), (count, expected), "{unit}");
        }

        let veteran = &types["Bowman"].traits.available[4];
        assert_eq!(veteran.attribute("availability"), Some("musthave"));
        assert_eq!(veteran.attribute("require_traits"), Some("strong,healthy"));
        assert_eq!(veteran.attribute("exclude_traits"), Some("quick"));
        let (tag, effect) = &veteran.children[0];
        assert_eq!(tag, "effect");
        assert_eq!(effect.attribute("increase_total"), Some("2"));
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
