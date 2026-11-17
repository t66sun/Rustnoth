use std::collections::BTreeSet;

use crate::{hex::Hex, recruit_hexes};

use super::{Action, FixedMatch, MatchError, supports_unit};

impl FixedMatch<'_> {
    pub(super) fn extend_recruit_choices(&self, side: usize, choices: &mut Vec<Action>) {
        let affordable: Vec<_> = self.scenario.sides[side - 1]
            .recruits
            .iter()
            .filter(|unit_type| self.recruit_cost(side, unit_type).is_some())
            .collect();
        if affordable.is_empty() {
            return;
        }

        let destinations = self.recruit_destinations(side);
        for unit_type in affordable {
            choices.extend(destinations.iter().map(|&to| Action::Recruit {
                unit_type: unit_type.clone(),
                to,
            }));
        }
    }

    pub(super) fn recruit(&mut self, side: usize, unit_type: &str, to: Hex) -> Result<(), MatchError> {
        let cost = self
            .recruit_cost(side, unit_type)
            .ok_or(MatchError::InvalidRecruit)?;
        if !self.recruit_destinations(side).contains(&to) {
            return Err(MatchError::InvalidRecruit);
        }

        self.scenario
            .board
            .place_random_recruit(side, unit_type, to, &mut self.rng)
            .map_err(|_| MatchError::InvalidRecruit)?;
        self.gold[side - 1] -= cost;
        self.capture_village(side, to);
        Ok(())
    }

    fn recruit_cost(&self, side: usize, unit_type: &str) -> Option<i64> {
        let index = side - 1;
        if !self.scenario.sides[index].recruits.contains(unit_type) {
            return None;
        }
        let unit = self.scenario.board.unit_type(unit_type)?;
        let cost = i64::from(unit.cost);
        (cost <= self.gold[index] && supports_unit(unit)).then_some(cost)
    }

    fn recruit_destinations(&self, side: usize) -> BTreeSet<Hex> {
        let board = &self.scenario.board;
        let terrain = self.rules.terrain;
        board
            .units()
            .filter(|(_, unit)| unit.side == side && unit.can_recruit)
            .flat_map(|(leader, _)| {
                recruit_hexes::destinations(
                    leader,
                    board.map(),
                    |code| terrain.features(code).keep,
                    |code| terrain.features(code).castle,
                    |hex| board.unit_at(hex).is_some(),
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::{
        fixed_match::Rules, scenario_start, terrain_aliases::TerrainAliases,
        unit_type::read_unit_types, wml,
    };

    use super::*;

    #[test]
    fn new_recruits_keep_selected_traits_and_use_their_effective_stats() {
        let types = read_unit_types(wml::parse(
            r#"
            [unit_type]
                id=Guard
                hitpoints=33
                movement=5
                experience=35
                num_traits=1
                [attack]
                    range=melee
                    damage=5
                    number=1
                [/attack]
                [trait]
                    id=veteran
                    [effect]
                        apply_to=hitpoints
                        increase_total=4
                    [/effect]
                    [effect]
                        apply_to=movement
                        increase=1
                    [/effect]
                    [effect]
                        apply_to=max_experience
                        increase=-20%
                    [/effect]
                    [effect]
                        apply_to=attack
                        range=melee
                        increase_damage=2
                    [/effect]
                [/trait]
            [/unit_type]
            "#,
        ).unwrap());
        let scenario = wml::parse(
            r#"
            experience_modifier=33
            map_data="Xv,Xv,Xv,Xv,Xv
Xv,1 Kh,Ch,2 Kh,Xv
Xv,Xv,Xv,Xv,Xv"
            [side]
                side=1
                type=Guard
                recruit=Guard
            [/side]
            [side]
                side=2
                type=Guard
            [/side]
            "#,
        ).unwrap();
        let terrain = TerrainAliases::from_wml(&wml::parse(
            "[terrain_type]\nstring=Kh\nrecruit_from=yes\nrecruit_onto=yes\n[/terrain_type]\n\
             [terrain_type]\nstring=Ch\nrecruit_onto=yes\n[/terrain_type]",
        ).unwrap());
        let movement = BTreeMap::new();
        let resistance = BTreeMap::new();
        let rules = Rules { terrain: &terrain, movement: &movement, resistance: &resistance };
        let mut game = FixedMatch::new(
            scenario_start::load(&scenario, &types, |_| unreachable!()).unwrap(), rules, 5489,
        ).unwrap();
        let castle = Hex { x: 1, y: 0 };
        let leader = game.scenario.board.unit_at(Hex { x: 0, y: 0 }).unwrap();
        assert_eq!(leader.max_experience(), 12);
        game.step(Action::Recruit { unit_type: "Guard".into(), to: castle }).unwrap();
        let recruit = game.scenario.board.unit_at(castle).unwrap();
        assert_eq!(recruit.traits().next().unwrap().attribute("id"), Some("veteran"));
        assert_eq!((recruit.hitpoints, recruit.moves_left, recruit.max_experience()), (37, 0, 10));
        assert_eq!(game.rng.seed_and_calls(), (5489, 1));

        game.step(Action::EndTurn).unwrap();
        game.step(Action::EndTurn).unwrap();
        assert_eq!(game.scenario.board.unit_at(castle).unwrap().moves_left, 6);
        let east = Hex { x: 2, y: 0 };
        game.step(Action::Attack(crate::board::AttackChoice { from: castle, to: east, weapon: 0 }))
            .unwrap();
        assert_eq!(game.scenario.board.unit_at(east).unwrap().hitpoints, 26);
    }

    #[test]
    fn recruits_at_the_advertised_castle_tiles_and_charges_only_accepted_actions() {
        let scenario = wml::parse(
            r#"
            map_data="Xv,Xv,Xv,Xv,Xv,Xv
Xv,1 Kh,Ch^Vh,Ch,2 Gg,Xv
Xv,Xv,Xv,Xv,Xv,Xv"
            [side]
                side=1
                type=Leader
                gold=12
                recruit=Fighter,Elite,Unknown,Spellcaster
            [/side]
            [side]
                side=2
                type=Leader
                [village]
                    x,y=2,1
                [/village]
            [/side]
            "#,
        )
        .unwrap();
        let units = read_unit_types(
            wml::parse(
                r#"
                [unit_type]
                    id=Leader
                    hitpoints=15
                    cost=1
                [/unit_type]
                [unit_type]
                    id=Fighter
                    hitpoints=28
                    movement=5
                    cost=10
                [/unit_type]
                [unit_type]
                    id=Elite
                    cost=13
                [/unit_type]
                [unit_type]
                    id=Spellcaster
                    cost=1
                    [attack]
                        [specials]
                            [drains]
                            [/drains]
                        [/specials]
                    [/attack]
                [/unit_type]
                "#,
            )
            .unwrap(),
        );
        let terrain = TerrainAliases::from_wml(
            &wml::parse(
                "[terrain_type]\nstring=Kh\nrecruit_from=yes\nrecruit_onto=yes\n[/terrain_type]\n\
                 [terrain_type]\nstring=Ch\nrecruit_onto=yes\n[/terrain_type]\n\
                 [terrain_type]\nstring=^Vh\ngives_income=yes\n[/terrain_type]",
            )
            .unwrap(),
        );
        let movement = BTreeMap::new();
        let resistance = BTreeMap::new();
        let rules = Rules {
            terrain: &terrain,
            movement: &movement,
            resistance: &resistance,
        };
        let start = scenario_start::load(&scenario, &units, |_| unreachable!()).unwrap();
        let mut game = FixedMatch::new(start, rules, 5489).unwrap();

        let village = Hex { x: 1, y: 0 };
        let castle = Hex { x: 2, y: 0 };
        let recruit = |unit_type: &str, to| Action::Recruit {
            unit_type: unit_type.to_owned(),
            to,
        };
        let before = game.decision().unwrap();
        let candidates: Vec<_> = before
            .1
            .iter()
            .filter(|action| matches!(action, Action::Recruit { .. }))
            .cloned()
            .collect();
        assert_eq!(
            candidates,
            [recruit("Fighter", village), recruit("Fighter", castle)]
        );
        assert_eq!(before.0.tiles[1].village_owner, Some(2));

        for invalid in [
            recruit("Elite", village),
            recruit("Unknown", village),
            recruit("Spellcaster", village),
            recruit("Leader", village),
            recruit("Fighter", Hex { x: 3, y: 0 }),
        ] {
            assert_eq!(game.step(invalid), Err(MatchError::InvalidRecruit));
            assert_eq!(game.decision().unwrap(), before);
        }

        game.step(recruit("Fighter", village)).unwrap();
        let (observation, choices) = game.decision().unwrap();
        let fighter = observation
            .units
            .iter()
            .find(|unit| unit.position == village)
            .unwrap();
        assert_eq!(fighter.type_id, "Fighter");
        assert_eq!(
            (fighter.hitpoints, fighter.moves_left, fighter.attacks_left),
            (28, Some(0), Some(0))
        );
        assert_eq!(observation.gold, Some(2));
        assert_eq!(observation.tiles[1].village_owner, Some(1));
        assert!(!choices.iter().any(|action| matches!(action, Action::Recruit { .. })));
    }
}
