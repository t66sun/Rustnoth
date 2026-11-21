//! Preserve the tutorial's authored outcomes when projecting its Lua-driven lessons.
use super::{FIRST_SCENARIO, Hero, Lesson, opening::{self, children, dialogue, node}, put};
use engine_lite::wml::Node;
use std::error::Error;

pub(crate) fn prepare_campaign(campaign: &mut Node, hero: Hero) -> Result<Lesson, Box<dyn Error>> {
    let first = scenario(campaign, FIRST_SCENARIO)?;
    let next = first.attribute("next_scenario").ok_or("Battle Training has no second scenario")?.to_owned();
    let lesson = opening::prepare(first, hero)?;
    let second = scenario(campaign, &next)?;
    set_second_hero(second, hero)?;
    Ok(lesson)
}

fn scenario<'a>(campaign: &'a mut Node, id: &str) -> Result<&'a mut Node, Box<dyn Error>> {
    campaign.children.iter_mut().find(|(tag, cfg)| tag == "scenario" && cfg.attribute("id") == Some(id))
        .map(|(_, cfg)| cfg).ok_or_else(|| format!("Missing Battle Training scenario {id}").into())
}

fn set_second_hero(scenario: &mut Node, hero: Hero) -> Result<(), Box<dyn Error>> {
    let student = scenario.children.iter_mut().find(|(tag, side)| tag == "side" && side.attribute("id") == Some("student"))
        .map(|(_, side)| side).ok_or("Missing Battle Training student in Part II")?;
    if hero == Hero::Lisar {
        put(student, "type", "Fighteress");
        put(student, "name", hero.name());
        put(student, "profile", "portraits/lisar.webp");
    }
    Ok(())
}

pub(super) fn append_events(scenario: &Node, hero: Hero, events: &mut Vec<(String, Node)>) {
    let victory_hint = children(scenario, "event").find(|event| event.attribute("name") == Some("victory"))
        .and_then(|event| children(event, "hint_message").next());
    for event in children(scenario, "event") {
        let name = event.attribute("name").unwrap_or_default();
        let student = children(event, "filter").any(|cfg| cfg.attribute("id") == Some("student"));
        if name != "enemies defeated" && !(student && matches!(name, "last_breath" | "last breath" | "die")) {
            continue;
        }
        let mut result = node(&[("name", name)]);
        for (tag, cfg) in &event.children {
            match tag.as_str() {
                "filter" => result.children.push((tag.clone(), cfg.clone())),
                "message" => {
                    let mut line = dialogue(cfg, hero);
                    if student && line.attribute("speaker") == Some("unit") { put(&mut line, "speaker", "student"); }
                    result.children.push((tag.clone(), line));
                }
                "endlevel" => {
                    result.children.push((tag.clone(), cfg.clone()));
                    if name == "enemies defeated" && let Some(hint) = victory_hint {
                        let mut line = dialogue(hint, hero);
                        put(&mut line, "speaker", "narrator");
                        result.children.push(("message".into(), line));
                    }
                }
                _ => {}
            }
        }
        events.push(("event".into(), result));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_lite::{fixed_match::{Action, MatchState, carryover::CampaignCarryover}, scenario_bundle::Bundle, wml, wml_emit};

    const CORE: &str = "[units]\n[unit_type]\nid=Fighter\nhitpoints=24\n[/unit_type]\n\
        [unit_type]\nid=Fighteress\nhitpoints=24\n[/unit_type]\n[/units]";

    fn board(scenario: Node) -> Bundle {
        let mut doc = Node::default();
        doc.children.push(("scenario".into(), scenario));
        let mut bytes = Vec::new();
        wml_emit::write_config(&mut bytes, &doc).unwrap();
        Bundle::parse_with_core(&bytes, CORE.as_bytes()).unwrap()
    }

    #[test]
    fn only_a_surviving_student_enters_part_two_with_the_same_roster() {
        for hero in [Hero::Konrad, Hero::Lisar] {
            let base = wml::parse("map_data=\"Xv,Xv,Xv,Xv,Xv\nXv,1 Gg,Gg,2 Gg,Xv\nXv,Xv,Xv,Xv,Xv\"\n\
                [side]\nid=student\ntype=Fighter\nname=Konrad\ncontroller=human\nsave_id=human_player\ngold=120\nexperience=7\n[/side]\n\
                [side]\nid=last_guard\ntype=Fighter\n[/side]").unwrap();
            for victim in ["last_guard", "student"] {
                let mut first = base.clone();
                set_second_hero(&mut first, hero).unwrap();
                put(&mut first, "victory_when_enemies_defeated", "no");
                put(&mut first, "next_scenario", "2_Tutorial");
                let script = wml::parse("[event]\nname=enemies defeated\n[message]\nspeaker=student\nmessage=Onwards, Konrad\nfemale_message=female^Onwards, Li’sar\n[/message]\n\
                    [endlevel]\nresult=victory\ncarryover_report=no\ncarryover_percentage=0\ncarryover_add=yes\n[/endlevel]\n[/event]\n\
                    [event]\nname=victory\n[hint_message]\ncaption=Victory\nmessage=Click End Scenario\n[/hint_message]\n[/event]\n\
                    [event]\nname=last_breath\n[filter]\nid=student\n[/filter]\n[message]\nspeaker=unit\nmessage=Too much for me\n[/message]\n[/event]\n\
                    [event]\nname=die\n[filter]\nid=student\n[/filter]\n[endlevel]\nresult=defeat\n[/endlevel]\n[/event]").unwrap();
                append_events(&script, hero, &mut first.children);
                first.children.push(("event".into(), wml::parse(format!("name=side 1 turn end\n[kill]\nid={victim}\nfire_event=yes\n[/kill]")).unwrap()));
                let first = board(first);
                let mut route = CampaignCarryover::default();
                let mut game = first.start_campaign(7, |_| unreachable!(), Some(&mut route)).unwrap();
                assert!(game.next_scenario_after_victory().is_none());
                game.step(Action::EndTurn).unwrap();
                let lines = game.take_dialogue(1).into_iter().map(|line| line.text).collect::<Vec<_>>();
                if victim == "student" {
                    assert_eq!(game.state(), MatchState::CampaignDefeat);
                    assert!(game.next_scenario_after_victory().is_none());
                    assert_eq!(lines, ["Too much for me"]);
                } else {
                    assert_eq!(game.state(), MatchState::CampaignVictory);
                    assert_eq!(game.next_scenario_after_victory(), Some("2_Tutorial"));
                    assert_eq!(lines, [format!("Onwards, {}", hero.name()), "Click End Scenario".into()]);
                    assert!(!game.show_end_report());
                    game.store_campaign_survivors(&mut route);
                    let mut second = base.clone();
                    set_second_hero(&mut second, hero).unwrap();
                    let student = second.children.iter_mut().find(|(_, cfg)| cfg.attribute("id") == Some("student")).unwrap();
                    put(&mut student.1, "experience", "0");
                    route.gold.transfer_to(&mut second);
                    let second = board(second);
                    let game = second.start_campaign(7, |_| unreachable!(), Some(&mut route)).unwrap();
                    let view = game.observation_for(1).unwrap();
                    let leader = view.units.iter().find(|u| u.side == 1 && u.can_recruit).unwrap();
                    assert_eq!(leader.type_id, if hero == Hero::Lisar { "Fighteress" } else { "Fighter" });
                    assert_eq!(leader.experience, 7);
                    assert_eq!(view.gold, Some(120));
                }
            }
        }
    }
}
