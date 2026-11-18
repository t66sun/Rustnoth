use super::{Action, FixedMatch};
use crate::{board::AttackChoice, hex::Hex, scenario_bundle::Bundle};

const DEFAULT_AMLA: &str = r#"
[advancement]
    id=amla_default
    strict_amla=yes
    max_times=100
    [effect]
        apply_to=hitpoints
        increase_total=3
        heal_full=yes
    [/effect]
    [effect]
        apply_to=max_experience
        increase=20%
    [/effect]
    [effect]
        apply_to=status
        remove=poisoned
    [/effect]
    [effect]
        apply_to=status
        remove=slowed
    [/effect]
[/advancement]"#;

fn guards(experience_limit: i32, saved_advancements: usize, advances_to: &str) -> Bundle {
    let saved = DEFAULT_AMLA.repeat(saved_advancements);
    let wml = format!(r#"
[units]
    [unit_type]
        id=Guard
        level=4
        hitpoints=28
        experience={experience_limit}
        advances_to={advances_to}
        {DEFAULT_AMLA}
        [attack]
            range=melee
            damage=1
            number=1
            accuracy=100
        [/attack]
    [/unit_type]
    [unit_type]
        id=Veteran
        hitpoints=35
    [/unit_type]
[/units]
[scenario]
    map_data="Xv,Xv,Xv,Xv
Xv,1 Gg,2 Gg,Xv
Xv,Xv,Xv,Xv"
    [side]
        side=1
        type=Guard
        experience=2
        hitpoints=14
        [status]
            slowed=yes
            poisoned=yes
        [/status]
        [modifications]
            {saved}
        [/modifications]
    [/side]
    [side]
        side=2
        type=Guard
        experience=2
        hitpoints=15
        [status]
            slowed=yes
            poisoned=yes
        [/status]
    [/side]
[/scenario]"#);
    Bundle::parse(wml.as_bytes()).unwrap()
}

fn attack(game: &mut FixedMatch<'_>, from_x: i32, to_x: i32) {
    game.step(Action::Attack(AttackChoice {
        from: Hex { x: from_x, y: 0 }, to: Hex { x: to_x, y: 0 }, weapon: 0,
    })).unwrap();
}

#[test]
fn default_amla_heals_both_survivors_and_uses_each_rounded_threshold_for_the_next_award() {
    let bundle = guards(3, 0, "null");
    let mut game = bundle.start(5489, |_| unreachable!()).unwrap();
    attack(&mut game, 0, 1);
    for x in 0..=1 {
        let unit = game.scenario.board.unit_at(Hex { x, y: 0 }).unwrap();
        assert_eq!((unit.hitpoints, unit.max_hitpoints(), unit.experience, unit.max_experience()), (31, 31, 3, 4));
        assert!(!unit.poisoned && !unit.slowed);
        assert_eq!(unit.unit_type.id, "Guard");
        assert_eq!(unit.attacks_left, if x == 0 { 0 } else { 1 });
    }
    game.step(Action::EndTurn).unwrap();
    attack(&mut game, 1, 0);
    for x in 0..=1 {
        let unit = game.scenario.board.unit_at(Hex { x, y: 0 }).unwrap();
        assert_eq!((unit.hitpoints, unit.max_hitpoints(), unit.experience, unit.max_experience()), (34, 34, 3, 5));
    }
}

#[test]
fn saved_default_advancements_count_toward_the_native_limit() {
    let bundle = guards(1, 100, "null");
    let mut game = bundle.start(5489, |_| unreachable!()).unwrap();
    let start = game.scenario.board.unit_at(Hex { x: 0, y: 0 }).unwrap();
    assert_eq!((start.hitpoints, start.max_hitpoints(), start.max_experience()), (14, 328, 1));
    attack(&mut game, 0, 1);
    let guard = game.scenario.board.unit_at(Hex { x: 0, y: 0 }).unwrap();
    assert_eq!((guard.max_hitpoints(), guard.experience), (328, 6));
    assert!(guard.poisoned && guard.slowed);
}

#[test]
fn a_normal_level_up_takes_precedence_over_strict_amla() {
    let bundle = guards(3, 0, "Veteran");
    let mut game = bundle.start(5489, |_| unreachable!()).unwrap();
    attack(&mut game, 0, 1);
    let unit = game.scenario.board.unit_at(Hex { x: 0, y: 0 }).unwrap();
    assert_eq!(unit.unit_type.id, "Veteran");
    assert_eq!((unit.hitpoints, unit.max_hitpoints(), unit.experience), (35, 35, 3));
}
