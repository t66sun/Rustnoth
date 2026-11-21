use std::error::Error;

use engine_lite::{hex::{Direction, Hex}, wml::Node};

use super::{Hero, Lesson, healing, recruiting, put};

pub(super) fn children<'a>(node: &'a Node, tag: &'a str) -> impl Iterator<Item = &'a Node> {
    node.children.iter().filter(move |(name, _)| name == tag).map(|(_, child)| child)
}

pub(super) fn node(attributes: &[(&str, &str)]) -> Node {
    let mut cfg = Node::default();
    for (key, value) in attributes { put(&mut cfg, key, value); }
    cfg
}

pub(super) fn placed(cfg: &Node) -> Option<Hex> {
    Hex::from_wml(cfg.attribute("x")?.trim().parse().ok()?, cfg.attribute("y")?.trim().parse().ok()?)
}

fn position(cfg: &mut Node, hex: Hex) {
    let (x, y) = hex.to_wml().expect("tutorial map coordinates fit in WML");
    put(cfg, "x", &x.to_string());
    put(cfg, "y", &y.to_string());
}

pub(super) fn gender_command<'a>(event: &'a Node, hero: Hero, tag: &'a str) -> Option<&'a Node> {
    children(event, "if")
        .filter(|condition| children(condition, "have_unit").any(|filter| filter.attribute("id") == Some("student")
            && filter.attribute("gender") == Some("male")))
        .flat_map(|condition| children(condition, if hero == Hero::Konrad { "then" } else { "else" }))
        .find_map(|branch| children(branch, tag).next())
}

pub(super) fn dialogue(source: &Node, hero: Hero) -> Node {
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
    // Both native paths move Delfador to the same lookout before the quintain's turn.
    if let Some(movement) = children(attack, "if").flat_map(|condition| children(condition, "then"))
        .flat_map(|branch| children(branch, "move_unit")).find(|cfg| cfg.attribute("id") == Some("Delfador")) {
        let mut relocated = mentor.clone();
        put(&mut relocated, "side", mentor_side);
        put(&mut relocated, "x", movement.attribute("to_x").ok_or("tutorial lookout is missing x")?);
        put(&mut relocated, "y", movement.attribute("to_y").ok_or("tutorial lookout is missing y")?);
        put(&mut relocated, "facing", "nw");
        finish.children.push(("kill".into(), node(&[("id", "Delfador")])));
        finish.children.push(("unit".into(), relocated));
    }
    events.push(("event".into(), finish));
    let healing = healing::Exercises::read(scenario, attack, hero)?;
    let recruiting = recruiting::Exercise::read(scenario, hero, &mut events)?;
    scenario.children.retain(|(tag, _)| tag != "event");
    scenario.children.extend(events);
    scenario.attributes.remove("next_scenario");
    Ok(Lesson::new(mentor_hex, hero, healing, recruiting))
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_lite::{fixed_match::Action, scenario_bundle::Bundle, wml, wml_emit};
    use rustnoth_gui::session::{Players, Session};

    fn gendered(tag: &str, male: &str, female: &str) -> String {
        format!("[if]\n[have_unit]\nid=student\ngender=male\n[/have_unit]\n\
            [then]\n[{tag}]\nspeaker=narrator\nmessage={male}\n[/{tag}]\n[/then]\n\
            [else]\n[{tag}]\nspeaker=narrator\nmessage={female}\n[/{tag}]\n[/else]\n[/if]")
    }

    fn opening() -> Node {
        let intro = gendered("message", "Welcome Konrad", "Welcome Lisar");
        let hint = gendered("hint_message", "Attack with Konrad", "Attack with Lisar");
        let crowns = gendered("hint_message", "Konrad wears a crown", "Lisar wears a crown");
        let villages = "[if]\n[have_unit]\nid=student\ngender=male\n[/have_unit]\n\
            [then]\n[print]\ntext=Move Konrad to a village\n[/print]\n[/then]\n\
            [else]\n[print]\ntext=Move Lisar to a village\n[/print]\n[/else]\n[/if]";
        let keep_prompt = villages.replace("a village", "the keep");
        wml::parse(format!("id=tutorial\nvictory_when_enemies_defeated=no\n\
            map_data=\"Xv,Xv,Xv,Xv,Xv,Xv,Xv\nXv,Vh,Ch,Gg,Gg,Gg,Xv\nXv,1 Kh,Ch,Gg,Gg,Gg,Xv\nXv,Ch,Ch,Gg,Gg,Vh,Xv\nXv,Xv,Xv,Xv,Xv,Xv,Xv\"\n\
            [side]\nside=1\ncontroller=human\ngold=100\nid=student\nname=Konrad\ntype=Fighter\nteam_name=player\n[/side]\n\
            [side]\nside=2\nno_leader=yes\ndefeat_condition=never\n[/side]\n\
            [side]\nside=3\ncontroller=null\nno_leader=yes\nteam_name=player\n\
            [unit]\nid=Delfador\ntype=Elder Mage\nx,y=3,2\n[/unit]\n[/side]\n\
            [event]\nname=start\n{intro}\n[/event]\n\
            [event]\nname=moveto\n[filter]\n[filter_adjacent]\nid=Delfador\n[/filter_adjacent]\n[/filter]\n\
            [unit]\nid=Quintain\ntype=Quintain\nside=2\nx,y=3,2\n[/unit]\n{hint}\n[/event]\n\
            [event]\nname=attack_end\n[filter]\nid=student\n[/filter]\n\
            [message]\nspeaker=student\nmessage=It fights back!\n[/message]\n\
            [message]\nspeaker=Delfador\nmessage=$student_hp to $($student_hp-15)\n[/message]\n\
            [hint_message]\nmessage=Finish your turn\n[/hint_message]\n[print]\ntext=End your turn\n[/print]\n[/event]\n\
            [event]\nname=turn 2\n[message]\nspeaker=student\nmessage=I have $student_hp left\nfemale_message=female^I retain $student_hp\n[/message]\n\
            [label]\nx=1,5\ny=1,3\n[/label]\n{villages}\n[event]\nname=capture\n[hint_message]\nmessage=You captured the village\n[/hint_message]\n[/event]\n[/event]\n\
            [event]\nname=turn 3 refresh\n[message]\nspeaker=Delfador\nmessage=Healed $student_hp_heal_amount\n[/message]\n\
            [message]\nspeaker=student\nmessage=I will recruit elves\nfemale_message=female^I shall recruit elves\n[/message]\n\
            [label]\nx,y=1,2\ntext=Keep\n[/label]\n{keep_prompt}\n\
            [event]\nname=moveto\n[filter]\nx,y=1,2\n[/filter]\n[set_recruit]\nside=1\nrecruit=Elvish Fighter\n[/set_recruit]\n\
            [hint_message]\ncaption=Recruiting\nmessage=Right-click a castle\n[/hint_message]\n[print]\ntext=Recruit two fighters\n[/print]\n[/event]\n\
            [event]\nname=recruit\n[filter_condition]\n[have_unit]\ntype=Elvish Fighter\ncount=2\n[/have_unit]\n[/filter_condition]\n\
            {crowns}\n[/event]\n[/event]")).unwrap()
    }

    #[test]
    fn both_heroes_face_the_quintain_and_heal_on_their_next_turn() {
        let mut units = String::from("[terrain_type]\nid=flat\nstring=Gg\n[/terrain_type]\n\
            [terrain_type]\nstring=Vh\ngives_income=yes\nheals=8\nmvt_alias=Gg\n[/terrain_type]\n\
            [terrain_type]\nstring=Kh\nrecruit_from=yes\nrecruit_onto=yes\nmvt_alias=Gg\n[/terrain_type]\n\
            [terrain_type]\nstring=Ch\nrecruit_onto=yes\nmvt_alias=Gg\n[/terrain_type]\n\
            [units]\n[movetype]\nname=foot\n[movement_costs]\nflat=1\n[/movement_costs]\n[/movetype]\n");
        for id in ["Fighter", "Fighteress", "Elder Mage", "Quintain", "Elvish Fighter"] {
            let moves = if id == "Quintain" { 1 } else { 6 };
            units.push_str(&format!("[unit_type]\nid={id}\ncost=14\nhitpoints=32\nmovement={moves}\nmovement_type=foot\n\
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
            let mut lesson = prepare(&mut scenario, hero).unwrap();
            let mut document = Node::default();
            document.children.push(("scenario".into(), scenario));
            let mut bytes = Vec::new();
            wml_emit::write_config(&mut bytes, &document).unwrap();
            let bundle = Bundle::parse_with_core(&bytes, units.as_bytes()).unwrap();
            let mut session = Session::with_players(bundle.start(9, |_| unreachable!()).unwrap(), Players::Campaign);
            assert_eq!(session.dialogue().unwrap().text, introduction);
            session.dismiss_dialogue();
            assert!(session.legal_actions().contains(&Action::EndTurn));
            assert!(!session.legal_actions().iter().any(|action| matches!(action, Action::Recruit { .. })));
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
            let hp = healing::student_hp(&session).unwrap();
            assert_eq!(session.dialogue().unwrap().text, format!("{hp} to {}", hp - 15));
            session.dismiss_dialogue();
            assert_eq!(session.dialogue().unwrap().text, "Finish your turn");
            session.dismiss_dialogue();
            assert_eq!(session.legal_actions(), &[Action::EndTurn]);
            let next_turn = |lesson: &mut Lesson, session: &mut Session<'_>| {
                session.apply(Action::EndTurn).unwrap();
                lesson.advance(session);
                for _ in 0..12 {
                    if !session.play_computer_action().unwrap() { return; }
                    lesson.after_computer(session);
                }
                panic!("Tutorial never returned control to the student");
            };
            next_turn(&mut lesson, &mut session);
            let hp = healing::student_hp(&session).unwrap();
            assert_eq!(session.dialogue().unwrap().text, match hero {
                Hero::Konrad => format!("I have {hp} left"), Hero::Lisar => format!("I retain {hp}")
            });
            session.dismiss_dialogue();
            assert!(!session.legal_actions().is_empty());
            assert!(session.legal_actions().iter().all(|a| matches!(a, Action::Move {to,..} if lesson.healing.villages.contains(to))));
            session.apply(session.legal_actions()[0].clone()).unwrap();
            lesson.advance(&mut session);
            assert_eq!(session.dialogue().unwrap().text, "You captured the village");
            session.dismiss_dialogue();
            assert_eq!(session.legal_actions(), &[Action::EndTurn]);
            next_turn(&mut lesson, &mut session);
            let recovered = healing::student_hp(&session).unwrap() - lesson.before_healing.unwrap();
            assert_eq!(session.dialogue().unwrap().text, format!("Healed {recovered}"));
            assert!((0..=8).contains(&recovered));
            session.dismiss_dialogue();
            assert_eq!(session.dialogue().unwrap().text, if hero == Hero::Konrad { "I will recruit elves" } else { "I shall recruit elves" });
            session.dismiss_dialogue();
            assert!(!lesson.finished());
            assert_eq!(lesson.recruiting.keep_prompt, format!("Move {} to the keep", if hero == Hero::Konrad { "Konrad" } else { "Lisar" }));
            assert!(!session.legal_actions().contains(&Action::EndTurn));
            let to_keep = session.legal_actions().iter().find(|action| matches!(action, Action::Move { to, .. } if *to == lesson.recruiting.keep)).unwrap().clone();
            session.apply(to_keep).unwrap();
            lesson.advance(&mut session);
            assert_eq!(session.dialogue().unwrap().text, "Right-click a castle");
            session.dismiss_dialogue();
            let gold = session.observation().unwrap().gold.unwrap();
            for n in 1..=2 {
                assert!(!session.legal_actions().contains(&Action::EndTurn));
                let recruit = session.legal_actions().iter().find(|a| matches!(a, Action::Recruit { unit_type, .. } if unit_type == "Elvish Fighter")).unwrap().clone();
                session.apply(recruit).unwrap();
                lesson.advance(&mut session);
                let view = session.observation().unwrap();
                assert_eq!(view.gold, Some(gold - n * 14));
                let fighters: Vec<_> = view.units.iter().filter(|u| u.side == 1 && u.type_id == "Elvish Fighter").collect();
                assert_eq!(fighters.len(), n as usize);
                assert!(fighters.iter().all(|u| u.moves_left == Some(0) && u.attacks_left == Some(0)));
                if n == 1 { assert!(session.dialogue().is_none()); }
            }
            assert_eq!(session.dialogue().unwrap().text, if hero == Hero::Konrad { "Konrad wears a crown" } else { "Lisar wears a crown" });
            session.dismiss_dialogue();
            assert!(session.legal_actions().contains(&Action::EndTurn));
            next_turn(&mut lesson, &mut session);
            assert!(lesson.finished());
        }
    }
}
