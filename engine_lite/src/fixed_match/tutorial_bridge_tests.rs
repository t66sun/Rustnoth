//! The Part II side-end event is evaluated after the guardian AI has acted.
use super::{Action, MatchState};
use crate::{hex::{Direction, Hex}, scenario_bundle::Bundle, turn::Turn};

const BRIDGE_EVENT: &str = r#"
[event]
    name=side 2 turn 2 end
    [if]
        [have_unit]
            id=Dumbo
            x,y=15,12
        [/have_unit]
        [not]
            [have_unit]
                x=12-14
                y=9-12
            [/have_unit]
        [/not]
        [then]
            [move_unit]
                id=Dumbo
                to_x,to_y=10,9
            [/move_unit]
        [/then]
    [/if]
    [if]
        [have_unit]
            id=Dumbo
        [/have_unit]
        [not]
            [have_unit]
                id=Dumbo
                x=1-12
            [/have_unit]
        [/not]
        [not]
            [have_unit]
                side=1
                [filter_adjacent]
                    id=Dumbo
                [/filter_adjacent]
            [/have_unit]
        [/not]
        [then]
            [message]
                speaker=narrator
                message=The orc should have crossed the bridge
            [/message]
        [/then]
    [/if]
[/event]"#;

fn scenario(from: (i32, i32), elf: Option<(i32, i32)>) -> Bundle {
    let mut rows = vec![vec!["Gg"; 19]; 16];
    rows[1][1] = "1 Gg";
    rows[14][17] = "2 Gg";
    // Northwest of the requested landing is impassable.
    rows[9][9] = "Xu";
    let map = rows.into_iter().map(|row| row.join(",")).collect::<Vec<_>>().join("\n");
    let elf = elf.map_or_else(String::new, |(x,y)| format!("[unit]\nid=elf\ntype=Elf\nx,y={x},{y}\n[/unit]"));
    let source = format!(r#"
[terrain_type]
    id=flat
    string=Gg
[/terrain_type]
[terrain_type]
    id=wall
    string=Xu
[/terrain_type]
[units]
    [unit_type]
        id=Elf
        hitpoints=30
        movement=5
    [/unit_type]
    [unit_type]
        id=Grunt
        hitpoints=38
        movement=5
        [movement_costs]
            flat=1
            wall=99
        [/movement_costs]
    [/unit_type]
[/units]
[scenario]
    map_data="{map}"
    [side]
        type=Elf
        {elf}
    [/side]
    [side]
        type=Elf
        [unit]
            id=Dumbo
            type=Grunt
            experience=17
            x,y={x},{y}
        [/unit]
    [/side]
    {BRIDGE_EVENT}
[/scenario]"#, x=from.0, y=from.1);
    Bundle::parse(source.as_bytes()).unwrap()
}

#[test]
fn the_bridge_guard_moves_after_the_second_enemy_turn_unless_blocked_or_already_engaged() {
    let home = (15,12);
    for (from, elf, expected, warned) in [
        (home, None, (10,9), false),
        (home, Some((12,10)), home, true),
        (home, Some((14,12)), home, false),
        ((11,10), None, (11,10), false),
        (home, Some((10,9)), (9,10), false),
    ] {
        let data = scenario(from, elf);
        let mut game = data.start(3, |_| unreachable!()).unwrap();
        for _ in 0..3 { game.step(Action::EndTurn).unwrap(); }
        assert_eq!(game.state(), MatchState::Playing(Turn { side: 2, round: 2 }));
        let position = |(x,y)| Hex::from_wml(x,y).unwrap();
        let unit = game.scenario.board.unit_at(position(from)).unwrap();
        let stats = (unit.hitpoints, unit.moves_left, unit.attacks_left, unit.experience);
        assert!(game.take_dialogue(1).is_empty());
        game.step(Action::EndTurn).unwrap();
        let unit = game.scenario.board.unit_at(position(expected)).unwrap();
        assert_eq!(unit.id.as_deref(), Some("Dumbo"));
        assert_eq!((unit.hitpoints, unit.moves_left, unit.attacks_left, unit.experience), stats);
        if from != expected { assert_eq!(unit.facing, Some(Direction::SouthWest)); }
        let lines = game.take_dialogue(1);
        assert_eq!(lines.len(), usize::from(warned));
        if warned { assert_eq!(lines[0].text, "The orc should have crossed the bridge"); }
    }
}
