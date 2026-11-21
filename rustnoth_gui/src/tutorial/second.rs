//! Project the official second tutorial's opening objectives for the chosen student.
use super::{Hero, put};
use engine_lite::wml::Node;

pub(super) fn prepare(scenario: &mut Node, hero: Hero) {
    let Some((event, command)) = scenario.children.iter().enumerate().find_map(|(index, (tag, cfg))| {
        if tag != "event" || cfg.attribute("name") != Some("prestart") { return None; }
        cfg.children.iter().position(|(name, goals)| name == "objectives" && goals.attribute("side") == Some("1"))
            .map(|command| (index, command))
    }) else { return };
    // Its other prestart commands include Lua-backed actions. An unsupported
    // command would otherwise suppress the entire handler, including these goals.
    let mut goals = scenario.children[event].1.children.remove(command);
    let gender = match hero { Hero::Konrad => "male", Hero::Lisar => "female" };
    goals.1.children.retain_mut(|(tag, objective)| {
        if tag != "objective" { return true; }
        let [(condition, test)] = objective.children.as_slice() else { return true };
        let [(predicate, unit)] = test.children.as_slice() else { return true };
        if condition != "show_if" || predicate != "have_unit" || !test.attributes.is_empty()
            || !unit.children.is_empty() || unit.attributes.len() != 2 || unit.attribute("id") != Some("student")
        {
            return true;
        }
        let Some(required) = unit.attribute("gender") else { return true };
        if required != gender { return false; }
        objective.children.clear();
        true
    });
    let mut setup = Node::default();
    put(&mut setup, "name", "prestart");
    setup.children.push(goals);
    scenario.children.insert(event, ("event".into(), setup));
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_lite::{scenario_bundle::Bundle, wml, wml_emit};
    use rustnoth_gui::session::{Players, Session};

    #[test]
    fn second_tutorial_both_students_see_their_own_objectives_in_the_opening_session() {
        let units = b"[units]\n[unit_type]\nid=Scout\nhitpoints=20\n[/unit_type]\n[/units]";
        let source = r#"[scenario]
id=2_Tutorial
turns=36
map_data="Xv,Xv,Xv,Xv
Xv,1 Gg,2 Gg,Xv
Xv,Xv,Xv,Xv"
[side]
controller=human
id=student
type=Scout
[/side]
[side]
controller=ai
id=Thrag
type=Scout
[/side]
[event]
name=prestart
[objectives]
side=1
[objective]
condition=win
description=Defeat the Orc Leader
[/objective]
[objective]
condition=lose
description=Death of Konrad
[show_if]
[have_unit]
id=student
gender=male
[/have_unit]
[/show_if]
[/objective]
[objective]
condition=lose
description=Death of Li’sar
[show_if]
[have_unit]
id=student
gender=female
[/have_unit]
[/show_if]
[/objective]
[objective]
condition=lose
description=Death of Galdrad
[/objective]
[objective]
condition=lose
description=Turns run out
show_turn_counter=yes
[/objective]
[/objectives]
[disallow_end_turn]
reason=Recruit and capture a village
[/disallow_end_turn]
[/event]
[/scenario]"#;
        for (hero, loss) in [(Hero::Konrad, "Death of Konrad"), (Hero::Lisar, "Death of Li’sar")] {
            let mut document = wml::parse(source).unwrap();
            prepare(&mut document.children[0].1, hero);
            let mut bytes = Vec::new();
            wml_emit::write_config(&mut bytes, &document).unwrap();
            let bundle = Bundle::parse_with_core(&bytes, units).unwrap();
            let session = Session::with_players(bundle.start(1, |_| unreachable!()).unwrap(), Players::Campaign);
            let objectives = session.objectives().unwrap();
            assert_eq!(objectives.victory, ["Defeat the Orc Leader"]);
            assert_eq!(objectives.defeat, [loss, "Death of Galdrad", "Turns run out"]);
        }
    }
}
