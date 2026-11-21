//! Project the installed Part II briefings, keeping recall checks in the referee.
use super::{Hero, opening::children};
use engine_lite::wml::Node;

pub(super) fn prepare(scenario: &mut Node, hero: Hero) {
    for (tag, event) in &mut scenario.children {
        if tag != "event" || children(event, "hint_message").next().is_none() { continue; }
        let briefing = match event.attribute("name") {
            Some("start") => children(event, "if").flat_map(|cfg| children(cfg, "have_unit"))
                .any(|filter| filter.attribute("search_recall_list") == Some("yes")),
            Some("turn 2") => true,
            _ => false,
        };
        if briefing { project(event, hero); }
    }
}

pub(super) fn project(commands: &mut Node, hero: Hero) {
    let mut scroll = None;
    for (tag, mut cfg) in std::mem::take(&mut commands.children) {
        match tag.as_str() {
            "scroll_to" | "scroll_to_unit" if cfg.attribute("highlight") == Some("yes") => {
                scroll = Some(cfg);
            }
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
                // TALK_ABOUT's scroll explicitly overrides the speaker for just this line.
                let focus = scroll.take().or_else(|| {
                    let id = cfg.attribute("speaker")?;
                    (id != "narrator" && cfg.attribute("highlight") != Some("no"))
                        .then(|| super::opening::node(&[("id", id), ("only_if_offscreen", "yes")]))
                });
                if let Some(focus) = focus {
                    cfg.children.push(("rustnoth_tutorial_focus".into(), focus));
                }
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
            "print" => commands.children.push(("rustnoth_tutorial_print".into(), cfg)),
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
    fn second_round_strategy_shows_the_guard_then_leaves_the_long_distance_hint_on_the_board() {
        use engine_lite::{fixed_match::dialogue::DialogueFocus, hex::Hex};
        let strategy = r#"
[event]
name=turn 2
[if]
    [have_unit]
        id=student
        gender=male
    [/have_unit]
    [then]
        [message]
            speaker=Galdrad
            message="Young man, let us discuss strategy."
        [/message]
    [/then]
    [else]
        [message]
            speaker=Galdrad
            message="Young lady, let us discuss strategy."
        [/message]
    [/else]
[/if]
[scroll_to_unit]
    id=Dumbo
    highlight=yes
[/scroll_to_unit]
[message]
    speaker=Galdrad
    highlight=no
    scroll=no
    message=Archers can engage this grunt with little risk.
[/message]
[hint_message]
    caption=Long-distance Movement
    message=Order a unit to move for multiple turns.
[/hint_message]
[message]
    speaker=student
    message=I will stay by my keep.
    female_message=female^I will recruit here.
[/message]
[/event]"#;
        for hero in [Hero::Konrad, Hero::Lisar] {
            let mut second = scenario(hero, "[unit]\nid=Galdrad\ntype=Champion\nx,y=2,1\n[/unit]", strategy);
            let enemy = second.children.iter_mut().find(|(tag, team)|
                tag == "side" && team.attribute("id") == Some("Thrag")).unwrap();
            super::super::put(&mut enemy.1, "id", "Dumbo");
            prepare(&mut second, hero);
            let scenario = board(second);
            let mut game = scenario.start(7, |_| unreachable!()).unwrap();
            assert!(game.take_presentation(1).is_empty());
            game.step(Action::EndTurn).unwrap();
            assert!(game.take_presentation(1).is_empty());
            game.step(Action::EndTurn).unwrap();
            let mut session = Session::with_players(game, Players::Campaign);
            let greeting = if hero == Hero::Konrad { "Young man" } else { "Young lady" };
            assert_eq!(session.dialogue().unwrap().text, format!("{greeting}, let us discuss strategy."));
            session.dismiss_dialogue();
            let line = session.dialogue().unwrap();
            assert_eq!(line.text, "Archers can engage this grunt with little risk.");
            assert_eq!(line.focus, Some(DialogueFocus { hex: Hex::from_wml(3, 1).unwrap(), only_if_offscreen: false }));
            assert!(session.tutorial_hint().is_empty());
            session.dismiss_dialogue();
            let hint = "<b><big>Long-distance Movement</big></b>\nOrder a unit to move for multiple turns.";
            assert_eq!(session.tutorial_hint(), hint);
            assert_eq!(session.dialogue().unwrap().text, if hero == Hero::Konrad { "I will stay by my keep." } else { "I will recruit here." });
            session.dismiss_dialogue();
            assert!(session.dialogue().is_none());
            assert_eq!(session.tutorial_hint(), hint);
            assert!(session.legal_actions().contains(&Action::EndTurn));
            session.apply(Action::EndTurn).unwrap();
        }
    }

    #[test]
    fn briefing_highlights_its_subject_and_returns_to_the_next_speaker() {
        use engine_lite::{fixed_match::dialogue::DialogueFocus, hex::Hex};
        let lines = BRIEFING.replace(
            "[if]\n[have_unit]\nside=1\nx,y=recall,recall",
            "[scroll_to]\nx,y=1,1\nhighlight=yes\n[/scroll_to]\n\
             [message]\nspeaker=Galdrad\nscroll=no\nhighlight=no\nmessage=Watch the water\n[/message]\n\
             [if]\n[have_unit]\nside=1\nx,y=recall,recall",
        );
        let mut second = scenario(Hero::Konrad, "[unit]\nid=Galdrad\ntype=Champion\nx,y=2,1\n[/unit]", &lines);
        prepare(&mut second, Hero::Konrad);
        let scenario = board(second);
        let mut session = Session::with_players(scenario.start(7, |_| unreachable!()).unwrap(), Players::Campaign);
        let at = |x, only_if_offscreen| Some(DialogueFocus { hex: Hex::from_wml(x, 1).unwrap(), only_if_offscreen });
        for focus in [None, at(1, true), at(2, true), at(3, false), at(1, false), at(2, true), at(2, true)] {
            assert_eq!(session.dialogue().unwrap().focus, focus);
            session.dismiss_dialogue();
        }
        assert!(session.dialogue().is_none());
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
