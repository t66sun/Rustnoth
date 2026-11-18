use std::collections::BTreeMap;

use crate::{movement_cost::MovementCosts, terrain_defense::TerrainDefense, wml::Node};

/// Terrain rules inherited from a base `[movetype]` definition.
#[derive(Debug, Default)]
pub struct MovementType {
    pub movement_costs: MovementCosts,
    pub vision_costs: MovementCosts,
    pub defense: TerrainDefense,
}

/// Loads the base `[movetype]` children of a preprocessed `[units]` block.
///
/// Keys match the unit type's `movement_type` attribute. The first definition
/// for each name wins. An unbound unit type can use `MovementType::default()`.
pub fn read_movement_types(units: &Node) -> BTreeMap<String, MovementType> {
    let mut types = BTreeMap::new();
    for (tag, node) in &units.children {
        if tag != "movetype" {
            continue;
        }
        let name = node.attribute("name").unwrap_or_default().to_owned();
        types.entry(name).or_insert_with(|| {
            let table = |name: &str| {
                node.children
                    .iter()
                    .find_map(|(tag, child)| (tag == name).then_some(child))
            };
            MovementType {
                movement_costs: table("movement_costs")
                    .map(MovementCosts::from_wml)
                    .unwrap_or_default(),
                vision_costs: table("vision_costs")
                    .map(MovementCosts::from_wml)
                    .unwrap_or_default(),
                defense: table("defense")
                    .map(TerrainDefense::from_wml)
                    .unwrap_or_default(),
            }
        });
    }
    types
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{terrain_aliases::TerrainAliases, unit_type::UnitType, wml};
    use std::num::NonZeroU32;

    fn flat_terrain() -> TerrainAliases {
        TerrainAliases::from_wml(
            &wml::parse("[terrain_type]\nstring=Gg\nid=flat\n[/terrain_type]").unwrap(),
        )
    }

    #[test]
    fn selects_the_first_named_profile_and_its_first_subtables() {
        let units = wml::parse(
            "[movetype]\nname=foot\n[movement_costs]\nflat=2\n[/movement_costs]\n\
                [movement_costs]\nflat=8\n[/movement_costs]\n\
                [defense]\nflat=55\n[/defense]\n[/movetype]\n\
                [movetype]\nname=mounted\n[movement_costs]\nflat=1\n[/movement_costs]\n[/movetype]\n\
                [movetype]\nname=foot\n[defense]\nflat=80\n[/defense]\n[/movetype]",
        )
        .unwrap();
        let types = read_movement_types(&units);
        let terrain = flat_terrain();
        let unit = UnitType::from_wml(wml::parse("id=infantry\nmovement_type=foot").unwrap());
        let profile = &types[&unit.movement_type];

        assert_eq!(types.len(), 2);
        assert_eq!(
            profile.movement_costs.cost(&terrain, "Gg", false),
            NonZeroU32::new(2)
        );
        assert_eq!(profile.defense.hit_chance(&terrain, "Gg"), 55);
        assert_eq!(
            types["mounted"].movement_costs.cost(&terrain, "Gg", false),
            NonZeroU32::new(1)
        );
    }

    #[test]
    fn missing_tables_use_unreachable_movement_and_default_hit_chance() {
        let units = wml::parse(
            "[movetype]\nname=stationary\n[defense]\nflat=50\n[/defense]\n[/movetype]\n\
                [movetype]\nname=runner\n[movement_costs]\nflat=1\n[/movement_costs]\n[/movetype]",
        )
        .unwrap();
        let types = read_movement_types(&units);
        let terrain = flat_terrain();

        assert_eq!(
            types["stationary"]
                .movement_costs
                .cost(&terrain, "Gg", false),
            None
        );
        assert_eq!(types["stationary"].defense.hit_chance(&terrain, "Gg"), 50);
        assert_eq!(types["runner"].defense.hit_chance(&terrain, "Gg"), 100);
    }
}
