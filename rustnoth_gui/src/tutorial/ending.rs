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
    super::second::prepare(second, hero);
    second_victory_dialogue(second);
    super::briefing::prepare(second, hero);
    super::reinforcements::prepare(second, hero);
    Ok(lesson)
}

fn second_victory_dialogue(scenario: &mut Node) {
    // The final tutorial is won by defeating the orc leader. Deliver its farewell
    // at that referee event, while Galdrad is still available as the speaker.
    let mut farewell = node(&[("name", "enemies defeated")]);
    if let Some((_, victory)) = scenario.children.iter_mut()
        .find(|(tag, cfg)| tag == "event" && cfg.attribute("name") == Some("victory"))
    {
        (farewell.children, victory.children) = std::mem::take(&mut victory.children)
            .into_iter().partition(|(tag, _)| tag == "message");
        if !farewell.children.is_empty() { scenario.children.push(("event".into(), farewell)); }
    }
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
                        result.children.push(("rustnoth_tutorial_hint".into(), hint.clone()));
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
    use rustnoth_gui::session::{Players, Session};

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
    fn part_two_plays_its_farewell_before_the_final_end_scenario_button() {
        for victim in ["Thrag", "student"] {
            let mut level = wml::parse(r#"map_data="Xv,Xv,Xv,Xv,Xv
Xv,1 Gg,Gg,2 Gg,Xv
Xv,Xv,Xv,Xv,Xv"
[side]
id=student
type=Fighter
controller=human
[unit]
id=Galdrad
name=Galdrad
type=Fighter
x,y=2,1
[/unit]
[/side]
[side]
id=Thrag
type=Fighter
controller=ai
[/side]
[event]
name=victory
[message]
speaker=Galdrad
message=You have beaten the orcs!
[/message]
[message]
speaker=narrator
image=wesnoth-icon.png
message=See the help browser.
[/message]
[progress_achievement]
content_for=tutorial
id=completed
amount=1
[/progress_achievement]
[/event]"#).unwrap();
            second_victory_dialogue(&mut level);
            level.children.push(("event".into(), wml::parse(format!(
                "name=side 1 turn end\n[kill]\nid={victim}\nfire_event=yes\n[/kill]"
            )).unwrap()));
            let bundle = board(level);
            let game = bundle.start_campaign(7, |_| unreachable!(), None).unwrap();
            let mut session = Session::with_players(game, Players::Campaign);
            assert!(!session.final_victory_ready(true));
            session.apply(Action::EndTurn).unwrap();
            assert!(session.next_scenario().is_none());
            if victim == "student" {
                assert!(session.dialogue().is_none());
                assert!(!session.final_victory_ready(true));
                continue;
            }
            let speech = session.dialogue().unwrap();
            assert_eq!(speech.caption.as_deref(), Some("Galdrad"));
            assert_eq!(speech.text, "You have beaten the orcs!");
            assert!(!session.final_victory_ready(true));
            session.dismiss_dialogue();
            let help = session.dialogue().unwrap();
            assert_eq!(help.image.as_deref(), Some("wesnoth-icon.png"));
            assert_eq!(help.text, "See the help browser.");
            assert!(!session.final_victory_ready(true));
            session.dismiss_dialogue();
            assert!(!session.final_victory_ready(false));
            assert!(session.final_victory_ready(true));
        }
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
                let game = first.start_campaign(7, |_| unreachable!(), Some(&mut route)).unwrap();
                let mut session = Session::with_players(game, Players::Campaign);
                assert!(session.next_scenario().is_none());
                session.apply(Action::EndTurn).unwrap();
                assert!(session.tutorial_hint().is_empty());
                if victim == "student" {
                    assert_eq!(session.state(), MatchState::CampaignDefeat);
                    assert!(session.next_scenario().is_none());
                    assert_eq!(session.dialogue().unwrap().text, "Too much for me");
                    session.dismiss_dialogue();
                    assert!(session.dialogue().is_none());
                    assert!(session.tutorial_hint().is_empty());
                } else {
                    assert_eq!(session.state(), MatchState::CampaignVictory);
                    assert_eq!(session.dialogue().unwrap().text, format!("Onwards, {}", hero.name()));
                    session.dismiss_dialogue();
                    assert!(session.dialogue().is_none(), "linger needs no extra dialogue click");
                    assert_eq!(session.tutorial_hint(), "<b><big>Victory</big></b>\nClick End Scenario");
                    assert_eq!(session.next_scenario(), Some("2_Tutorial"));
                    assert!(!session.show_end_report());
                    session.store_campaign_survivors(&mut route);
                    let mut second = base.clone();
                    set_second_hero(&mut second, hero).unwrap();
                    let student = second.children.iter_mut().find(|(_, cfg)| cfg.attribute("id") == Some("student")).unwrap();
                    put(&mut student.1, "experience", "0");
                    route.gold.transfer_to(&mut second);
                    let second = board(second);
                    let game = second.start_campaign(7, |_| unreachable!(), Some(&mut route)).unwrap();
                    let session = Session::with_players(game, Players::Campaign);
                    assert!(session.tutorial_hint().is_empty());
                    let view = session.observation().unwrap();
                    let leader = view.units.iter().find(|u| u.side == 1 && u.can_recruit).unwrap();
                    assert_eq!(leader.type_id, if hero == Hero::Lisar { "Fighteress" } else { "Fighter" });
                    assert_eq!(leader.experience, 7);
                    assert_eq!(view.gold, Some(120));
                }
            }
        }
    }
}
