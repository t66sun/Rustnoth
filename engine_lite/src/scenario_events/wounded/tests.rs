use crate::{board::AttackChoice, fixed_match::Action, hex::Hex, scenario_bundle::Bundle};

#[test]
fn wounded_elves_speak_after_the_guard_lesson_with_their_own_gender_and_experience() {
    let mut variables = String::new();
    for i in 0..4 {
        for (suffix, prefix) in [("", "Male"), ("_female", "female^Female")] {
            variables.push_str(&format!("[set_variable]\nname=low_hp_unit_message{suffix}[{i}].value\nvalue={prefix} {i}\n[/set_variable]\n"));
        }
    }
    for (turn, hp, xp) in [(2, 2, 94), (3, 20, 94), (3, 2, 92), (3, 2, 94)] {
        let bundle = Bundle::parse(format!(r#"
[units]
[unit_type]
id=Elf
hitpoints=100
experience=100
movement=5
attacks=8
[attack]
name=sword
type=blade
range=melee
damage=1
number=1
[/attack]
[/unit_type]
[unit_type]
id=Orc
hitpoints=100
experience=100
movement=4
[/unit_type]
[/units]
[scenario]
turn_at={turn}
map_data="Xv,Xv,Xv,Xv,Xv
Xv,1 Gg,2 Gg,Gg,Xv
Xv,Gg,Gg,Gg,Xv
Xv,Gg,Gg,Gg,Xv
Xv,Xv,Xv,Xv,Xv"
[side]
side=1
id=student
type=Elf
team_name=elves
[unit]
id=archer
name=Fia
type=Elf
gender=female
hitpoints={hp}
experience={xp}
x,y=2,2
[/unit]
[unit]
id=Galdrad
type=Elf
x,y=1,3
[/unit]
[/side]
[side]
side=2
id=Dumbo
type=Orc
team_name=orcs
[/side]
[event]
name=prestart
{variables}
[/event]
[event]
name=turn 3
[event]
name=attack_end
[filter_second]
id=Dumbo
[/filter_second]
[if]
[then]
[message]
speaker=student
message=Heal me
[/message]
[/then]
[else]
[message]
speaker=student
message=Heal my troops
[/message]
[/else]
[/if]
[event]
name=attack_end
[filter]
side=1
[not]
id=student
[/not]
[/filter]
[fire_event]
name=warn_about_units_with_low_health
[/fire_event]
[/event]
[/event]
[/event]
[event]
name=warn_about_units_with_low_health
[filter_condition]
[variable]
name=unit.hitpoints
less_than="$($unit.max_hitpoints/5)"
[/variable]
[variable]
name=lhpu_msg_i
less_than_equal_to=3
[/variable]
[/filter_condition]
[message]
speaker=unit
message="$low_hp_unit_message[$lhpu_msg_i].value"
female_message="$low_hp_unit_message_female[$lhpu_msg_i].value"
[/message]
[message]
speaker=Galdrad
message=Nearly advanced
[show_if]
[variable]
name=unit.experience
greater_than="$($unit.max_experience-8)"
[/variable]
[/show_if]
[/message]
[/event]
[/scenario]"#).as_bytes()).unwrap();
        let mut game = bundle.start(15, |_| unreachable!()).unwrap();
        let attack = |from| Action::Attack(AttackChoice { from, to: Hex { x:1, y:0 }, weapon:0 });
        game.step(attack(Hex { x:0, y:0 })).unwrap();
        let words = |lines: Vec<crate::fixed_match::dialogue::DialogueLine>| {
            lines.into_iter().map(|line|line.text).collect::<Vec<_>>()
        };
        let introduction = words(game.take_dialogue(1));
        if turn == 3 { assert_eq!(introduction, ["Heal me"]); } else { assert!(introduction.is_empty()); }
        game.step(attack(Hex { x:0, y:0 })).unwrap();
        assert!(game.take_dialogue(1).is_empty(), "the student's own attacks do not consume warnings");
        for i in 0..5 {
            game.step(attack(Hex { x:1, y:1 })).unwrap();
            let lines = game.take_dialogue(1);
            if turn < 3 || hp >= 20 || i == 4 {
                assert!(lines.is_empty());
            } else {
                assert_eq!(lines[0].caption.as_deref(), Some("Fia"));
                assert_eq!(lines[0].focus.unwrap().hex, Hex { x:1, y:1 });
                assert_eq!(lines[0].text, format!("Female {i}"));
                assert_eq!(lines.iter().any(|l| l.text == "Nearly advanced"), xp > 92);
            }
        }
    }
}
