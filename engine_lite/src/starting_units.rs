use std::{collections::HashSet, error, fmt};

use crate::{hex::Hex, income::Upkeep, map::ScenarioMap, scenario_time::integer, wml::Node};

/// A fixed unit placement, in scenario side and creation order.
#[derive(Debug, PartialEq, Eq)]
pub struct StartingUnit {
    pub side: usize,
    pub id: Option<String>,
    pub type_id: String,
    pub position: Hex,
    pub can_recruit: bool,
}

pub(crate) struct AuthoredStartingUnit {
    pub(crate) side: usize,
    pub(crate) id: Option<String>,
    pub(crate) type_id: String,
    /// None places the unit in its side's recall list.
    pub(crate) position: Option<Hex>,
    pub(crate) can_recruit: bool,
    pub(crate) extra_recruits: Vec<String>,
    pub(crate) recall_cost: Option<i32>,
    pub(crate) recall_filter: Option<Node>,
    pub(crate) upkeep: Option<Upkeep>,
    pub(crate) traits: Vec<Node>,
    pub(crate) default_amlas: u8,
    pub(crate) hitpoints: Option<i32>,
    pub(crate) experience: i32,
    pub(crate) moves_left: Option<i32>,
    pub(crate) attacks_left: Option<i32>,
    pub(crate) poisoned: bool,
    pub(crate) petrified: bool,
    pub(crate) unpetrifiable: bool,
    pub(crate) unhealable: bool,
    pub(crate) unplagueable: bool,
    pub(crate) undead_variation: Option<String>,
    pub(crate) slowed: bool,
    pub(crate) uncovered: bool,
}

/// Reads the on-map starting units from an already preprocessed scenario.
///
/// Each side contributes its inline leader, then its `[leader]` children,
/// then its `[unit]` children. The owning side comes from the roster order.
/// Units destined for a side's recall list have no map position and are omitted.
pub fn read_starting_units(
    scenario: &Node,
    map: &ScenarioMap,
) -> Result<Vec<StartingUnit>, PlacementError> {
    Ok(read_authored_starting_units(scenario, map)?
        .into_iter()
        .filter_map(|unit| Some(StartingUnit {
            position: unit.position?,
            side: unit.side,
            id: unit.id,
            type_id: unit.type_id,
            can_recruit: unit.can_recruit,
        }))
        .collect())
}

pub(crate) fn read_authored_starting_units(
    scenario: &Node,
    map: &ScenarioMap,
) -> Result<Vec<AuthoredStartingUnit>, PlacementError> {
    let mut result = Vec::new();
    let mut occupied = HashSet::new();
    for (index, side_cfg) in children_named(scenario, "side").enumerate() {
        let side = index + 1;
        let mut ids = HashSet::new();
        let mut leader_hex = None;
        let inline_leader = (!side_cfg.attribute("no_leader").is_some_and(wml_true))
            .then_some((side_cfg, true));
        let units = inline_leader
            .into_iter()
            .chain(children_named(side_cfg, "leader").map(|node| (node, true)))
            .chain(children_named(side_cfg, "unit").map(|node| (node, false)));
        for (unit, leader) in units {
            let Some(type_id) = nonempty(unit, "type").filter(|&id| id != "null") else {
                continue;
            };
            let id = nonempty(unit, "id");
            if let Some(id) = id
                && !ids.insert(id)
            {
                return Err(PlacementError::RepeatedId {
                    side,
                    id: id.to_owned(),
                });
            }

            let can_recruit = nonempty(unit, "canrecruit").map_or(leader, wml_true);
            let position = fixed_position(unit, map, side, leader, leader_hex, type_id)?;
            if let Some(hex) = position {
                if !occupied.insert(hex) {
                    return Err(PlacementError::Occupied { side, position: hex });
                }
                if can_recruit && leader_hex.is_none() {
                    leader_hex = Some(hex);
                }
            }
            let status = children_named(unit, "status").next();
            let has_status = |name: &str| {
                status.and_then(|node| node.attributes.get(name))
                    .is_some_and(|value| value.translations.is_empty() && wml_true(&value.text))
            };
            result.push(AuthoredStartingUnit {
                side,
                id: id.map(str::to_owned),
                type_id: type_id.to_owned(),
                position,
                can_recruit,
                extra_recruits: crate::scenario_sides::names(unit.attribute("extra_recruit").unwrap_or_default()).collect(),
                // team_builder strips side attributes, including recall_cost, from leaders.
                recall_cost: if leader { None } else { integer(unit, "recall_cost") },
                recall_filter: children_named(unit, "filter_recall").next().cloned(),
                upkeep: Upkeep::from_wml(unit),
                traits: children_named(unit, "modifications")
                    .flat_map(|mods| children_named(mods, "trait")).cloned().collect(),
                default_amlas: children_named(unit, "modifications")
                    .flat_map(|mods| children_named(mods, "advancement"))
                    .filter(|advance| crate::unit_type::is_default_amla(advance)).take(100).count() as u8,
                hitpoints: integer(unit, "hitpoints"),
                experience: integer(unit, "experience").unwrap_or_default(),
                moves_left: integer(unit, "moves"),
                attacks_left: integer(unit, "attacks_left"),
                poisoned: has_status("poisoned"),
                petrified: has_status("petrified"),
                unpetrifiable: has_status("unpetrifiable"),
                unhealable: has_status("unhealable"),
                unplagueable: has_status("unplagueable"),
                undead_variation: unit.attribute("undead_variation").map(str::to_owned),
                slowed: has_status("slowed"),
                uncovered: has_status("uncovered"),
            });
        }
    }
    Ok(result)
}

fn children_named<'a>(node: &'a Node, name: &'a str) -> impl Iterator<Item = &'a Node> {
    node.children
        .iter()
        .filter_map(move |(tag, child)| (tag == name).then_some(child))
}

fn nonempty<'a>(node: &'a Node, name: &str) -> Option<&'a str> {
    node.attribute(name).filter(|value| !value.is_empty())
}

fn wml_true(value: &str) -> bool {
    matches!(value, "yes" | "true")
}

fn fixed_position(
    unit: &Node,
    map: &ScenarioMap,
    side: usize,
    leader: bool,
    leader_hex: Option<Hex>,
    type_id: &str,
) -> Result<Option<Hex>, PlacementError> {
    let placement = nonempty(unit, "placement").unwrap_or(if leader { "map,leader" } else { "" });
    let places = placement.split(',').map(str::trim).filter(|s| !s.is_empty());
    for place in places.chain(std::iter::once("map")) {
        let candidate = match place {
            "map" => map_target(unit, map),
            "leader" => leader_hex.or_else(|| map.special_location(&side.to_string())),
            "recall" => return Ok(None),
            _ => {
                return Err(PlacementError::Unsupported {
                    side,
                    directive: place.to_owned(),
                });
            }
        };
        if let Some(position) = candidate.filter(|&hex| map.is_playable(hex)) {
            if unit.attribute("passable").is_some_and(wml_true) {
                return Err(PlacementError::Unsupported {
                    side,
                    directive: "passable=yes".to_owned(),
                });
            }
            return Ok(Some(position));
        }
    }
    if ["x", "y"].iter().all(|key| unit.attribute(key).map(str::trim) == Some("recall")) {
        return Ok(None);
    }
    Err(PlacementError::NoPlayableLocation {
        side,
        type_id: type_id.to_owned(),
    })
}

fn map_target(unit: &Node, map: &ScenarioMap) -> Option<Hex> {
    let marker = nonempty(unit, "location_id")
        .and_then(|id| map.special_location(id))
        .filter(|hex| hex.x >= 0 && hex.y >= 0);
    marker.or_else(|| {
        let x = unit.attribute("x")?.trim().parse().ok()?;
        let y = unit.attribute("y")?.trim().parse().ok()?;
        Hex::from_wml(x, y)
    })
}

#[derive(Debug, PartialEq, Eq)]
pub enum PlacementError {
    Unsupported { side: usize, directive: String },
    NoPlayableLocation { side: usize, type_id: String },
    Occupied { side: usize, position: Hex },
    RepeatedId { side: usize, id: String },
}

impl fmt::Display for PlacementError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported { side, directive } => {
                write!(f, "side {side} needs placement directive {directive}")
            }
            Self::NoPlayableLocation { side, type_id } => {
                write!(f, "side {side} has no playable placement for {type_id}")
            }
            Self::Occupied { side, position } => {
                write!(
                    f,
                    "side {side} needs to place a unit on occupied hex {position:?}"
                )
            }
            Self::RepeatedId { side, id } => {
                write!(
                    f,
                    "side {side} needs recall handling for repeated unit ID {id}"
                )
            }
        }
    }
}

impl error::Error for PlacementError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wml;

    fn map() -> ScenarioMap {
        ScenarioMap::parse(
            "Xv,Xv,Xv,Xv,Xv\nXv,1 Kh,post Gg,Gg,Xv\nXv,Gg,Gg,2 Kh,Xv\nXv,Xv,Xv,Xv,Xv",
        )
        .unwrap()
    }

    #[test]
    fn places_inline_and_child_leaders_before_units_in_roster_order() {
        let scenario = wml::parse(
            r#"
            [side]
                side=1
                id=hero
                type=Lieutenant
                [unit]
                    type=Spearman
                    x,y=3,1
                    side=2
                [/unit]
                [leader]
                    id=ally
                    type=Swordsman
                    x,y=1,2
                    location_id=post
                    canrecruit=no
                [/leader]
            [/side]
            [side]
                side=2
                type=Orcish Warrior
                x,y=2,2
            [/side]
            "#,
        )
        .unwrap();
        let placed = read_starting_units(&scenario, &map()).unwrap();
        let placements: Vec<_> = placed
            .iter()
            .map(|unit| {
                (
                    unit.side,
                    unit.id.as_deref(),
                    unit.type_id.as_str(),
                    unit.position.to_wml().unwrap(),
                    unit.can_recruit,
                )
            })
            .collect();
        assert_eq!(
            placements,
            [
                (1, Some("hero"), "Lieutenant", (1, 1), true),
                (1, Some("ally"), "Swordsman", (2, 1), false),
                (1, None, "Spearman", (3, 1), false),
                (2, None, "Orcish Warrior", (2, 2), true),
            ]
        );
    }

    #[test]
    fn requires_playable_unoccupied_fixed_positions() {
        let scenario = |xy| {
            wml::parse(format!(
                "[side]\ntype=Lieutenant\n[unit]\ntype=Spearman\nx,y={xy}\n[/unit]\n[/side]",
            ))
            .unwrap()
        };
        assert_eq!(
            read_starting_units(&scenario("1,1"), &map()),
            Err(PlacementError::Occupied {
                side: 1,
                position: Hex::from_wml(1, 1).unwrap(),
            })
        );
        assert_eq!(
            read_starting_units(&scenario("0,1"), &map()),
            Err(PlacementError::NoPlayableLocation {
                side: 1,
                type_id: "Spearman".to_owned(),
            })
        );
    }

    #[test]
    fn tries_placement_directives_in_order_before_storing_a_reserve() {
        let scenario = |placement: &str, coordinates: &str| wml::parse(format!(
            "[side]\n[unit]\ntype=Spearman\nplacement={placement}\nx,y={coordinates}\n[/unit]\n[/side]",
        )).unwrap();
        for (placement, coordinates) in [("recall,map", "1,1"), ("map,recall", "0,1"), ("map", "recall,recall")] {
            assert!(read_starting_units(&scenario(placement, coordinates), &map()).unwrap().is_empty());
        }
        assert_eq!(read_starting_units(&scenario("map,recall", "1,1"), &map()).unwrap().len(), 1);
        assert_eq!(
            read_starting_units(&scenario("map_passable,recall", "1,1"), &map()),
            Err(PlacementError::Unsupported {
                side: 1,
                directive: "map_passable".to_owned(),
            })
        );
    }

    #[test]
    fn no_leader_disables_the_inline_type_but_keeps_authored_units() {
        let scenario = wml::parse(
            "[side]\nno_leader=yes\ntype=Lieutenant\n\
             [unit]\ntype=Spearman\nx,y=1,1\n[/unit]\n[/side]",
        )
        .unwrap();
        let units = read_starting_units(&scenario, &map()).unwrap();
        assert_eq!(units.len(), 1);
        assert_eq!(units[0].type_id, "Spearman");
        assert!(!units[0].can_recruit);
    }
}
