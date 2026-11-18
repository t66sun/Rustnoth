use super::{FixedMatch, MatchError};
use crate::{hex::Hex, visibility::HexVisibility};

impl FixedMatch<'_> {
    pub(super) fn walk(&mut self, side: usize, from: Hex, to: Hex) -> Result<(), MatchError> {
        let route = self.planned_route(side, from, to)?;
        let (mut position, mut moves_left) = route[0];
        let mut discovered = Vec::new();
        let board = &self.scenario.board;
        let sides = &self.scenario.sides;
        let moving_side = &sides[side - 1];
        let mover = board.unit_at(from).expect("validated route origin");
        let mover_serial = mover.serial;
        let mut view = (moving_side.fog || moving_side.shroud).then(|| self.effective_vision(side));
        let mut sighted = false;

        // Native movement checks the plotted path for hidden units before moving the scout.
        // An allied occupant can be traversed but cannot become the scout's final location.
        for steps in route.windows(2) {
            let (previous, _) = steps[0];
            let (next, remaining) = steps[1];
            // A hidden occupant blocks a tunnel without revealing the remote exit.
            if previous.distance(next) > 1 && (board.unit_at(next).is_some() || previous != position) {
                break;
            }
            match board.unit_at(next) {
                Some(occupant) if sides[side - 1].is_enemy(&sides[occupant.side - 1]) => {
                    discovered.push(next);
                    break;
                }
                None => position = next,
                Some(_) => {}
            }
            moves_left = remaining;
            discovered.extend(next.neighbors().filter(|&neighbor| {
                board.unit_at(neighbor).is_some()
                    && !self.unit_visible_on_known_hex(side, neighbor)
            }));
            if let Some(view) = &mut view {
                let sight = self.unit_sight(from, next, mover);
                sighted |= sight.iter().any(|&hex| {
                    view.at(hex) != Some(HexVisibility::Visible)
                        && board.unit_at(hex).is_some_and(|unit| unit.side != side)
                        && self.unit_visible_on_known_hex(side, hex)
                });
                self.vision[side - 1].add_sight(sight.iter().copied());
                view.add_sight(sight);
            }
            if !discovered.is_empty() {
                moves_left = 0;
                break;
            }
            if sighted && position == next {
                let code = board.map().terrain_at(next).expect("route stays on the map");
                if !self.rules.terrain.features(code).village || self.village_owners.get(&next) == Some(&side) {
                    break;
                }
            }
        }

        let board = &mut self.scenario.board;
        board.finish_walk(from, position, moves_left);
        for hex in discovered {
            board.uncover_unit(hex);
        }
        let captured = self.capture_village(side, position)?;
        if self.scenario.board.unit_at(position).is_none_or(|unit| unit.serial != mover_serial) { return Ok(()); }
        if captured {
            self.scenario.board.stop_movement(position);
        }
        if position != from {
            self.fire_unit_event("moveto", position)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{fixed_match::Action, scenario_bundle::Bundle};

    const START: Hex = Hex { x: 0, y: 0 };
    const STOP: Hex = Hex { x: 3, y: 0 };
    const GOAL: Hex = Hex { x: 6, y: 0 };

    fn scenario(fog: bool, forest: bool, scout_row: &str) -> Bundle {
        scenario_with_events(fog, forest, scout_row, "")
    }

    fn scenario_with_events(fog: bool, forest: bool, scout_row: &str, events: &str) -> Bundle {
        let fog = if fog { "yes" } else { "no" };
        let enemy_row = if forest {
            "Gg,Gg,Gg,Gg,2 Gg^Fp,Gg,Gg,Gg"
        } else {
            "Xv,Xv,Xv,Xv,Xv,Xv,Xv,2 Gg"
        };
        let wml = format!(r#"
[terrain_type]
    id=flat
    string=Gg
[/terrain_type]
[terrain_type]
    id=village
    string=^Vh
    aliasof=_bas
    gives_income=yes
[/terrain_type]
[units]
    [unit_type]
        id=Elvish Scout
        hitpoints=32
        movement=9
        vision=0
        [movement_costs]
            flat=1
        [/movement_costs]
    [/unit_type]
    [unit_type]
        id=Wose
        hitpoints=52
        level=2
        [abilities]
            [hides]
                id=ambush
                [filter]
                    [filter_location]
                        terrain=*^F*,*^Qhhf,*^Qhuf
                    [/filter_location]
                [/filter]
            [/hides]
        [/abilities]
    [/unit_type]
[/units]
[scenario]
    map_data="Xv,Xv,Xv,Xv,Xv,Xv,Xv,Xv,Xv,Xv
Xv,{scout_row},Xv
Xv,{enemy_row},Xv
Xv,Xv,Xv,Xv,Xv,Xv,Xv,Xv,Xv,Xv"
    [side]
        side=1
        team_name=west
        id=scout
        type=Elvish Scout
        fog={fog}
        [unit]
            type=Elvish Scout
            x,y=2,1
        [/unit]
    [/side]
    [side]
        side=2
        team_name=east
        type=Wose
    [/side]
    {events}
[/scenario]
"#);
        Bundle::parse(wml.as_bytes()).unwrap()
    }

    #[test]
    fn moveto_fires_on_the_actual_stop_after_an_ambush() {
        let events = r#"
            [event]
                name=moveto
                [filter]
                    x=2,4-5
                    y=2,1
                    side=1
                    id=scout,second_scout
                    type=Cavalryman,Elvish Scout
                [/filter]
                [gold]
                    side=1
                    amount=7
                [/gold]
                [allow_recruit]
                    side=1
                    type=Wose
                [/allow_recruit]
            [/event]
            [event]
                name=moveto
                [filter]
                    x=2-3,7
                    y=1,1
                [/filter]
                [gold]
                    side=1
                    amount=100
                [/gold]
            [/event]
            [event]
                name=moveto
                [filter]
                    x=4
                    race=elf
                [/filter]
                [gold]
                    side=1
                    amount=1000
                [/gold]
            [/event]"#;
        let bundle = scenario_with_events(true, true, "1 Gg,Gg,Gg,Gg^Vh,Gg,Gg,Gg^Vh,Gg", events);
        let mut game = bundle.start(1, |_| unreachable!()).unwrap();
        let initial = game.gold[0];
        game.step(Action::Move { from: START, to: GOAL }).unwrap();
        assert_eq!(game.scenario.board.unit_at(STOP).unwrap().id.as_deref(), Some("scout"));
        assert_eq!(game.gold[0], initial + 7);
        assert!(game.scenario.sides[0].recruits.contains("Wose"));
    }

    #[test]
    fn a_village_capture_rewards_its_unit_before_moveto_and_only_when_ownership_changes() {
        let events = r#"
            [event]
                name=capture
                first_time_only=no
                [filter]
                    side=1
                    x,y=4,1
                    id=scout
                [/filter]
                [gold]
                    side=1
                    amount=9
                [/gold]
                [allow_recruit]
                    side=1
                    type=Wose
                [/allow_recruit]
            [/event]
            [event]
                name=moveto
                [filter]
                    x,y=4,1
                [/filter]
                [disallow_recruit]
                    side=1
                    type=Wose
                [/disallow_recruit]
            [/event]"#;
        let bundle = scenario_with_events(false, false, "1 Gg,Gg,Gg,Gg^Vh,Gg,Gg,Gg,Gg", events);
        let mut game = bundle.start(1, |_| unreachable!()).unwrap();
        game.village_owners.insert(STOP, 2);
        let initial = game.gold[0];
        game.step(Action::Move { from: START, to: STOP }).unwrap();
        assert_eq!(game.gold[0], initial + 9);
        assert_eq!(game.village_owners.get(&STOP), Some(&1));
        assert!(!game.scenario.sides[0].recruits.contains("Wose"));
        game.step(Action::EndTurn).unwrap();
        game.step(Action::EndTurn).unwrap();
        let gold = game.gold[0];
        let next = Hex { x: 4, y: 0 };
        game.step(Action::Move { from: STOP, to: next }).unwrap();
        game.step(Action::Move { from: next, to: STOP }).unwrap();
        assert_eq!(game.gold[0], gold);
        assert!(!game.scenario.sides[0].recruits.contains("Wose"));
    }

    #[test]
    fn a_campaign_destination_can_finish_a_match_with_both_leaders_alive() {
        let events = "[event]\nname=moveto\n[filter]\nx,y=4,1\nid=scout\n[/filter]\n[endlevel]\nresult=victory\n[/endlevel]\n[/event]";
        let bundle = scenario_with_events(false, false, "1 Gg,Gg,Gg,Gg,Gg,Gg,Gg,Gg", events);
        let mut game = bundle.start(1, |_| unreachable!()).unwrap();
        assert_eq!(game.step(Action::Move { from: START, to: STOP }).unwrap(),
                   crate::fixed_match::MatchState::CampaignVictory);
    }

    #[test]
    fn unmatched_arrivals_leave_once_handlers_pending_and_matching_arrivals_can_repeat() {
        let reward = |once: &str, amount: i32| format!(
            "[event]\nname=moveto\nfirst_time_only={once}\n[filter]\nx=4\ny=1\nid=scout\n[/filter]\n\
             [gold]\nside=1\namount={amount}\n[/gold]\n[/event]\n"
        );
        let events = reward("yes", 7) + &reward("no", 2) + &reward("no", 100).replace("x=4", "x=1");
        let bundle = scenario_with_events(false, false, "1 Gg,Gg,Gg,Gg,Gg,Gg,Gg,Gg", &events);
        let mut game = bundle.start(1, |_| unreachable!()).unwrap();
        let initial = game.gold[0];
        assert!(game.step(Action::Move { from: START, to: START }).is_err());
        assert_eq!(game.gold[0], initial);
        let through = Hex { x: 2, y: 0 };
        let away = Hex { x: 4, y: 0 };
        for (from, to, bonus) in [(START, through, 0), (through, STOP, 9), (STOP, away, 9), (away, STOP, 11)] {
            game.step(Action::Move { from, to }).unwrap();
            assert_eq!(game.gold[0], initial + bonus);
        }
    }

    #[test]
    fn discovering_a_wose_stops_the_scout_and_reveals_only_the_travelled_route() {
        let enemy = Hex { x: 4, y: 1 };
        for fog in [false, true] {
            let bundle = scenario(fog, true, "1 Gg,Gg,Gg,Gg^Vh,Gg,Gg,Gg^Vh,Gg");
            let mut game = bundle.start(1, |_| unreachable!()).unwrap();
            let action = Action::Move { from: START, to: GOAL };
            let (view, candidates) = game.decision().unwrap();
            assert!(!view.units.iter().any(|unit| unit.position == enemy));
            assert!(candidates.contains(&action));

            game.step(action).unwrap();
            let scout = game.scenario.board.unit_at(STOP).unwrap();
            assert_eq!((scout.unit_type.id.as_str(), scout.moves_left, scout.attacks_left), ("Elvish Scout", 0, 1));
            assert!(!game.scenario.board.uncover_unit(enemy));
            assert_eq!(game.village_owners.get(&STOP), Some(&1));
            assert!(!game.village_owners.contains_key(&GOAL));
            let view = game.observation_for(1).unwrap();
            assert!(view.units.iter().any(|unit| unit.position == enemy));
            assert_eq!(view.tiles.iter().find(|tile| tile.position == GOAL).unwrap().visible, !fog);
        }
    }

    #[test]
    fn open_route_crosses_an_ally_and_charges_only_the_six_entered_hexes() {
        let bundle = scenario(false, false, "1 Gg,Gg,Gg,Gg,Gg,Gg,Gg,Gg");
        let mut game = bundle.start(1, |_| unreachable!()).unwrap();
        game.step(Action::Move { from: START, to: GOAL }).unwrap();
        assert_eq!(game.scenario.board.unit_at(GOAL).unwrap().moves_left, 3);
        assert_eq!(game.scenario.board.unit_at(Hex { x: 1, y: 0 }).unwrap().side, 1);
    }

    #[test]
    fn an_unseen_blocker_is_not_overwritten_or_charged_as_an_entered_hex() {
        let bundle = scenario(true, false, "1 Gg,Gg,Gg,Gg,Gg,Gg,Gg,Gg");
        let mut game = bundle.start(1, |_| unreachable!()).unwrap();
        let blocker = Hex { x: 4, y: 0 };
        // The scout sights the blocker while passing through an ally and has no
        // vacant tile to stop on before attempting to enter the blocker's tile.
        game.scenario.board.place_recruit(1, "Elvish Scout", STOP).unwrap();
        game.scenario.board.place_recruit(2, "Elvish Scout", blocker).unwrap();
        let action = Action::Move { from: START, to: GOAL };
        assert!(game.decision().unwrap().1.contains(&action));
        let route: Vec<_> = game.planned_route(1, START, GOAL).unwrap().into_iter().map(|(hex, _)| hex).collect();
        assert_eq!(route, (0..=6).map(|x| Hex { x, y: 0 }).collect::<Vec<_>>());
        game.step(action).unwrap();
        assert_eq!(game.scenario.board.unit_at(Hex { x: 2, y: 0 }).unwrap().moves_left, 6);
        assert_eq!(game.scenario.board.unit_at(blocker).unwrap().side, 2);
        assert!(!game.scenario.board.uncover_unit(blocker));
    }

    fn sighting_lane(third_tile: &str, concealment: &str) -> Bundle {
        let border = ["Xv"; 19].join(",");
        let mut row = ["Gg"; 19];
        row[0] = "Xv";
        row[1] = "1 Gg";
        row[3] = third_tile;
        row[13] = "2 Gg";
        row[18] = "Xv";
        let row = row.join(",");
        let config = format!(r#"
[terrain_type]
    id=flat
    string=Gg
[/terrain_type]
[terrain_type]
    id=village
    string=^Vh
    aliasof=_bas
    gives_income=yes
[/terrain_type]
[units]
    [unit_type]
        id=Scout
        hitpoints=32
        movement=9
        [movement_costs]
            flat=1
        [/movement_costs]
    [/unit_type]
    [unit_type]
        id=Lookout
        hitpoints=33
        {concealment}
    [/unit_type]
[/units]
[scenario]
    map_data="{border}
{row}
{border}"
    [side]
        side=1
        type=Scout
        fog=yes
    [/side]
    [side]
        side=2
        type=Lookout
        fog=yes
    [/side]
[/scenario]"#);
        Bundle::parse(config.as_bytes()).unwrap()
    }

    #[test]
    fn sighting_a_foreign_unit_stops_once_at_the_first_reasonable_hex_with_unspent_moves() {
        let enemy = Hex { x: 12, y: 0 };
        for (tile, stop_x, remaining) in [("Gg", 2, 7), ("Gg^Vh", 3, 6)] {
            let bundle = sighting_lane(tile, "");
            let mut game = bundle.start(1, |_| unreachable!()).unwrap();
            let stop = Hex { x: stop_x, y: 0 };
            assert!(!game.observation_for(1).unwrap().units.iter().any(|u| u.position == enemy));
            game.step(Action::Move { from: START, to: GOAL }).unwrap();
            assert_eq!(game.scenario.board.unit_at(stop).unwrap().moves_left, remaining);
            assert!(game.observation_for(1).unwrap().units.iter().any(|u| u.position == enemy));
            assert!(game.village_owners.is_empty());
            game.step(Action::Move { from: stop, to: GOAL }).unwrap();
            assert_eq!(game.scenario.board.unit_at(GOAL).unwrap().moves_left, 3);
        }
    }

    #[test]
    fn revealing_the_terrain_of_a_concealed_unit_does_not_interrupt_a_move() {
        let bundle = sighting_lane("Gg", "[abilities]\n[hides]\nid=hidden\n[/hides]\n[/abilities]");
        let mut game = bundle.start(1, |_| unreachable!()).unwrap();
        game.step(Action::Move { from: START, to: GOAL }).unwrap();
        assert_eq!(game.scenario.board.unit_at(GOAL).unwrap().moves_left, 3);
        let view = game.observation_for(1).unwrap();
        let enemy = Hex { x: 12, y: 0 };
        assert!(view.tiles.iter().any(|tile| tile.position == enemy && tile.visible));
        assert!(!view.units.iter().any(|unit| unit.position == enemy));
    }
}
