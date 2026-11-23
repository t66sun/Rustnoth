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
        let mut scenario = take_block(&mut bundle, "scenario")?.ok_or("missing [scenario] block")?;
        if let Some(mode) = bundle.attributes.remove("random_mode") {
            scenario.attributes.insert("random_mode".into(), mode);
        }
        let (config, mut units) = if let Some(bytes) = core_bytes {
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
        crate::unit_type::resolve_base_units(&mut units)?;
        Ok(Self {
            scenario,
            terrain: TerrainAliases::from_wml(&config),
            movement: read_movement_types(&units),
            resistance: read_resistance(&units),
            unit_types: read_unit_types(units),
        })
    }

    /// Public unit-type data used by the recruit preview for a legal recruit.
    pub fn unit_type(&self, id: &str) -> Option<&UnitType> {
        self.unit_types.get(id)
    }

    /// Public unit-type help uses the same terrain and resistance tables as combat.
    pub fn unit_resistances<'a>(&'a self, unit: &'a UnitType) -> BTreeMap<&'a str, i32> {
        let base = self.resistance.get(&unit.movement_type);
        unit.resistance_overrides.damage_types().chain(base.into_iter().flat_map(DamageTable::damage_types))
            .map(|kind| (kind, 100 - unit.resistance_overrides.damage_taken_with_fallback(kind, base)))
            .collect()
    }

    pub fn unit_terrain_modifiers(&self, unit: &UnitType, terrain: &str) -> (u8, Option<u32>) {
        let base = self.movement.get(&unit.movement_type);
        let hit = unit.defense_overrides.hit_chance_with_fallback(&self.terrain, terrain, base.map(|m| &m.defense));
        let cost = unit.movement_cost_overrides.cost_with_fallback(&self.terrain, terrain, false, base.map(|m| &m.movement_costs));
        (100 - hit, cost.map(|n| n.get()))
    }

    /// Start a fresh match, using the caller's resolver for a named map file.
    pub fn start(
        &self,
        seed: u32,
        read_map_file: impl FnOnce(&str) -> io::Result<String>,
    ) -> Result<FixedMatch<'_>, Box<dyn Error>> {
        self.start_campaign(seed, read_map_file, None)
    }

    /// Restore survivors before opening events, consuming their records once the match starts.
    pub fn start_campaign(
        &self,
        seed: u32,
        read_map_file: impl FnOnce(&str) -> io::Result<String>,
        carryover: Option<&mut crate::fixed_match::carryover::CampaignCarryover>,
    ) -> Result<FixedMatch<'_>, Box<dyn Error>> {
        self.start_with_controllers(seed, read_map_file, carryover, false)
    }

    /// Single-player campaigns follow WML controller changes. Hotseat and fixed
    /// matches supply their own players, including when restoring older replays.
    pub fn start_with_controllers(
        &self,
        seed: u32,
        read_map_file: impl FnOnce(&str) -> io::Result<String>,
        carryover: Option<&mut crate::fixed_match::carryover::CampaignCarryover>,
        skip_null_controllers: bool,
    ) -> Result<FixedMatch<'_>, Box<dyn Error>> {
        let mut start = scenario_start::load(&self.scenario, &self.unit_types, read_map_file)?;
        start.skip_null_controllers = skip_null_controllers;
        let claimed = match &carryover {
            Some(route) => route.restore_units(&self.scenario, &mut start.board)?,
            None => Default::default(),
        };
        let rules = Rules {
            terrain: &self.terrain,
            movement: &self.movement,
            resistance: &self.resistance,
        };
        let game = FixedMatch::new(start, rules, seed)?;
        if let Some(route) = carryover { route.finish_transfer(claimed); }
        Ok(game)
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
    fn campaign_skips_null_turns_after_events_and_wraps_at_the_last_playing_side() {
        use crate::{fixed_match::MatchState, turn::Turn};
        let side = |n, controller| format!("[side]\nside={n}\ncontroller={controller}\ndefeat_condition=never\ngold=0\n[/side]\n");
        let sides = [side(1, "null"), side(2, "human"), side(3, "null"), side(4, "null"),
                     side(5, "ai"), side(6, "null")].concat();
        let events = r#"
[event]
name=side 5 turn 1 end
[modify_side]
side=6
controller=ai
[/modify_side]
[/event]
[event]
name=turn 1 end
[gold]
side=5
amount=7
[/gold]
[/event]
[event]
name=side 1 turn,side 3 turn,side 4 turn,side 1 turn end,side 3 turn refresh
[gold]
side=1
amount=100
[/gold]
[/event]
"#;
        let source = format!("[units]\n[/units]\n[scenario]\nturns=2\nmap_data=\"Xv,Xv,Xv\nXv,Gg,Xv\nXv,Xv,Xv\"\n{sides}{events}\n[/scenario]");
        let bundle = Bundle::parse(source.as_bytes()).unwrap();
        let playing = |round, side| MatchState::Playing(Turn { round, side });
        let start = |controllers| bundle.start_with_controllers(7, |_| unreachable!(), None, controllers).unwrap();
        assert_eq!(start(false).state(), playing(1, 1)); // standalone hotseat
        let mut game = start(true);
        assert_eq!(game.state(), playing(1, 2));
        assert_eq!(game.step(Action::EndTurn).unwrap(), playing(1, 5));
        // The final dormant side was activated at side-turn end. Round end waits for it.
        assert_eq!(game.step(Action::EndTurn).unwrap(), playing(1, 6));
        assert_eq!(game.audit_snapshot().gold[4], 0);
        assert_eq!(game.step(Action::EndTurn).unwrap(), playing(2, 2));
        assert_eq!(game.audit_snapshot().gold, [0, 2, 0, 0, 7, 0]);
        assert_eq!(game.step(Action::EndTurn).unwrap(), playing(2, 5));
        assert_eq!(game.step(Action::EndTurn).unwrap(), playing(2, 6));
        assert_eq!(game.step(Action::EndTurn).unwrap(), MatchState::Draw);
        assert_eq!(game.audit_snapshot().gold, [0, 2, 0, 0, 9, 2]);
    }

    #[test]
    fn all_null_campaigns_require_a_playing_side_unless_the_opening_already_ended() {
        use crate::fixed_match::{MatchError, MatchState};
        let source = "[units]\n[/units]\n[scenario]\nmap_data=\"Xv,Xv,Xv\nXv,Gg,Xv\nXv,Xv,Xv\"\n";
        let sides = "[side]\ncontroller=null\ndefeat_condition=never\n[/side]\n".repeat(2);
        let read = |event| Bundle::parse(format!("{source}{sides}{event}\n[/scenario]").as_bytes()).unwrap();
        let empty = read("");
        let error = empty.start_with_controllers(7, |_| unreachable!(), None, true).err().unwrap();
        assert_eq!(error.downcast_ref::<MatchError>(), Some(&MatchError::EmptyTurnController));
        let ended = read("[event]\nname=start\n[endlevel]\nresult=victory\n[/endlevel]\n[/event]");
        assert_eq!(ended.start_with_controllers(7, |_| unreachable!(), None, true).unwrap().state(), MatchState::CampaignVictory);
    }

    #[test]
    fn replaying_the_saved_campaign_classification_reproduces_biased_strikes() {
        use crate::{board::AttackChoice, hex::Hex};
        let core = b"[terrain_type]\nid=flat\nstring=Gg\n[/terrain_type]\n\
            [units]\n[movetype]\nname=foot\n[defense]\nflat=50\n[/defense]\n[/movetype]\n\
            [unit_type]\nid=Guard\nhitpoints=30\nmovement_type=foot\n\
            [attack]\nname=blade\nrange=melee\ndamage=1\nnumber=4\n[/attack]\n[/unit_type]\n[/units]";
        let scenario = "[scenario]\nmap_data=\"Xv,Xv,Xv,Xv\nXv,1 Gg,2 Gg,Xv\nXv,Xv,Xv,Xv\"\n\
            [side]\ntype=Guard\n[/side]\n[side]\ntype=Guard\n[/side]\n[/scenario]";
        let play = |source: &str| {
            let bundle = Bundle::parse_with_core(source.as_bytes(), core).unwrap();
            let mut game = bundle.start(5489, |_| unreachable!()).unwrap();
            let action = AttackChoice { from: Hex { x: 0, y: 0 }, to: Hex { x: 1, y: 0 }, weapon: 0 };
            let (_, playback) = game.attack_for_display(action, None, 1).unwrap();
            playback.unwrap().blows.into_iter().map(|blow| blow.hit).collect::<Vec<_>>()
        };
        let expected = [false, true, false, true, true, false, true, false];
        let saved = format!("random_mode=biased\n{scenario}");
        assert_eq!(play(&saved), expected);
        assert_eq!(play(&scenario.replace("[scenario]", "[scenario]\nrandom_mode=biased")), expected);
        assert_ne!(play(scenario), expected);
        assert_eq!(play(&format!("random_mode=deterministic\n{scenario}")), play(scenario));
    }

    #[test]
    fn campaign_victory_can_wait_for_an_explicit_objective() {
        use crate::fixed_match::MatchState;
        for (controller, expected) in [
            ("human", MatchState::Playing(crate::turn::Turn { round: 1, side: 1 })),
            ("ai", MatchState::Won(1)),
        ] {
            let config = format!(r#"[units]
[unit_type]
id=Guard
hitpoints=20
[/unit_type]
[/units]
[scenario]
map_data="Xv,Xv,Xv,Xv
Xv,1 Gg,2 Gg,Xv
Xv,Xv,Xv,Xv"
victory_when_enemies_defeated=no
[side]
type=Guard
controller={controller}
[/side]
[side]
no_leader=yes
[/side]
[/scenario]"#);
            let bundle = Bundle::parse(config.as_bytes()).unwrap();
            assert_eq!(bundle.start(1, |_| unreachable!()).unwrap().state(), expected);
        }
    }

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
    fn derived_campaign_units_inherit_core_weapons_with_ordinal_overrides() {
        let core = b"[units]\n[unit_type]\nid=Base\nhitpoints=32\n[attack]\nname=sword\ndamage=4\n[/attack]\n[attack]\nname=bow\n[/attack]\n[/unit_type]\n[/units]";
        let bundle = b"[scenario]\n[/scenario]\n[units]\n[unit_type]\nid=Hero\n[base_unit]\nid=Veteran\n[/base_unit]\n[/unit_type]\n\
            [unit_type]\nid=Veteran\n[base_unit]\nid=Base\n[/base_unit]\n[attack]\ndamage=7\n[/attack]\n[/unit_type]\n[/units]";
        let parsed = Bundle::parse_with_core(bundle, core).unwrap();
        let hero = &parsed.unit_types["Hero"];
        assert_eq!((hero.hitpoints, hero.attacks.len()), (32, 2));
        assert_eq!((hero.attacks[0].id.as_str(), hero.attacks[0].damage), ("sword", 7));
        assert_eq!(hero.attacks[1].id, "bow");
        let text = std::str::from_utf8(bundle).unwrap();
        for base in ["Missing", "Hero"] {
            let broken = text.replace("id=Base", &format!("id={base}"));
            assert!(Bundle::parse_with_core(broken.as_bytes(), core).is_err());
        }
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
