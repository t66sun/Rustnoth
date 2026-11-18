use super::{FixedMatch, Hex, UnitState};

impl FixedMatch<'_> {
    pub(super) fn teleport_villages(
        &self,
        from: Hex,
        mover: &UnitState<'_>,
        terrain_known: impl Fn(Hex) -> bool,
        unit_visible: impl Fn(Hex) -> bool,
    ) -> Vec<Hex> {
        if !mover.unit_type.village_teleport {
            return Vec::new();
        }
        let board = &self.scenario.board;
        self.village_owners.iter().filter_map(|(&hex, &owner)| {
            if owner != mover.side || !terrain_known(hex) {
                return None;
            }
            let (_, overlay) = board.map().terrain_at(hex)?.split_once('^')?;
            let occupied = hex != from && board.unit_at(hex)
                .is_some_and(|unit| unit.side == mover.side || unit_visible(hex));
            (overlay.starts_with('V') && !occupied).then_some(hex)
        }).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{fixed_match::Action, scenario_bundle::Bundle};

    const ABILITY: &str = r#"
[abilities]
    [teleport]
        id=teleport
        [tunnel]
            id=village_teleport
            [source]
                terrain=*^V*
                formula="owner_side = teleport_unit.side_number and (unit = teleport_unit or not unit)
                    where unit = unit_at(loc)"
            [/source]
            [target]
                terrain=*^V*
                formula="owner_side = teleport_unit.side_number and not unit_at(loc)"
            [/target]
            [filter]
                ability=teleport
            [/filter]
        [/tunnel]
    [/teleport]
[/abilities]"#;

    fn at(x: i32) -> Hex { Hex { x, y: 0 } }

    fn scenario(ability: &str, start_terrain: &str, fog: bool, shroud: bool) -> Bundle {
        let border = ["Xv"; 18].join(",");
        let wml = format!(r#"
[terrain_type]
    id=flat
    string=Gg
[/terrain_type]
[terrain_type]
    id=village
    string=Gg^Vh
    gives_income=yes
[/terrain_type]
[units]
    [unit_type]
        id=Mage
        hitpoints=25
        level=1
        movement=5
        vision=0
        [movement_costs]
            flat=1
            village=2
        [/movement_costs]
        {ability}
    [/unit_type]
[/units]
[scenario]
    map_data="{border}
Xv,1 {start_terrain},Gg^Vh,Gg,Gg,Gg,Gg,Gg,Gg,Gg^Vh,Gg,Gg,Gg,Gg,Gg^Vh,Gg,2 Gg,Xv
{border}"
    [side]
        side=1
        type=Mage
        fog={fog}
        shroud={shroud}
        [village]
            x,y=1,1
        [/village]
        [village]
            x,y=2,1
        [/village]
        [village]
            x,y=9,1
        [/village]
    [/side]
    [side]
        side=2
        type=Mage
        [village]
            x,y=14,1
        [/village]
    [/side]
[/scenario]"#);
        Bundle::parse(wml.as_bytes()).unwrap()
    }

    fn move_to(x: i32) -> Action { Action::Move { from: at(0), to: at(x) } }

    #[test]
    fn enters_and_leaves_an_owned_village_network_and_sees_only_travelled_hexes() {
        let bundle = scenario(ABILITY, "Gg", true, false);
        let mut game = bundle.start(1, |_| unreachable!()).unwrap();
        let (view, choices) = game.decision().unwrap();
        assert_eq!(view.tiles.iter().find(|t| t.position == at(8)).unwrap().village_owner, Some(1));
        assert!(choices.contains(&move_to(9)));
        assert!(!choices.contains(&move_to(13)));
        assert_eq!(game.planned_route(1, at(0), at(9)).unwrap(),
            [(at(0), 5), (at(1), 3), (at(8), 1), (at(9), 0)]);
        game.step(move_to(9)).unwrap();
        assert_eq!(game.scenario.board.unit_at(at(9)).unwrap().moves_left, 0);
        let view = game.observation_for(1).unwrap();
        assert!(!view.tiles.iter().find(|tile| tile.position == at(4)).unwrap().visible);
        assert!(view.tiles.iter().find(|tile| tile.position == at(8)).unwrap().visible);
    }

    #[test]
    fn ordinary_units_custom_tunnels_and_shrouded_exits_get_no_village_jump() {
        let custom = ABILITY.replace("not unit_at(loc)", "true");
        for (ability, shroud) in [("", false), (custom.as_str(), false), (ABILITY, true)] {
            let bundle = scenario(ability, "Gg", true, shroud);
            let game = bundle.start(1, |_| unreachable!()).unwrap();
            let choices = game.decision().unwrap().1;
            assert!(choices.contains(&move_to(1)));
            assert!(!choices.contains(&move_to(8)));
            assert!(game.planned_route(1, at(0), at(8)).is_err());
        }
    }

    #[test]
    fn visible_exit_blockers_prevent_jumps_and_visible_zoc_ends_movement() {
        let bundle = scenario(ABILITY, "Gg^Vh", false, false);
        for side in [1, 2] {
            let mut game = bundle.start(1, |_| unreachable!()).unwrap();
            game.scenario.board.place_recruit(side, "Mage", at(8)).unwrap();
            assert!(!game.decision().unwrap().1.contains(&move_to(8)));
            assert!(!game.decision().unwrap().1.contains(&move_to(9)));
        }
        let mut game = bundle.start(1, |_| unreachable!()).unwrap();
        game.scenario.board.place_recruit(2, "Mage", at(9)).unwrap();
        assert!(game.decision().unwrap().1.contains(&move_to(8)));
        game.step(move_to(8)).unwrap();
        assert_eq!(game.scenario.board.unit_at(at(8)).unwrap().moves_left, 0);
    }

    #[test]
    fn a_hidden_exit_blocker_keeps_the_mover_at_the_entrance_without_exposing_the_blocker() {
        let bundle = scenario(ABILITY, "Gg^Vh", true, false);
        let mut game = bundle.start(1, |_| unreachable!()).unwrap();
        let initial = game.decision().unwrap();
        assert!(initial.1.contains(&move_to(8)));
        assert_eq!(game.planned_route(1, at(0), at(8)).unwrap(), [(at(0), 5), (at(8), 3)]);
        game.scenario.board.place_recruit(2, "Mage", at(8)).unwrap();
        assert_eq!(game.decision().unwrap(), initial);
        game.step(move_to(8)).unwrap();
        assert_eq!(game.scenario.board.unit_at(at(8)).unwrap().side, 2);
        assert_eq!(game.scenario.board.unit_at(at(0)).unwrap().moves_left, 5);
        assert_eq!(game.decision().unwrap(), initial);
    }
}
