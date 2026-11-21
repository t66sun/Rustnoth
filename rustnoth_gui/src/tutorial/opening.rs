use std::error::Error;

use engine_lite::{hex::{Direction, Hex}, wml::Node};

use super::{Hero, Lesson, put};

fn children<'a>(node: &'a Node, tag: &'a str) -> impl Iterator<Item = &'a Node> {
    node.children.iter().filter(move |(name, _)| name == tag).map(|(_, child)| child)
}

fn node(attributes: &[(&str, &str)]) -> Node {
    let mut cfg = Node::default();
    for (key, value) in attributes { put(&mut cfg, key, value); }
    cfg
}

fn placed(cfg: &Node) -> Option<Hex> {
    Hex::from_wml(cfg.attribute("x")?.trim().parse().ok()?, cfg.attribute("y")?.trim().parse().ok()?)
}

fn position(cfg: &mut Node, hex: Hex) {
    let (x, y) = hex.to_wml().expect("tutorial map coordinates fit in WML");
    put(cfg, "x", &x.to_string());
    put(cfg, "y", &y.to_string());
}

fn gender_command<'a>(event: &'a Node, hero: Hero, tag: &'a str) -> Option<&'a Node> {
    children(event, "if")
        .filter(|condition| children(condition, "have_unit").any(|filter| filter.attribute("id") == Some("student")
            && filter.attribute("gender") == Some("male")))
        .flat_map(|condition| children(condition, if hero == Hero::Konrad { "then" } else { "else" }))
        .find_map(|branch| children(branch, tag).next())
}

fn dialogue(source: &Node, hero: Hero) -> Node {
    let mut line = source.clone();
    if let Some(female) = line.attributes.remove("female_message") && hero == Hero::Lisar {
        put(&mut line, "message", female.text.strip_prefix("female^").unwrap_or(&female.text));
    }
    line
}

/// Project just the opening exercises into commands the Rust referee can execute.
/// In particular the original Lua character selection, adjacent-unit filter and MOVE_UNIT
/// macro must be resolved before handing the scenario to the fixed-match event reader.
pub(super) fn prepare(scenario: &mut Node, hero: Hero) -> Result<Lesson, Box<dyn Error>> {
    let student = scenario.children.iter_mut().find(|(tag, side)| tag == "side" && side.attribute("id") == Some("student"))
        .ok_or("tutorial has no student")?;
    if hero == Hero::Lisar {
        put(&mut student.1, "type", "Fighteress");
        put(&mut student.1, "name", hero.name());
        put(&mut student.1, "profile", "portraits/lisar.webp");
    }
    let (mentor_side, mentor) = children(scenario, "side").find_map(|side| {
        children(side, "unit").find(|unit| unit.attribute("id") == Some("Delfador"))
            .map(|unit| (side.attribute("side").unwrap_or("3"), unit))
    }).ok_or("tutorial has no Delfador")?;
    let mentor_hex = placed(mentor).ok_or("tutorial mentor has no starting hex")?;

    let start = children(scenario, "event").find(|event| event.attribute("name") == Some("start"))
        .ok_or("missing opening tutorial dialogue")?;
    let welcome = gender_command(start, hero, "message").ok_or("missing hero introduction")?;
    let mut intro = node(&[("name", "start")]);
    intro.children.push(("message".into(), dialogue(welcome, hero)));

    let meet = children(scenario, "event").find(|event| event.attribute("name") == Some("moveto")
        && children(event, "filter").any(|filter| children(filter, "filter_adjacent")
            .any(|adjacent| adjacent.attribute("id") == Some("Delfador"))))
        .ok_or("missing meet-Delfador event")?;
    let quintain = children(meet, "unit").find(|it| it.attribute("id") == Some("Quintain") && placed(it) == Some(mentor_hex))
        .ok_or("missing summoned quintain")?;
    let mut hint = gender_command(meet, hero, "hint_message").ok_or("missing attack instructions")?.clone();
    put(&mut hint, "speaker", "narrator");

    let mut events = vec![("event".into(), intro)];
    for prestart in children(scenario, "event").filter(|event| event.attribute("name") == Some("prestart")) {
        // Character selection replaces the Lua setup but the student's objectives still apply.
        let mut setup = node(&[("name", "prestart")]);
        setup.children.extend(prestart.children.iter().filter(|(tag, _)| tag == "objectives").cloned());
        if !setup.children.is_empty() {
            events.push(("event".into(), setup));
        }
    }
    for destination in mentor_hex.neighbors() {
        let mut event = node(&[("name", "moveto")]);
        let mut filter = node(&[("id", "student"), ("side", "1")]);
        position(&mut filter, destination);
        event.children.push(("filter".into(), filter));
        for (tag, command) in &meet.children {
            if tag == "message" {
                event.children.push((tag.clone(), dialogue(command, hero)));
            } else if tag == "unit" && command == quintain {
                // Stock MOVE_UNIT steps northeast unless the student occupies that hex;
                // the quintain appears on the mentor's old hex in both branches.
                let northeast = mentor_hex.neighbor(Direction::NorthEast).ok_or("invalid mentor position")?;
                let direction = if destination == northeast { Direction::SouthEast } else { Direction::NorthEast };
                let mut moved = mentor.clone();
                put(&mut moved, "side", mentor_side);
                position(&mut moved, mentor_hex.neighbor(direction).ok_or("invalid summoning position")?);
                event.children.push(("kill".into(), node(&[("id", "Delfador")])));
                event.children.push(("unit".into(), moved));
                event.children.push(("unit".into(), quintain.clone()));
            }
        }
        event.children.push(("message".into(), hint.clone()));
        events.push(("event".into(), event));
    }
    let attack = children(scenario, "event").find(|event| event.attribute("name") == Some("attack_end")
        && children(event, "filter").any(|filter| filter.attribute("id") == Some("student"))
        && children(event, "message").next().is_some()).ok_or("missing first attack response")?;
    let mut finish = node(&[("name", "attack_end")]);
    finish.children.push(("filter".into(), node(&[("id", "student")])));
    finish.children.push(("filter_second".into(), node(&[("id", "Quintain")])));
    for line in children(attack, "message").take(2) {
        finish.children.push(("message".into(), dialogue(line, hero)));
    }
    finish.children.push(("message".into(), node(&[("speaker", "narrator"), ("caption", "Battle Training"),
        ("message", "Opening exercises complete. You moved your hero next to Delfador and attacked the quintain.")])));
    events.push(("event".into(), finish));
    scenario.children.retain(|(tag, _)| tag != "event");
    scenario.children.extend(events);
    scenario.attributes.remove("next_scenario");
    Ok(Lesson::new(mentor_hex, hero))
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_lite::{fixed_match::Action, scenario_bundle::Bundle, wml, wml_emit};
    use rustnoth_gui::session::Session;

    fn gendered(tag: &str, male: &str, female: &str) -> String {
        format!("[if]\n[have_unit]\nid=student\ngender=male\n[/have_unit]\n\
            [then]\n[{tag}]\nspeaker=narrator\nmessage={male}\n[/{tag}]\n[/then]\n\
            [else]\n[{tag}]\nspeaker=narrator\nmessage={female}\n[/{tag}]\n[/else]\n[/if]")
    }

    fn opening() -> Node {
        let intro = gendered("message", "Welcome Konrad", "Welcome Lisar");
        let hint = gendered("hint_message", "Attack with Konrad", "Attack with Lisar");
        wml::parse(format!("id=tutorial\nvictory_when_enemies_defeated=no\n\
            map_data=\"Xv,Xv,Xv,Xv,Xv,Xv,Xv\nXv,Gg,Gg,Gg,Gg,Gg,Xv\nXv,1 Gg,Gg,Gg,Gg,Gg,Xv\nXv,Gg,Gg,Gg,Gg,Gg,Xv\nXv,Xv,Xv,Xv,Xv,Xv,Xv\"\n\
            [side]\nside=1\ncontroller=human\nid=student\nname=Konrad\ntype=Fighter\nteam_name=player\n[/side]\n\
            [side]\nside=2\nno_leader=yes\ndefeat_condition=never\n[/side]\n\
            [side]\nside=3\nno_leader=yes\nteam_name=player\n\
            [unit]\nid=Delfador\ntype=Elder Mage\nx,y=3,2\n[/unit]\n[/side]\n\
            [event]\nname=start\n{intro}\n[/event]\n\
            [event]\nname=moveto\n[filter]\n[filter_adjacent]\nid=Delfador\n[/filter_adjacent]\n[/filter]\n\
            [unit]\nid=Quintain\ntype=Quintain\nside=2\nx,y=3,2\n[/unit]\n{hint}\n[/event]\n\
            [event]\nname=attack_end\n[filter]\nid=student\n[/filter]\n\
            [message]\nspeaker=student\nmessage=It fights back!\n[/message]\n[/event]")).unwrap()
    }

    #[test]
    fn both_heroes_move_beside_the_mentor_summon_the_dummy_and_finish_by_attacking_it() {
        let mut units = String::from("[terrain_type]\nid=flat\nstring=Gg\n[/terrain_type]\n\
            [units]\n[movetype]\nname=foot\n[movement_costs]\nflat=1\n[/movement_costs]\n[/movetype]\n");
        for id in ["Fighter", "Fighteress", "Elder Mage", "Quintain"] {
            units.push_str(&format!("[unit_type]\nid={id}\nhitpoints=32\nmovement=6\nmovement_type=foot\n\
                [attack]\nname=sword\nrange=melee\ntype=blade\ndamage=1\nnumber=1\n[/attack]\n[/unit_type]\n"));
        }
        units.push_str("[/units]");
        let center = Hex::from_wml(3, 2).unwrap();
        let ne = center.neighbor(Direction::NorthEast).unwrap();
        for (hero, to, introduction, hint) in [
            (Hero::Konrad, center.neighbor(Direction::SouthWest).unwrap(), "Welcome Konrad", "Attack with Konrad"),
            (Hero::Lisar, ne, "Welcome Lisar", "Attack with Lisar"),
        ] {
            let mut scenario = opening();
            let summary = "You will learn the basics of:\n• <small>Movement</small>\n• <small>Attacking</small>\n• <small>Healing</small>\n• <small>Recruiting</small>";
            scenario.children.extend(wml::parse(format!(r#"[event]
name=prestart
[store_unit]
variable=student_store
[/store_unit]
[objectives]
side=1
summary= _ "{summary}"
[objective]
description= _ "Destroy a fierce enemy"
condition=win
[/objective]
[objective]
description= _ "Get yourself killed"
condition=lose
[/objective]
[/objectives]
[/event]"#)).unwrap().children);
            let mut lesson = prepare(&mut scenario, hero).unwrap();
            let mut document = Node::default();
            document.children.push(("scenario".into(), scenario));
            let mut bytes = Vec::new();
            wml_emit::write_config(&mut bytes, &document).unwrap();
            let bundle = Bundle::parse_with_core(&bytes, units.as_bytes()).unwrap();
            let mut session = Session::new(bundle.start(9, |_| unreachable!()).unwrap());
            assert_eq!(session.dialogue().unwrap().text, introduction);
            session.dismiss_dialogue();
            assert!(session.has_objective_notice());
            let goals = session.objectives().unwrap();
            assert_eq!(goals.summary, summary);
            assert_eq!(goals.victory, ["Destroy a fierce enemy"]);
            assert_eq!(goals.defeat, ["Get yourself killed"]);
            session.acknowledge_objectives();
            assert!(session.legal_actions().contains(&Action::EndTurn));
            lesson.restrict(&mut session);
            assert!(session.legal_actions().iter().all(|action| matches!(action, Action::Move { to, .. } if to.distance(center) == 1)));
            let movement = session.legal_actions().iter().find(|action| matches!(action, Action::Move { to: dst, .. } if *dst == to)).unwrap().clone();
            session.apply(movement).unwrap();
            lesson.advance(&mut session);
            assert_eq!(session.dialogue().unwrap().text, hint);
            session.dismiss_dialogue();
            let view = session.observation().unwrap();
            let mentor = view.units.iter().find(|u| u.type_id == "Elder Mage").unwrap();
            assert_eq!(mentor.position, if to == ne { center.neighbor(Direction::SouthEast).unwrap() } else { ne });
            assert!(view.units.iter().any(|u| u.position == to && u.type_id == if hero == Hero::Konrad { "Fighter" } else { "Fighteress" }));
            assert!(matches!(session.legal_actions(), [Action::Attack(attack)] if attack.to == center && attack.from == to));
            session.apply(session.legal_actions()[0].clone()).unwrap();
            lesson.advance(&mut session);
            assert_eq!(session.dialogue().unwrap().text, "It fights back!");
            session.dismiss_dialogue();
            assert!(session.dialogue().unwrap().text.contains("Opening exercises complete"));
            assert!(lesson.finished());
        }
    }
}
