//! Load a fixed scenario and its rules from a preprocessed WML bundle.

use std::{collections::BTreeMap, error::Error, io};

use crate::{
    fixed_match::{FixedMatch, Rules},
    movement_types::{MovementType, read_movement_types},
    resistance::DamageTable,
    scenario_start,
    terrain_aliases::TerrainAliases,
    unit_type::{UnitType, read_unit_types},
    wml::{self, Node},
};

/// A fixed scenario and the catalogs shared by each match started from it.
pub struct Bundle {
    scenario: Node,
    terrain: TerrainAliases,
    movement: BTreeMap<String, MovementType>,
    resistance: BTreeMap<String, DamageTable>,
    unit_types: BTreeMap<String, UnitType>,
}

impl Bundle {
    /// Parse the terrain definitions, one units block, and one scenario block.
    pub fn parse(bytes: &[u8]) -> Result<Self, Box<dyn Error>> {
        let bundle = wml::parse(bytes)?;
        let terrain = TerrainAliases::from_wml(&bundle);
        let mut units = None;
        let mut scenario = None;
        for (tag, node) in bundle.children {
            let slot = match tag.as_str() {
                "units" => &mut units,
                "scenario" => &mut scenario,
                _ => continue,
            };
            if slot.replace(node).is_some() {
                return Err(format!("expected exactly one [{tag}] block").into());
            }
        }
        let units = units.ok_or("missing [units] block")?;
        let scenario = scenario.ok_or("missing [scenario] block")?;
        let movement = read_movement_types(&units);
        let resistance = read_resistance(&units);
        let unit_types = read_unit_types(units);
        Ok(Self {
            scenario,
            terrain,
            movement,
            resistance,
            unit_types,
        })
    }

    /// Start a fresh match, using the caller's resolver for a named map file.
    pub fn start(
        &self,
        seed: u32,
        read_map_file: impl FnOnce(&str) -> io::Result<String>,
    ) -> Result<FixedMatch<'_>, Box<dyn Error>> {
        let start = scenario_start::load(&self.scenario, &self.unit_types, read_map_file)?;
        let rules = Rules {
            terrain: &self.terrain,
            movement: &self.movement,
            resistance: &self.resistance,
        };
        Ok(FixedMatch::new(start, rules, seed)?)
    }
}

fn read_resistance(units: &Node) -> BTreeMap<String, DamageTable> {
    let mut tables = BTreeMap::new();
    for (tag, node) in &units.children {
        if tag == "movetype" {
            let name = node.attribute("name").unwrap_or_default().to_owned();
            tables.entry(name).or_insert_with(|| {
                node.children
                    .iter()
                    .find(|(tag, _)| tag == "resistance")
                    .map(|(_, table)| DamageTable::from_wml(table))
                    .unwrap_or_default()
            });
        }
    }
    tables
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixed_match::Action;

    #[test]
    fn a_parsed_bundle_starts_independent_matches() {
        let bundle = Bundle::parse(
            br#"
            [units]
                [unit_type]
                    id=Fighter
                    hitpoints=10
                [/unit_type]
            [/units]
            [scenario]
                map_file=arena.map
                [side]
                    side=1
                    type=Fighter
                [/side]
                [side]
                    side=2
                    type=Fighter
                [/side]
            [/scenario]
            "#,
        )
        .unwrap();
        let read_map_file = |name: &str| {
            assert_eq!(name, "arena.map");
            Ok("Xv,Xv,Xv,Xv\nXv,1 Kh,2 Kh,Xv\nXv,Xv,Xv,Xv".to_owned())
        };
        let mut first = bundle.start(5489, read_map_file).unwrap();
        let initial_state = first.state();
        first.step(Action::EndTurn).unwrap();
        let second = bundle.start(5489, read_map_file).unwrap();
        assert_eq!(first.decision().unwrap().0.side, 2);
        assert_eq!(second.decision().unwrap().0.side, 1);
        assert_eq!(second.state(), initial_state);
    }
}
