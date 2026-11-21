//! Project the stock first quintain's death into battlefield commands.
use engine_lite::wml::Node;
use rustnoth_gui::session::Session;

use super::{Hero, opening::{children, dialogue, gender_command, node}};

pub(super) fn death_event(scenario: &Node, hero: Hero) -> Option<Node> {
    let source = children(scenario, "event").find(|event| event.attribute("name") == Some("die")
        && children(event, "filter").any(|filter| filter.attribute("id") == Some("Quintain")))?;
    let farewell = gender_command(source, hero, "message");
    let mut event = node(&[("name", "die")]);
    for (tag, cfg) in &source.children {
        match tag.as_str() {
            "filter" | "unit" | "modify_side" | "kill" => event.children.push((tag.clone(), cfg.clone())),
            "message" => event.children.push((tag.clone(), dialogue(cfg, hero))),
            "if" if children(cfg, "have_unit").any(|f| f.attribute("id") == Some("student")
                && f.attribute("gender") == Some("male")) => {
                if let Some(line) = farewell {
                    event.children.push(("message".into(), dialogue(line, hero)));
                }
            }
            _ => {}
        }
    }
    Some(event)
}

pub(super) fn guardians_have_arrived(session: &Session<'_>) -> bool {
    // This unshrouded exercise starts with one quintain. Its die event places
    // the three practice guardians before control returns to either side.
    session.observation().is_some_and(|view| view.units.iter()
        .filter(|u| u.side == 2 && u.type_id == "Quintain").nth(1).is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_lite::{board::AttackChoice, fixed_match::{Action, MatchState}, hex::Hex, scenario_bundle::Bundle, wml, wml_emit};

    fn battlefield(hero: Hero) -> Bundle {
        let scenario = wml::parse(r#"map_data="Xv,Xv,Xv,Xv,Xv,Xv,Xv,Xv
Xv,1 Gg,Gg,Gg,Gg,Gg,Gg,Xv
Xv,Gg,Gg,Gg,Gg,Gg,Gg,Xv
Xv,Gg,Gg,Gg,Gg,Gg,Gg,Xv
Xv,Xv,Xv,Xv,Xv,Xv,Xv,Xv"
[side]
side=1
type=Fighter
team_name=player
[unit]
type=Elf
name=Elandil
profile=elf.webp
x,y=2,2
[/unit]
[/side]
[side]
side=2
no_leader=yes
defeat_condition=never
[unit]
id=Quintain
type=Quintain
x,y=3,2
[/unit]
[/side]
[side]
side=3
team_name=player
no_leader=yes
[unit]
id=Delfador
type=Mage
x,y=1,3
[/unit]
[/side]
[event]
name=die
[filter]
id=Quintain
[/filter]
[message]
speaker=second_unit
message=I gained experience.
[/message]
[message]
speaker=Delfador
message=You gain experience through battle.
[/message]
[if]
[have_unit]
id=student
gender=male
[/have_unit]
[then]
[message]
speaker=Delfador
message="Practice now, Konrad."
[/message]
[/then]
[else]
[message]
speaker=Delfador
message="Practice now, Li'sar."
[/message]
[/else]
[/if]
[unit]
type=Quintain
ai_special=guardian
side=2
x,y=4,3
[/unit]
[unit]
type=Quintain
ai_special=guardian
side=2
x,y=5,3
[/unit]
[unit]
type=Quintain
ai_special=guardian
side=2
x,y=6,3
[/unit]
[modify_side]
side=2
defeat_condition=no_units_left
[/modify_side]
[kill]
id=Delfador
[/kill]
[message]
speaker=narrator
message=Practice one at a time.
[/message]
[/event]"#).unwrap();
        let event = death_event(&scenario, hero).unwrap();
        assert!(children(&event, "unit").all(|unit| unit.attribute("ai_special") == Some("guardian")));
        let mut scenario = scenario;
        scenario.children.retain(|(tag, _)| tag != "event");
        scenario.children.push(("event".into(), event));
        scenario.children.push(("event".into(), wml::parse("name=turn 3\n[kill]\nside=2\n[/kill]").unwrap()));
        let mut source = Node::default();
        source.children.push(("scenario".into(), scenario));
        let mut bytes = Vec::new();
        wml_emit::write_config(&mut bytes, &source).unwrap();
        let mut units = String::from("[terrain_type]\nid=flat\nstring=Gg\n[/terrain_type]\n[units]\n");
        for (id, hp, damage) in [("Fighter", 20, 5), ("Mage", 20, 6), ("Elf", 24, 8), ("Quintain", 4, 1)] {
            units.push_str(&format!("[unit_type]\nid={id}\nhitpoints={hp}\n[attack]\nname=touch\ntype=impact\nrange=melee\ndamage={damage}\nnumber=1\naccuracy=100\n[/attack]\n[/unit_type]\n"));
        }
        units.push_str("[/units]");
        Bundle::parse_with_core(&bytes, units.as_bytes()).unwrap()
    }

    #[test]
    fn the_quintain_killer_speaks_and_the_three_practice_guardians_replace_it() {
        for (hero, farewell) in [(Hero::Konrad, "Practice now, Konrad."), (Hero::Lisar, "Practice now, Li'sar.")] {
            for counterattack in [false, true] {
                let bundle = battlefield(hero);
                let mut game = bundle.start(7, |_| unreachable!()).unwrap();
                let elf = Hex::from_wml(2, 2).unwrap();
                let dummy = Hex::from_wml(3, 2).unwrap();
                let (from, to) = if counterattack {
                    game.step(Action::EndTurn).unwrap();
                    (dummy, elf)
                } else { (elf, dummy) };
                assert!(matches!(game.step(Action::Attack(AttackChoice { from, to, weapon: 0 })).unwrap(), MatchState::Playing(_)));
                let lines = game.take_dialogue(1);
                assert_eq!(lines.iter().map(|line| line.text.as_str()).collect::<Vec<_>>(),
                    ["I gained experience.", "You gain experience through battle.", farewell, "Practice one at a time."]);
                assert_eq!((lines[0].speaker_type.as_deref(), lines[0].caption.as_deref(), lines[0].image.as_deref()),
                    (Some("Elf"), Some("Elandil"), Some("elf.webp")));
                let view = game.observation_for(1).unwrap();
                assert!(!view.units.iter().any(|u| u.type_id == "Mage" || u.position == dummy));
                let mut guards: Vec<_> = view.units.iter().filter(|u| u.side == 2).map(|u| u.position).collect();
                guards.sort();
                assert_eq!(guards, [4, 5, 6].map(|x| Hex::from_wml(x, 3).unwrap()));
                assert!(view.units.iter().any(|u| u.position == elf && u.experience > 0));
                for _ in 0..6 {
                    if !matches!(game.state(), MatchState::Playing(_)) { break; }
                    game.step(Action::EndTurn).unwrap();
                }
                // The test's turn 3 removes the remaining guardians. Side 2
                // would survive indefinitely with its original defeat condition.
                assert_eq!(game.state(), MatchState::Won(1));
            }
        }
    }
}
