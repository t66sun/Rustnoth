use std::collections::BTreeSet;

use crate::{hex::Hex, recruit_hexes};

use super::{Action, FixedMatch, MatchError, supports_unit};

impl FixedMatch<'_> {
    pub(super) fn extend_recruit_choices_in_view(
        &self,
        side: usize,
        choices: &mut Vec<Action>,
        terrain_known: impl Fn(Hex) -> bool,
        unit_visible: impl Fn(Hex) -> bool,
    ) {
        let board = &self.scenario.board;
        let team_types = &self.scenario.sides[side - 1].recruits;
        let leaders = board.units().filter(|(_, unit)| unit.side == side && unit.can_recruit);
        let reserves: Vec<_> = self.affordable_recalls(side).collect();
        let mut recall_destinations = BTreeSet::new();
        let mut placements = BTreeSet::new();
        for (leader, unit) in leaders {
            let types: Vec<_> = team_types.iter().chain(&unit.extra_recruits)
                .filter(|id| self.recruit_cost(side, id).is_some()).collect();
            if types.is_empty() && reserves.is_empty() {
                continue;
            }
            let destinations = self.leader_castle(leader, &terrain_known, |hex| {
                board.unit_at(hex).is_some_and(|occupant| occupant.side == side || unit_visible(hex))
            });
            if !reserves.is_empty() {
                recall_destinations.extend(destinations.iter().copied());
            }
            for id in types {
                placements.extend(destinations.iter().map(|&to| (id, to)));
            }
        }
        choices.extend(placements.into_iter().map(|(id, to)| Action::Recruit { unit_type: id.clone(), to }));
        for (id, _) in reserves {
            choices.extend(recall_destinations.iter().map(|&to| Action::Recall { unit_id: id.to_owned(), to }));
        }
    }

    fn affordable_recalls(&self, side: usize) -> impl Iterator<Item = (&str, i64)> {
        let default_cost = self.scenario.sides[side - 1].recall_cost;
        let gold = self.gold[side - 1];
        self.scenario.board.recall_roster(side).iter().filter_map(move |unit| {
            let id = unit.id.as_deref()?;
            let cost = i64::from(unit.recall_cost(default_cost));
            (cost <= gold && supports_unit(unit.unit_type)).then_some((id, cost))
        })
    }

    pub(super) fn recall(&mut self, side: usize, unit_id: &str, to: Hex) -> Result<(), MatchError> {
        let cost = self.affordable_recalls(side)
            .find_map(|(id, cost)| (id == unit_id).then_some(cost))
            .ok_or(MatchError::InvalidRecall)?;
        let occupied = |hex| self.scenario.board.unit_at(hex).is_some();
        let allowed = self.scenario.board.units().any(|(leader, unit)| {
            unit.side == side && unit.can_recruit && self.leader_castle(leader, |_| true, occupied).contains(&to)
        });
        if !allowed {
            return Err(MatchError::InvalidRecall);
        }
        self.scenario.board.place_recall(side, unit_id, to).ok_or(MatchError::InvalidRecall)?;
        self.gold[side - 1] -= cost;
        self.capture_village(side, to);
        Ok(())
    }

    pub(super) fn recruit(&mut self, side: usize, unit_type: &str, to: Hex) -> Result<(), MatchError> {
        let cost = self
            .recruit_cost(side, unit_type)
            .ok_or(MatchError::InvalidRecruit)?;
        let occupied = |hex| self.scenario.board.unit_at(hex).is_some();
        let team_recruit = self.scenario.sides[side - 1].recruits.contains(unit_type);
        let allowed = self.scenario.board.units().any(|(leader, unit)| {
            unit.side == side && unit.can_recruit
                && (team_recruit || unit.extra_recruits.iter().any(|id| id == unit_type))
                && self.leader_castle(leader, |_| true, occupied).contains(&to)
        });
        if !allowed {
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
        let unit = self.scenario.board.unit_type(unit_type)?;
        let cost = i64::from(unit.cost);
        (cost <= self.gold[index] && supports_unit(unit)).then_some(cost)
    }

    fn leader_castle(
        &self,
        leader: Hex,
        terrain_known: impl Fn(Hex) -> bool,
        occupied: impl Fn(Hex) -> bool,
    ) -> Vec<Hex> {
        let terrain = self.rules.terrain;
        recruit_hexes::destinations_in_view(
            leader, self.scenario.board.map(), |code| terrain.features(code).keep,
            |code| terrain.features(code).castle, terrain_known, occupied,
        )
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
    fn recalls_the_selected_reserve_once_at_its_own_cost_with_its_existing_stats() {
        let config = wml::parse(r#"
            map_data="Xv,Xv,Xv,Xv,Xv,Xv,Xv,Xv
Xv,1 Kh,Ch,Gg,Ch,2 Kh,Ch,Xv
Xv,Xv,Xv,Xv,Xv,Xv,Xv,Xv"
            [side]
                type=Guard
                gold=50
                recall_cost=18
                [unit]
                    id=veteran
                    type=Guard
                    placement=recall
                    experience=15
                    hitpoints=33
                    recall_cost=13
                    [modifications]
                        [trait]
                            id=loyal
                        [/trait]
                    [/modifications]
                [/unit]
                [unit]
                    id=reserve
                    type=Guard
                    placement=recall
                [/unit]
                [unit]
                    id=expensive
                    type=Guard
                    placement=recall
                    recall_cost=51
                [/unit]
            [/side]
            [side]
                type=Guard
                [unit]
                    id=enemy_veteran
                    type=Guard
                    placement=recall
                [/unit]
            [/side]
        "#).unwrap();
        let types = read_unit_types(wml::parse(
            "[unit_type]\nid=Guard\nhitpoints=55\nmovement=5\n[/unit_type]"
        ).unwrap());
        let terrain = TerrainAliases::from_wml(&wml::parse(
            "[terrain_type]\nstring=Kh\nrecruit_from=yes\nrecruit_onto=yes\n[/terrain_type]\n\
             [terrain_type]\nstring=Ch\nrecruit_onto=yes\n[/terrain_type]"
        ).unwrap());
        let movement = BTreeMap::new();
        let resistance = BTreeMap::new();
        let rules = Rules { terrain: &terrain, movement: &movement, resistance: &resistance };
        let start = scenario_start::load(&config, &types, |_| unreachable!()).unwrap();
        let mut game = FixedMatch::new(start, rules, 42).unwrap();

        let castle = Hex { x: 1, y: 0 };
        let recall = |id: &str, to| Action::Recall { unit_id: id.into(), to };
        let before = game.decision().unwrap();
        let recalls: Vec<_> = before.1.iter().filter(|a| matches!(a, Action::Recall { .. })).cloned().collect();
        assert_eq!(recalls, [recall("veteran", castle), recall("reserve", castle)]);
        for action in [
            recall("veteran", Hex { x: 3, y: 0 }),
            recall("expensive", castle),
            recall("enemy_veteran", castle),
        ] {
            assert_eq!(game.step(action), Err(MatchError::InvalidRecall));
            assert_eq!(game.decision().unwrap(), before);
        }
        game.step(recall("veteran", castle)).unwrap();
        let unit = game.scenario.board.unit_at(castle).unwrap();
        assert_eq!(unit.id.as_deref(), Some("veteran"));
        assert_eq!(unit.traits().next().unwrap().attribute("id"), Some("loyal"));
        assert_eq!((unit.hitpoints, unit.experience, unit.moves_left, unit.attacks_left), (33, 15, 0, 0));
        assert_eq!(game.gold[0], 37);
        assert_eq!(game.rng.seed_and_calls(), (42, 0));
        assert_eq!(game.scenario.board.recall_roster(1).len(), 2);
        assert_eq!(game.step(recall("veteran", castle)), Err(MatchError::InvalidRecall));
        assert_eq!(game.gold[0], 37);
        game.step(Action::EndTurn).unwrap();
        game.step(Action::EndTurn).unwrap();
        let unit = game.scenario.board.unit_at(castle).unwrap();
        assert_eq!((unit.moves_left, unit.attacks_left), (5, 1));
    }

    #[test]
    fn personal_recruits_are_limited_to_the_eligible_leaders_castle() {
        let scenario = wml::parse(r#"
            map_data="Xv,Xv,Xv,Xv,Xv,Xv,Xv,Xv,Xv
Xv,1 Kh,Ch,Gg,Kh,Ch,Gg,2 Kh,Xv
Xv,Xv,Xv,Xv,Xv,Xv,Xv,Xv,Xv"
            [side]
                type=Guard
                recruit=Guard
                gold=20
                [unit]
                    type=Guard
                    x,y=4,1
                    canrecruit=yes
                    extra_recruit=Bowman,Guard
                [/unit]
            [/side]
            [side]
                type=Guard
            [/side]
        "#).unwrap();
        let types = read_unit_types(wml::parse(
            "[unit_type]\nid=Guard\nhitpoints=10\ncost=3\n[/unit_type]\n\
             [unit_type]\nid=Bowman\nhitpoints=9\ncost=5\n[/unit_type]"
        ).unwrap());
        let terrain = TerrainAliases::from_wml(&wml::parse(
            "[terrain_type]\nstring=Kh\nrecruit_from=yes\nrecruit_onto=yes\n[/terrain_type]\n\
             [terrain_type]\nstring=Ch\nrecruit_onto=yes\n[/terrain_type]"
        ).unwrap());
        let movement = BTreeMap::new();
        let resistance = BTreeMap::new();
        let rules = Rules { terrain: &terrain, movement: &movement, resistance: &resistance };
        let start = scenario_start::load(&scenario, &types, |_| unreachable!()).unwrap();
        let mut game = FixedMatch::new(start, rules, 10).unwrap();
        let left = Hex { x: 1, y: 0 };
        let right = Hex { x: 4, y: 0 };
        let action = |id: &str, to| Action::Recruit { unit_type: id.to_owned(), to };
        let mut choices = Vec::new();
        game.extend_recruit_choices_in_view(1, &mut choices, |_| true, |_| true);
        assert_eq!(choices, [action("Bowman", right), action("Guard", left), action("Guard", right)]);
        assert_eq!(game.step(action("Bowman", left)), Err(MatchError::InvalidRecruit));
        assert_eq!(game.gold[0], 20);
        game.step(action("Bowman", right)).unwrap();
        assert_eq!(game.gold[0], 15);
    }

    #[test]
    fn recruitment_candidates_use_only_known_castle_connections_and_visible_blockers() {
        let config = wml::parse(r#"
            map_data="Xv,Xv,Xv,Xv,Xv,Xv,Xv
Xv,1 Kh,Ch,Ch,Ch,2 Gg,Xv
Xv,Xv,Xv,Xv,Xv,Xv,Xv"
            [side]
                side=1
                type=Guard
                recruit=Guard
            [/side]
            [side]
                side=2
                type=Guard
            [/side]
        "#).unwrap();
        let types = read_unit_types(wml::parse(
            "[unit_type]\nid=Guard\nhitpoints=10\ncost=1\n[/unit_type]"
        ).unwrap());
        let terrain = TerrainAliases::from_wml(&wml::parse(
            "[terrain_type]\nstring=Kh\nrecruit_from=yes\nrecruit_onto=yes\n[/terrain_type]\n\
             [terrain_type]\nstring=Ch\nrecruit_onto=yes\n[/terrain_type]"
        ).unwrap());
        let movement = BTreeMap::new();
        let resistance = BTreeMap::new();
        let rules = Rules { terrain: &terrain, movement: &movement, resistance: &resistance };
        let start = scenario_start::load(&config, &types, |_| unreachable!()).unwrap();
        let mut game = FixedMatch::new(start, rules, 42).unwrap();
        let bridge = Hex { x: 1, y: 0 };
        let fogged = Hex { x: 2, y: 0 };
        let own = Hex { x: 3, y: 0 };
        game.scenario.board.place_recruit(1, "Guard", own).unwrap();

        let candidates = |game: &FixedMatch<'_>, bridge_known, foreign_visible| {
            let mut choices = Vec::new();
            game.extend_recruit_choices_in_view(1, &mut choices,
                |hex| hex != bridge || bridge_known,
                |hex| hex == fogged && foreign_visible,
            );
            choices
        };
        let recruit = |to| Action::Recruit { unit_type: "Guard".into(), to };
        let empty_castle = candidates(&game, true, false);
        assert_eq!(empty_castle, [recruit(bridge), recruit(fogged)]);
        assert!(candidates(&game, false, false).is_empty());

        game.scenario.board.place_recruit(2, "Guard", fogged).unwrap();
        assert_eq!(candidates(&game, true, false), empty_castle);
        assert_eq!(candidates(&game, true, true), [recruit(bridge)]);
        assert_eq!(game.step(recruit(fogged)), Err(MatchError::InvalidRecruit));
    }

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
