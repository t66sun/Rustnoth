use engine_lite::{fixed_match::Action, hex::Hex, scenario_bundle::Bundle};

const SOURCE: &str = r#"[terrain_type]
string=Gg
id=flat
[/terrain_type]
[terrain_type]
string=Ww
id=shallow_water
[/terrain_type]
[units]
[movetype]
name=foot
[movement_costs]
flat=1
shallow_water=1
[/movement_costs]
[/movetype]
[unit_type]
id=Guard
movement_type=foot
movement=5
[/unit_type]
[/units]
[scenario]
map_data="Xv,Xv,Xv,Xv,Xv,Xv,Xv
Xv,1 Gg,Ww,Gg,Ww,2 Gg,Xv
Xv,Xv,Xv,Xv,Xv,Xv,Xv"
[side]
type=Guard
[/side]
[side]
type=Guard
[/side]
[event]
name=prestart
[object]
silent=yes
[filter]
side=2
canrecruit=yes
[/filter]
[effect]
apply_to=movement_costs
replace=yes
[movement_costs]
shallow_water=99
[/movement_costs]
[/effect]
[/object]
[objectives]
side=1
[objective]
description=Bring Deoran to the Citadel of Westin
condition=win
[/objective]
[/objectives]
[/event]
[/scenario]"#;

fn movement(a: i32, b: i32) -> Action {
    Action::Move { from: Hex { x: a, y: 0 }, to: Hex { x: b, y: 0 } }
}

#[test]
fn a_prestart_movement_object_restricts_the_bandit_leader_and_keeps_the_following_objectives() {
    let bundle = Bundle::parse(SOURCE.as_bytes()).unwrap();
    let mut game = bundle.start(1, |_| unreachable!()).unwrap();
    assert!(game.has_authored_objectives(1));
    assert!(game.decision().unwrap().1.contains(&movement(0, 1)));
    game.step(Action::EndTurn).unwrap();
    assert!(!game.decision().unwrap().1.contains(&movement(4, 3)));
    assert!(game.move_preview(Hex { x: 4, y: 0 }, Hex { x: 3, y: 0 }).is_none());
}

#[test]
fn object_uses_the_first_created_matching_unit_even_if_another_is_to_its_left() {
    let source = SOURCE.replace("type=Guard\n[/side]\n[event]", "type=Guard\n[unit]\nid=second\ntype=Guard\ncanrecruit=yes\nx,y=3,1\n[/unit]\n[/side]\n[event]");
    let bundle = Bundle::parse(source.as_bytes()).unwrap();
    let mut game = bundle.start(1, |_| unreachable!()).unwrap();
    game.step(Action::EndTurn).unwrap();
    let (_, actions) = game.decision().unwrap();
    assert!(!actions.contains(&movement(4, 3)));
    assert!(actions.contains(&movement(2, 1)));
}
