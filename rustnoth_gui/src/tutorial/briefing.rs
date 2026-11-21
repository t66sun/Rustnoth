//! Project Part II's welcome from the installed campaign, keeping recall checks in the referee.
use super::{Hero, opening::children};
use engine_lite::wml::Node;

pub(super) fn prepare(scenario: &mut Node, hero: Hero) {
    let Some((_, start)) = scenario.children.iter_mut().find(|(tag, event)|
        tag == "event" && event.attribute("name") == Some("start")
            && children(event, "hint_message").next().is_some()
            && children(event, "if").flat_map(|cfg| children(cfg, "have_unit"))
                .any(|filter| filter.attribute("search_recall_list") == Some("yes"))) else { return };
    project(start, hero);
}

fn project(commands: &mut Node, hero: Hero) {
    for (tag, mut cfg) in std::mem::take(&mut commands.children) {
        match tag.as_str() {
            "if" if children(&cfg, "have_unit").any(|unit| unit.attribute("id") == Some("student")
                && unit.attribute("gender") == Some("male")) => {
                let chosen = if hero == Hero::Konrad { "then" } else { "else" };
                if let Some((_, mut branch)) = cfg.children.into_iter().find(|(name, _)| name == chosen) {
                    project(&mut branch, hero);
                    commands.children.extend(branch.children);
                }
            }
            "if" => {
                for (name, branch) in &mut cfg.children {
                    if matches!(name.as_str(), "then" | "else") { project(branch, hero); }
                }
                commands.children.push((tag, cfg));
            }
            "message" => {
                if let Some(female) = cfg.attributes.remove("female_message") && hero == Hero::Lisar {
                    cfg.attributes.insert("message".into(), female);
                }
                if let Some(text) = cfg.attributes.get_mut("message") && text.text.starts_with("female^") {
                    let prefix = "female^".len();
                    text.text.drain(..prefix);
                    for segment in &mut text.translations {
                        segment.bytes = segment.bytes.start.saturating_sub(prefix)..segment.bytes.end.saturating_sub(prefix);
                    }
                }
                if let Some(index) = cfg.children.iter().position(|(name, _)| name == "show_if") {
                    let (_, mut condition) = cfg.children.remove(index);
                    let mut branch = Node::default();
                    branch.children.push((tag, cfg));
                    condition.children.push(("then".into(), branch));
                    commands.children.push(("if".into(), condition));
                } else {
                    commands.children.push((tag, cfg));
                }
            }
            // This action is defined in tutorial Lua. Only project the installed briefing's hint.
            "hint_message" => commands.children.push(("rustnoth_tutorial_hint".into(), cfg)),
            "print" => {}
            _ => commands.children.push((tag, cfg)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_lite::{fixed_match::{Action, carryover::CampaignCarryover}, scenario_bundle::Bundle, wml, wml_emit};
    use rustnoth_gui::session::{Players, Session};

    const MAP: &str = "map_data=\"Xv,Xv,Xv,Xv,Xv\nXv,1 Gg,Gg,2 Gg,Xv\nXv,Xv,Xv,Xv,Xv\"";
    const BRIEFING: &str = r#"
[event]
name=start
[message]
speaker=narrator
caption=Welcome to Wesnoth!
image=wesnoth-icon.png
message=Welcome back!
[/message]
[message]
speaker=student
message="Ho, Galdrad!"
female_message="female^Greetings, Galdrad!"
[/message]
[if]
[have_unit]
id=student
gender=male
[/have_unit]
[then]
[message]
speaker=Galdrad
message="This is no game, Konrad!"
[/message]
[/then]
[else]
[message]
speaker=Galdrad
message="female^This is no game, Li’sar!"
[/message]
[/else]
[/if]
[scroll_to_unit]
id=Thrag
highlight=yes
[/scroll_to_unit]
[message]
speaker=Galdrad
scroll=no
highlight=no
message="The more likely attack, then, is across the bridge."
[/message]
[if]
[have_unit]
side=1
x,y=recall,recall
search_recall_list=yes
[/have_unit]
[then]
[message]
speaker=Galdrad
message=I see you have veteran troops from your training!
[/message]
[message]
speaker=Galdrad
message=Some of your units have also managed to level up.
[show_if]
[have_unit]
side=1
x,y=recall,recall
level=2,3
search_recall_list=yes
[/have_unit]
[/show_if]
[/message]
[/then]
[else]
[message]
speaker=Galdrad
message="Unfortunately, none of your troops survived your training."
[/message]
[/else]
[/if]
[hint_message]
caption=Recalling
message=Recall your experienced veterans.
[/hint_message]
[message]
speaker=Galdrad
message=Recruit a balanced force.
[/message]
[print]
text=Recruit or recall your troops
[/print]
[/event]
[event]
name=start
[gold]
side=1
amount=5
[/gold]
[/event]"#;

    fn board(scenario: Node) -> Bundle {
        let mut doc = Node::default();
        doc.children.push(("scenario".into(), scenario));
        let mut bytes = Vec::new();
        wml_emit::write_config(&mut bytes, &doc).unwrap();
        let mut core = String::from("[units]\n");
        for (kind, level) in [("Fighter", 2), ("Fighteress", 2), ("Elf", 1), ("Hero", 2), ("Champion", 3)] {
            core.push_str(&format!("[unit_type]\nid={kind}\nlevel={level}\nhitpoints=30\n[/unit_type]\n"));
        }
        core.push_str("[/units]");
        Bundle::parse_with_core(&bytes, core.as_bytes()).unwrap()
    }

    fn scenario(hero: Hero, extra: &str, events: &str) -> Node {
        let kind = if hero == Hero::Konrad { "Fighter" } else { "Fighteress" };
        wml::parse(format!("{MAP}\n[side]\nid=student\ntype={kind}\nname={}\ncontroller=human\nsave_id=human_player\ngold=120\n{extra}\n[/side]\n\
            [side]\nid=Thrag\ntype=Hero\n[/side]\n{events}", hero.name())).unwrap()
    }

    #[test]
    fn galdrad_briefs_each_student_using_only_survivors_in_the_recall_list() {
        for hero in [Hero::Konrad, Hero::Lisar] {
            for kind in [None, Some("Elf"), Some("Hero"), Some("Champion")] {
                let veteran = kind.map(|kind| format!("[unit]\nid=Veteran\ntype={kind}\nx,y=2,1\n[/unit]")).unwrap_or_default();
                let mut first = scenario(hero, &veteran,
                    "[event]\nname=side 1 turn end\n[endlevel]\nresult=victory\n[/endlevel]\n[/event]");
                super::super::put(&mut first, "next_scenario", "2_Tutorial");
                let first = board(first);
                let mut route = CampaignCarryover::default();
                let mut match_one = first.start(7, |_| unreachable!()).unwrap();
                match_one.step(Action::EndTurn).unwrap();
                assert_eq!(match_one.next_scenario_after_victory(), Some("2_Tutorial"));
                match_one.store_campaign_survivors(&mut route);

                let mut second = scenario(hero, "[unit]\nid=Galdrad\nname=Galdrad\ntype=Champion\nx,y=2,1\n[/unit]", BRIEFING);
                prepare(&mut second, hero);
                prepare(&mut second, hero);
                let second = board(second);
                let mut session = Session::with_players(second.start_campaign(7, |_| unreachable!(), Some(&mut route)).unwrap(), Players::Campaign);
                assert_eq!(session.observation().unwrap().gold, Some(125));
                assert_eq!(session.recall_info("Veteran").map(|unit| unit.type_id), kind);
                let welcome = session.dialogue().unwrap();
                assert_eq!(welcome.caption.as_deref(), Some("Welcome to Wesnoth!"));
                assert_eq!(welcome.image.as_deref(), Some("wesnoth-icon.png"));
                let mut expected = vec![
                    "Welcome back!",
                    if hero == Hero::Konrad { "Ho, Galdrad!" } else { "Greetings, Galdrad!" },
                    if hero == Hero::Konrad { "This is no game, Konrad!" } else { "This is no game, Li’sar!" },
                    "The more likely attack, then, is across the bridge.",
                    if kind.is_some() { "I see you have veteran troops from your training!" } else {
                        "Unfortunately, none of your troops survived your training."
                    },
                ];
                if matches!(kind, Some("Hero" | "Champion")) {
                    expected.push("Some of your units have also managed to level up.");
                }
                for message in expected {
                    assert_eq!(session.dialogue().unwrap().text, message);
                    assert!(session.tutorial_hint().is_empty());
                    session.dismiss_dialogue();
                }
                assert_eq!(session.dialogue().unwrap().text, "Recruit a balanced force.");
                assert_eq!(session.dialogue().unwrap().caption.as_deref(), Some("Galdrad"));
                let hint = "<b><big>Recalling</big></b>\nRecall your experienced veterans.";
                assert_eq!(session.tutorial_hint(), hint);
                session.dismiss_dialogue();
                assert!(session.dialogue().is_none());
                assert_eq!(session.tutorial_hint(), hint);
            }
        }
    }
}
