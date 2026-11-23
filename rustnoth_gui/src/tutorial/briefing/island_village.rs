use super::{Hero, Node, children, project};

pub(super) fn prepare(scenario: &mut Node, hero: Hero) {
    for (tag, event) in &mut scenario.children {
        if tag != "event" || event.attribute("name") != Some("capture") { continue; }
        // The island congratulation has a conditional TALK_ABOUT_LOC pointing to the ford.
        let has_ford_reminder = children(event, "if").any(|branch|
            children(branch, "have_location").next().is_some() && children(branch, "then")
                .any(|body| children(body, "scroll_to").any(|scroll| scroll.attribute("highlight") == Some("yes"))));
        if has_ford_reminder { project(event, hero); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_lite::{fixed_match::Action, hex::Hex, scenario_bundle::Bundle, wml, wml_emit};
    use rustnoth_gui::session::{Players, Session};

    #[test]
    fn taking_the_island_shows_the_ford_only_while_the_student_does_not_own_it() {
        const CORE: &[u8] = b"[units]\n[unit_type]\nid=Scout\nhitpoints=30\nmovement=5\n\
            [movement_costs]\nflat=1\n[/movement_costs]\n[/unit_type]\n[/units]\n\
            [terrain_type]\nid=flat\nstring=Gg\n[/terrain_type]\n\
            [terrain_type]\nid=village\nstring=^Vh\naliasof=_bas\ngives_income=yes\n[/terrain_type]";
        // First capture is the island. The village next to it represents the northern ford.
        const OWN_FORD: &str = "[village]\nx,y=3,1\n[/village]";
        for (owner, expects_reminder) in [(None, true), (Some(1), false), (Some(2), true)] {
            let student = if owner == Some(1) { OWN_FORD } else { "" };
            let opponent = if owner == Some(2) { OWN_FORD } else { "" };
            let mut doc = wml::parse(format!(r#"[scenario]
id=2_Tutorial
map_data="Xv,Xv,Xv,Xv,Xv,Xv
Xv,1 Gg,Gg^Vh,Gg^Vh,2 Gg,Xv
Xv,Xv,Xv,Xv,Xv,Xv"
[side]
id=Galdrad
name=Galdrad
type=Scout
controller=human
{student}
[/side]
[side]
id=Thrag
type=Scout
controller=ai
{opponent}
[/side]
[event]
name=capture
[filter]
side=1
x,y=2,1
[/filter]
[message]
speaker=Galdrad
message=The island village is ours!
[/message]
[if]
[have_location]
x,y=3,1
[not]
owner_side=1
[/not]
[/have_location]
[then]
[scroll_to]
x,y=3,1
highlight=yes
[/scroll_to]
[message]
speaker=Galdrad
scroll=no
highlight=no
message=Remember the village near the channel.
[/message]
[/then]
[/if]
[/event]
[/scenario]"#)).unwrap();
            super::super::prepare(&mut doc.children[0].1, Hero::Konrad);
            let mut bytes = Vec::new();
            wml_emit::write_config(&mut bytes, &doc).unwrap();
            let bundle = Bundle::parse_with_core(&bytes, CORE).unwrap();
            let game = bundle.start(7, |_| unreachable!()).unwrap();
            let mut session = Session::with_players(game, Players::Campaign);
            assert!(session.dialogue().is_none());
            let island = Hex { x: 1, y: 0 };
            session.apply(Action::Move { from: Hex { x: 0, y: 0 }, to: island }).unwrap();
            let congratulations = session.dialogue().unwrap();
            assert_eq!(congratulations.text, "The island village is ours!");
            assert_eq!(congratulations.focus.unwrap().hex, island);
            session.dismiss_dialogue();
            if expects_reminder {
                let reminder = session.dialogue().unwrap();
                assert_eq!(reminder.text, "Remember the village near the channel.");
                assert_eq!(reminder.focus.unwrap().hex, Hex { x: 2, y: 0 });
                session.dismiss_dialogue();
            }
            assert!(session.dialogue().is_none());
        }
    }
}
