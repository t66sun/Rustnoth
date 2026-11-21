use super::*;
use engine_lite::{fixed_match::{Action, MatchError}, hex::Hex, scenario_bundle::Bundle, wml, wml_emit};
use rustnoth_gui::session::{Players, Session};

fn script() -> Node {
    let mut scenario = wml::parse(r#"
map_data="Xv,Xv,Xv,Xv,Xv,Xv,Xv,Xv,Xv
Xv,1 Kh,Ch,Ch,Ch,Ch,Ch,2 Kh,Xv
Xv,Gg^Vh,Gg,Gg,Gg,Gg,Gg,Gg,Xv
Xv,Xv,Xv,Xv,Xv,Xv,Xv,Xv,Xv"
[side]
id=student
controller=human
type=Student
gold=120
recruit="Elf,Archer,Shaman"
[unit]
id=veteran
type=Elf
placement=recall
[/unit]
[/side]
[side]
type=Student
[/side]
[event]
name=prestart
[disallow_end_turn]
reason=Recruit and capture.
[/disallow_end_turn]
[/event]
[event]
name=start
[print]
text=Recruit or recall.
[/print]
[/event]
[event]
name=recruit
[filter]
side=1
type=Archer
[/filter]
[message]
speaker=narrator
image=$unit.profile
caption=Elvish Archer
message=Use your bow.
[/message]
[/event]
[event]
name=recruit
[filter]
side=1
type=Shaman
[/filter]
[filter_condition]
[have_unit]
ability_type=heals
count=2
[/have_unit]
[/filter_condition]
[message]
speaker=narrator
message=Two healers.
[/message]
[/event]
[event]
name="recruit,recall"
[filter]
side=1
[/filter]
[filter_condition]
[variable]
name=turn_number
numerical_equals=1
[/variable]
[have_unit]
side=1
race=elf
count=5
[/have_unit]
[/filter_condition]
[message]
speaker=narrator
message=Capture for income.
[/message]
[print]
text=Capture a village.
[/print]
[disallow_end_turn]
reason=Still need a village.
[/disallow_end_turn]
[/event]
[event]
name=capture
first_time_only=no
[filter]
side=1
[/filter]
[filter_condition]
[variable]
name=turn_number
numerical_equals=1
[/variable]
[/filter_condition]
[if]
[not]
[have_unit]
side=1
race=elf
count=5
[/have_unit]
[/not]
[then]
[print]
text=Forgot the troops.
[/print]
[disallow_end_turn]
reason=Still need recruits.
[/disallow_end_turn]
[allow_undo]
[/allow_undo]
[/then]
[else]
[message]
speaker=narrator
message=Village income.
[/message]
[hint_message]
message=Income and upkeep.
[/hint_message]
[allow_end_turn]
[/allow_end_turn]
[print]
text=End your turn.
[/print]
[/else]
[/if]
[on_undo]
[print]
text=Recruit or recall.
[/print]
[/on_undo]
[/event]
[event]
name=moveto
first_time_only=no
[filter]
id=student
x=1-40
y=1-20
[not]
x=1,6
y=2,2
[/not]
[/filter]
[filter_condition]
[variable]
name=turn_number
numerical_equals=1
[/variable]
[/filter_condition]
[if]
[not]
[have_unit]
side=1
race=elf
count=5
[/have_unit]
[/not]
[then]
[print]
text=Return to recruit.
[/print]
[allow_undo]
[/allow_undo]
[on_undo]
[print]
text=Recruit or recall.
[/print]
[/on_undo]
[/then]
[else]
[print]
text=Return to capture.
[/print]
[allow_undo]
[/allow_undo]
[on_undo]
[print]
text=Capture a village.
[/print]
[/on_undo]
[/else]
[/if]
[/event]"#).unwrap();
    let start = scenario.children.iter_mut().find(|(_, e)| e.attribute("name") == Some("start")).unwrap();
    briefing::project(&mut start.1, Hero::Konrad);
    prepare(&mut scenario, Hero::Konrad);
    scenario
}

fn game() -> Bundle {
    let mut source = Node::default();
    source.children.push(("scenario".into(), script()));
    let mut bytes = Vec::new();
    wml_emit::write_config(&mut bytes, &source).unwrap();
    let mut types = String::from("[units]\n");
    for (kind, race, abilities) in [("Student", "human", ""), ("Elf", "elf", ""), ("Archer", "elf", ""),
        ("Shaman", "elf", "[abilities]\n[heals]\nid=healing\n[/heals]\n[/abilities]")] {
        types.push_str(&format!("[unit_type]\nid={kind}\nrace={race}\nhitpoints=32\ncost=5\nmovement=5\n\
            [movement_costs]\nflat=1\ncastle=1\n[/movement_costs]\n{abilities}\n[/unit_type]\n"));
    }
    types.push_str("[/units]\n[terrain_type]\nid=castle\nstring=Kh\nrecruit_from=yes\nrecruit_onto=yes\n[/terrain_type]\n\
        [terrain_type]\nid=castle\nstring=Ch\nrecruit_onto=yes\n[/terrain_type]\n\
        [terrain_type]\nid=flat\nstring=Gg\n[/terrain_type]\n\
        [terrain_type]\nid=village\nstring=^Vh\naliasof=_bas\ngives_income=yes\n[/terrain_type]");
    Bundle::parse_with_core(&bytes, types.as_bytes()).unwrap()
}

#[test]
fn galdrad_counts_placed_recruits_and_recalls_before_allowing_the_first_turn_to_end() {
    let bundle = game();
    let mut session = Session::with_players(bundle.start(8, |_| unreachable!()).unwrap(), Players::Campaign);
    let capture = Action::Move { from: Hex { x: 0, y: 0 }, to: Hex { x: 0, y: 1 } };
    let mut early = Session::with_players(bundle.start(8, |_| unreachable!()).unwrap(), Players::Campaign);
    early.apply(capture.clone()).unwrap();
    assert_eq!(early.tutorial_print().unwrap().attribute("text"), Some("Forgot the troops."));
    assert_eq!(early.tutorial_end_turn_denial().unwrap().attribute("reason"), Some("Still need recruits."));
    assert_eq!(early.apply(Action::EndTurn), Err(MatchError::ActionUnavailable));

    assert_eq!(session.tutorial_print().unwrap().attribute("text"), Some("Recruit or recall."));
    assert_eq!(session.tutorial_end_turn_denial().unwrap().attribute("reason"), Some("Recruit and capture."));
    assert!(!session.legal_actions().contains(&Action::EndTurn));
    for (index, kind) in ["Archer", "Archer", "Shaman", "Shaman"].iter().enumerate() {
        let to = Hex { x: index as i32 + 1, y: 0 };
        session.apply(Action::Recruit { unit_type: (*kind).into(), to }).unwrap();
        match index {
            0 => {
                let line = session.dialogue().unwrap();
                assert_eq!(line.caption.as_deref(), Some("Elvish Archer"));
                assert_eq!(line.speaker_type.as_deref(), Some("Archer"));
                assert_eq!(line.text, "Use your bow.");
                session.dismiss_dialogue();
            }
            3 => {
                assert_eq!(session.dialogue().unwrap().text, "Two healers.");
                session.dismiss_dialogue();
            }
            _ => assert!(session.dialogue().is_none()),
        }
        assert_eq!(session.tutorial_print().unwrap().attribute("text"), Some("Recruit or recall."));
    }
    session.apply(Action::Recall { unit_id: "veteran".into(), to: Hex { x: 5, y: 0 } }).unwrap();
    assert_eq!(session.dialogue().unwrap().text, "Capture for income.");
    session.dismiss_dialogue();
    assert_eq!(session.tutorial_print().unwrap().attribute("text"), Some("Capture a village."));
    assert_eq!(session.tutorial_end_turn_denial().unwrap().attribute("reason"), Some("Still need a village."));
    assert_eq!(session.apply(Action::EndTurn), Err(MatchError::ActionUnavailable));

    let restart = || Ok(Session::with_players(bundle.start(8, |_| unreachable!())?, Players::Campaign));
    let mut before = restart().unwrap();
    for command in session.history() { command.apply(&mut before).unwrap(); }
    session.apply(Action::Move { from: Hex { x: 0, y: 0 }, to: Hex { x: 1, y: 1 } }).unwrap();
    assert_eq!(session.tutorial_print().unwrap().attribute("text"), Some("Return to capture."));
    assert_eq!(session.undo_tutorial_move(restart).unwrap(), Some(Hex { x: 0, y: 0 }));
    assert_eq!(session.observation(), before.observation());
    assert_eq!(session.tutorial_print().unwrap().attribute("text"), Some("Capture a village."));

    session.apply(capture).unwrap();
    assert_eq!(session.dialogue().unwrap().text, "Village income.");
    session.dismiss_dialogue();
    assert_eq!(session.tutorial_hint(), "Income and upkeep.");
    assert_eq!(session.tutorial_print().unwrap().attribute("text"), Some("End your turn."));
    assert!(!session.can_undo_tutorial_move(), "successful capture is outside the undo instruction");
    assert!(session.tutorial_end_turn_denial().is_none());
    assert!(session.legal_actions().contains(&Action::EndTurn));
    session.apply(Action::EndTurn).unwrap();
    assert!(session.tutorial_print().is_none());
}

#[test]
fn undo_rewinds_only_the_last_wrong_move_and_runs_its_own_print_handler() {
    use rustnoth_gui::save_replay::Event;
    let bundle = game();
    let restart = || Ok(Session::with_players(bundle.start(8, |_| unreachable!())?, Players::Campaign));
    let mut game = restart().unwrap();
    let keep = Hex { x: 0, y: 0 };
    let first = Hex { x: 1, y: 0 };
    let second = Hex { x: 2, y: 0 };
    let before = restart().unwrap();
    game.apply(Action::Move { from: keep, to: first }).unwrap();
    assert_eq!(game.tutorial_print().unwrap().attribute("text"), Some("Return to recruit."));
    let mut after_first = restart().unwrap();
    after_first.apply(Action::Move { from: keep, to: first }).unwrap();
    game.apply(Action::Move { from: first, to: second }).unwrap();
    assert_eq!(game.undo_tutorial_move(restart).unwrap(), Some(first));
    assert_eq!(game.observation(), after_first.observation());
    assert_eq!(game.tutorial_print().unwrap().attribute("text"), Some("Recruit or recall."));
    let mut loaded = restart().unwrap();
    for command in game.history() {
        Event::decode(&command.encode()).unwrap().apply(&mut loaded).unwrap();
    }
    assert_eq!(loaded.tutorial_print(), game.tutorial_print());
    assert_eq!(loaded.undo_tutorial_move(restart).unwrap(), Some(keep));
    assert_eq!(loaded.observation(), before.observation());
    assert!(!loaded.can_undo_tutorial_move());

    loaded.apply(Action::Move { from: keep, to: Hex { x: 0, y: 1 } }).unwrap();
    assert_eq!(loaded.tutorial_print().unwrap().attribute("text"), Some("Forgot the troops."));
    assert_eq!(loaded.undo_tutorial_move(restart).unwrap(), Some(keep));
    assert_eq!(loaded.observation(), before.observation(), "early capture restores village ownership and movement");
    assert_eq!(loaded.tutorial_end_turn_denial().unwrap().attribute("reason"), Some("Recruit and capture."));
}
