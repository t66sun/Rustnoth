//! Round events for the free-practice portion of the first tutorial.
use std::collections::BTreeMap;
use engine_lite::{fixed_match::MatchState, wml::Node};
use rustnoth_gui::session::Session;

use super::{Hero, opening::{children, dialogue, node}};

#[derive(Default)]
pub(super) struct Advice {
    hints: BTreeMap<i32, Node>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_lite::{fixed_match::Action, scenario_bundle::Bundle, wml, wml_emit};

    #[test]
    fn round_advice_follows_the_mentor_and_replaces_only_the_floating_hint() {
        for mentor in [true, false] {
            let mentor_unit = if mentor { "[unit]\nid=Delfador\ntype=Mage\nx,y=2,1\n[/unit]" } else { "" };
            let mut scenario = wml::parse(format!("map_data=\"Xv,Xv,Xv,Xv,Xv\nXv,1 Kh,Gg,2 Kh,Xv\nXv,Xv,Xv,Xv,Xv\"\n\
                [side]\nid=student\ntype=Mage\n{mentor_unit}\n[/side]\n[side]\nid=enemy\ntype=Mage\n[/side]")).unwrap();
            for round in 5..=10 {
                let mut event = node(&[("name", &format!("turn {round}"))]);
                if [5, 6, 9].contains(&round) {
                    let message = wml::parse("speaker=narrator\nmessage=Advice from Delfador\n\
                        [show_if]\n[have_unit]\nid=Delfador\n[/have_unit]\n[/show_if]").unwrap();
                    event.children.push(("message".into(), message));
                }
                if round >= 6 {
                    event.children.push(("hint_message".into(), node(&[("caption", "Hint"), ("message", &format!("Round {round}"))])));
                }
                scenario.children.push(("event".into(), event));
            }
            let mut events = Vec::new();
            let mut advice = Advice::project(&scenario, Hero::Konrad, &mut events);
            scenario.children.retain(|(tag, _)| tag != "event");
            scenario.children.extend(events);
            let mut doc = wml::parse("[units]\n[unit_type]\nid=Mage\nhitpoints=32\n[/unit_type]\n[/units]").unwrap();
            doc.children.push(("scenario".into(), scenario));
            let mut bytes = Vec::new();
            wml_emit::write_config(&mut bytes, &doc).unwrap();
            let bundle = Bundle::parse(&bytes).unwrap();
            let mut session = Session::new(bundle.start(8, |_| unreachable!()).unwrap());
            let mut visible = String::new();
            for round in 1..=11 {
                assert!(matches!(session.state(), MatchState::Playing(turn) if turn.round == round && turn.side == 1));
                advice.begin_turn(&mut session);
                if mentor && [5, 6, 9].contains(&round) {
                    assert_eq!(session.dialogue().unwrap().text, "Advice from Delfador");
                    assert_eq!(session.tutorial_hint(), visible, "the prior hint remains while the mentor is speaking");
                    session.dismiss_dialogue();
                }
                if (6..=10).contains(&round) { visible = format!("<b><big>Hint</big></b>\nRound {round}"); }
                assert_eq!(session.tutorial_hint(), visible);
                // A board refresh in this same round must not replay an already delivered instruction.
                if round == 8 {
                    session.set_tutorial_hint(&node(&[("remove", "yes")]));
                    visible.clear();
                }
                advice.begin_turn(&mut session);
                assert!(session.dialogue().is_none());
                assert_eq!(session.tutorial_hint(), visible);
                session.apply(Action::EndTurn).unwrap();
                advice.begin_turn(&mut session);
                assert_eq!(session.tutorial_hint(), visible);
                session.apply(Action::EndTurn).unwrap();
            }
        }
    }
}

impl Advice {
    pub(super) fn project(scenario: &Node, hero: Hero, events: &mut Vec<(String, Node)>) -> Self {
        let mut advice = Self::default();
        for authored in children(scenario, "event") {
            let Some(name) = authored.attribute("name") else { continue };
            let Some(round @ 5..=10) = name.strip_prefix("turn ").and_then(|n| n.parse().ok()) else { continue };
            let mut event = node(&[("name", name)]);
            for (tag, source) in &authored.children {
                match tag.as_str() {
                    "hint_message" => { advice.hints.insert(round, source.clone()); }
                    "message" => {
                        let mut line = dialogue(source, hero);
                        if let Some(index) = line.children.iter().position(|(tag, _)| tag == "show_if") {
                            // Evaluate Delfador's presence on this turn, after any earlier combat.
                            let (_, mut condition) = line.children.remove(index);
                            let mut then = Node::default();
                            then.children.push(("message".into(), line));
                            condition.children.push(("then".into(), then));
                            event.children.push(("if".into(), condition));
                        } else {
                            event.children.push(("message".into(), line));
                        }
                    }
                    _ => {}
                }
            }
            if !event.children.is_empty() { events.push(("event".into(), event)); }
        }
        advice
    }

    pub(super) fn begin_turn(&mut self, session: &mut Session<'_>) {
        if let MatchState::Playing(turn) = session.state() && turn.side == 1
            && let Some(hint) = self.hints.remove(&turn.round) {
            session.set_tutorial_hint(&hint);
        }
    }
}
