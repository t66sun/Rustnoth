//! Keep the installed tutorial's localizable [disallow_end_turn] commands.
use std::collections::BTreeMap;

use engine_lite::{fixed_match::{Action, MatchState}, wml::Node};
use rustnoth_gui::session::Session;

use super::{Lesson, Step, opening::children};

pub(super) struct Reasons {
    movement: Option<Node>,
    first_attack: Option<Node>,
    later_turns: BTreeMap<i32, Node>,
}

impl Reasons {
    pub(super) fn read(scenario: &Node, meet: &Node) -> Self {
        let restriction = |event: &Node| children(event, "disallow_end_turn").next().cloned();
        let mut movement = None;
        let mut later_turns = BTreeMap::new();
        for event in children(scenario, "event") {
            let Some(command) = restriction(event) else { continue };
            let name = event.attribute("name").unwrap_or_default();
            if name == "prestart" {
                movement = Some(command);
            } else if let Some(round) = name.strip_prefix("turn ") {
                if let Ok(round) = round.trim_end_matches(" refresh").parse() {
                    later_turns.insert(round, command);
                }
            }
        }
        Self { movement, first_attack: restriction(meet), later_turns }
    }
}

impl Lesson {
    pub(crate) fn end_turn_denial(&self, session: &Session<'_>) -> Option<&Node> {
        let MatchState::Playing(turn) = session.state() else { return None };
        if turn.side != 1 || self.finished() || session.legal_actions().contains(&Action::EndTurn) {
            return None;
        }
        match self.step {
            Step::Approach => self.end_turn_reasons.movement.as_ref(),
            Step::Attack => self.end_turn_reasons.first_attack.as_ref(),
            _ => self.end_turn_reasons.later_turns.get(&turn.round),
        }
    }
}
