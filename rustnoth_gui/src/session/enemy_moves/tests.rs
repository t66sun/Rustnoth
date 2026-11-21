use super::*;
use engine_lite::scenario_bundle::Bundle;

fn at(x: i32) -> Hex { Hex { x: x - 1, y: 0 } }

fn scenario(fog: bool, shroud: bool, second_enemy: i32) -> Bundle {
    let map = (1..=7).map(|x| {
        if x == 1 { "1 Gg" } else if x == second_enemy { "2 Gg" } else { "Gg" }
    }).collect::<Vec<_>>().join(",");
    let border = ["Xv"; 9].join(",");
    Bundle::parse(format!(r#"
[terrain_type]
id=flat
string=Gg
[/terrain_type]
[units]
[unit_type]
id=Scout
movement=3
vision=0
hitpoints=20
level=1
[movement_costs]
flat=1
[/movement_costs]
[/unit_type]
[/units]
[scenario]
map_data="{border}
Xv,{map},Xv
{border}"
[side]
side=1
type=Scout
fog={fog}
shroud={shroud}
[/side]
[side]
side=2
type=Scout
[/side]
[event]
name=start
[unit]
side=2
type=Scout
x,y=2,1
moves=0
[/unit]
[/event]
[/scenario]"#).as_bytes()).unwrap()
}

#[test]
fn command_counts_and_highlights_opponents_then_allows_single_unit_inspection() {
    let bundle = scenario(false, false, 5);
    let mut session = Session::new(bundle.start(1, |_| unreachable!()).unwrap());
    assert!(session.can_show_enemy_moves());
    session.show_enemy_moves(Some(at(4)));
    let reach = session.enemy_reach().unwrap();
    assert_eq!(reach.counts[&at(4)], 2);
    assert_eq!(reach.counts[&at(5)], 2); // includes the other enemy's occupied hex
    assert!(!reach.counts.contains_key(&at(1)));
    assert_eq!(reach.threatening.iter().copied().collect::<Vec<_>>(), [at(2), at(5)]);

    session.show_enemy_moves(Some(at(2)));
    session.hover_enemy_moves(Some(at(5)), false);
    assert_eq!(session.enemy_reach().unwrap().counts[&at(4)], 2);
    session.hover_enemy_moves(Some(at(4)), false);
    session.hover_enemy_moves(Some(at(5)), false);
    assert_eq!(session.enemy_reach().unwrap().counts[&at(4)], 1);
    session.dismiss_enemy_moves(Some(at(5)));
    session.hover_enemy_moves(Some(at(5)), false);
    assert!(session.enemy_reach().is_none());
}

#[test]
fn range_ignores_fogged_units_and_never_walks_into_unexplored_terrain() {
    for shroud in [false, true] {
        let mut ranges = Vec::new();
        for second_enemy in [5, 7] {
            let bundle = scenario(true, shroud, second_enemy);
            let session = Session::new(bundle.start(1, |_| unreachable!()).unwrap());
            let view = session.observation().unwrap();
            assert_eq!(view.units.iter().filter(|u| u.side == 2).count(), 1);
            let reach = session.game.enemy_reach(view.side, None, Some(at(4)));
            assert_eq!(reach.counts.get(&at(2)), Some(&1));
            assert_eq!(reach.counts.get(&at(4)), (!shroud).then_some(&1));
            assert!(reach.counts.keys().all(|h| view.tiles.iter().any(|t| t.position == *h)));
            ranges.push(reach);
        }
        assert_eq!(ranges[0], ranges[1]);
    }
}
