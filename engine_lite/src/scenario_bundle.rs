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
        Self::parse_bundle(bytes, None)
    }

    /// Load the original preprocessed core catalogs and a scenario with optional custom definitions.
    /// Core terrain codes, movetype names and unit type IDs take precedence.
    pub fn parse_with_core(scenario_bytes: &[u8], core_bytes: &[u8]) -> Result<Self, Box<dyn Error>> {
        Self::parse_bundle(scenario_bytes, Some(core_bytes))
    }

    fn parse_bundle(bytes: &[u8], core_bytes: Option<&[u8]>) -> Result<Self, Box<dyn Error>> {
        let mut bundle = wml::parse(bytes)?;
        let units = take_block(&mut bundle, "units")?;
        let scenario = take_block(&mut bundle, "scenario")?;
        let (config, units) = if let Some(bytes) = core_bytes {
            let mut core = wml::parse(bytes)?;
            let mut core_units =
                take_block(&mut core, "units")?.ok_or("missing [units] block in core catalog")?;
            core.children.extend(bundle.children);
            if let Some(units) = units {
                core_units.children.extend(units.children);
            }
            (core, core_units)
        } else {
            (bundle, units.ok_or("missing [units] block")?)
        };
        Ok(Self {
            scenario: scenario.ok_or("missing [scenario] block")?,
            terrain: TerrainAliases::from_wml(&config),
            movement: read_movement_types(&units),
            resistance: read_resistance(&units),
            unit_types: read_unit_types(units),
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

fn take_block(config: &mut Node, tag: &str) -> Result<Option<Node>, Box<dyn Error>> {
    let mut matching = config
        .children
        .iter()
        .enumerate()
        .filter(|(_, (name, _))| name == tag);
    let Some((index, _)) = matching.next() else {
        return Ok(None);
    };
    if matching.next().is_some() {
        return Err(format!("expected exactly one [{tag}] block").into());
    }
    Ok(Some(config.children.remove(index).1))
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
    fn campaign_replaces_the_global_schedule_at_the_active_round() {
        let config = b"[units]\n[unit_type]\nid=Guard\nhitpoints=24\n[/unit_type]\n[/units]\n\
            [scenario]\nmap_file=test.map\n[side]\ntype=Guard\n[/side]\n[side]\ntype=Guard\n[/side]\n\
            [time]\nid=day\n[/time]\n[event]\nname=turn 2\n[replace_schedule]\ncurrent_time=1\n\
            [time]\nid=dusk\n[/time]\n[time]\nid=night\nlawful_bonus=-25\n[/time]\n[/replace_schedule]\n[/event]\n[/scenario]";
        let bundle = Bundle::parse(config).unwrap();
        let mut game = bundle.start(1, |_| Ok("Xv,Xv,Xv,Xv\nXv,1 Gg,2 Gg,Xv\nXv,Xv,Xv,Xv".into())).unwrap();
        assert_eq!(game.global_time_of_day().unwrap().id, "day");
        for _ in 0..2 { game.step(Action::EndTurn).unwrap(); }
        let night = game.global_time_of_day().unwrap();
        assert_eq!((night.id.as_str(), night.lawful_bonus), ("night", -25));
        for _ in 0..2 { game.step(Action::EndTurn).unwrap(); }
        assert_eq!(game.global_time_of_day().unwrap().id, "dusk");
    }

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

    #[test]
    fn loads_core_and_adds_custom_catalog_entries_with_core_precedence() {
        let core = br#"
            [terrain_type]
                string=Gg
                id=flat
            [/terrain_type]
            [units]
                [movetype]
                    name=smallfoot
                    [movement_costs]
                        flat=1
                    [/movement_costs]
                    [resistance]
                        blade=90
                    [/resistance]
                [/movetype]
                [unit_type]
                    id=Scout
                    hitpoints=30
                    movement_type=smallfoot
                [/unit_type]
            [/units]
            [scenario]
                id=ignored
            [/scenario]
        "#;
        let mut scenario = br#"
            [scenario]
                map_data="Xv,Xv,Xv,Xv
Xv,1 Kh,2 Kh,Xv
Xv,Xv,Xv,Xv"
                [side]
                    side=1
                    type=Scout
                [/side]
                [side]
                    side=2
                    type=Scout
                [/side]
            [/scenario]
        "#
        .to_vec();
        assert!(Bundle::parse(&scenario).is_err());
        let stock = Bundle::parse_with_core(&scenario, core).unwrap();
        assert!(stock.start(5489, |_| unreachable!()).is_ok());

        scenario.extend_from_slice(
            br#"
            [terrain_type]
                string=Gg
                id=custom_flat
            [/terrain_type]
            [terrain_type]
                string=Qp
                id=custom_path
            [/terrain_type]
            [units]
                [movetype]
                    name=smallfoot
                    [movement_costs]
                        flat=7
                    [/movement_costs]
                    [resistance]
                        blade=30
                    [/resistance]
                [/movetype]
                [movetype]
                    name=customfoot
                    [movement_costs]
                        custom_path=2
                    [/movement_costs]
                [/movetype]
                [unit_type]
                    id=Scout
                    hitpoints=12
                [/unit_type]
                [unit_type]
                    id=Runner
                    movement_type=customfoot
                [/unit_type]
            [/units]
        "#,
        );
        let mixed = Bundle::parse_with_core(&scenario, core).unwrap();
        assert_eq!(mixed.terrain.terrain_id("Gg"), Some("flat"));
        assert_eq!(mixed.terrain.terrain_id("Qp"), Some("custom_path"));
        assert_eq!(mixed.unit_types["Scout"].hitpoints, 30);
        assert_eq!(mixed.unit_types["Runner"].movement_type, "customfoot");
        let move_cost = |name: &str, code| {
            mixed.movement[name]
                .movement_costs
                .cost(&mixed.terrain, code, false)
                .map(|cost| cost.get())
        };
        assert_eq!(move_cost("smallfoot", "Gg"), Some(1));
        assert_eq!(move_cost("customfoot", "Qp"), Some(2));
        assert_eq!(mixed.resistance["smallfoot"].damage_taken("blade"), 90);
    }
}
