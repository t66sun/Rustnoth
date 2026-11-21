//! Third-turn instruction and the student’s return to the keep.
use std::error::Error;

use engine_lite::{hex::Hex, wml::Node};
use rustnoth_gui::session::Session;

use super::{Hero, opening::{children, gender_command, node, placed}, put};

pub(super) struct Exercise {
    pub keep: Hex,
    pub label: String,
    pub keep_prompt: String,
    pub recruit_prompt: String,
    pub instruction: Node,
    pub crowns: Node,
    fighter: String,
    count: usize,
}

impl Exercise {
    pub fn read(scenario: &Node, hero: Hero, events: &mut Vec<(String, Node)>) -> Result<Self, Box<dyn Error>> {
        let third = children(scenario, "event").find(|e| e.attribute("name") == Some("turn 3 refresh"))
            .ok_or("missing third-turn tutorial")?;
        let event = |name: &str| children(third, "event").find(|e| e.attribute("name") == Some(name));
        let arrival = event("moveto").ok_or("missing keep lesson")?;
        let filter = children(arrival, "filter").next().ok_or("missing keep filter")?;
        let keep = placed(filter).ok_or("missing tutorial keep coordinates")?;
        let label = children(third, "label").find(|cfg| placed(cfg) == Some(keep))
            .and_then(|cfg| cfg.attribute("text")).ok_or("missing keep label")?;
        let roster = children(arrival, "set_recruit").next().ok_or("missing tutorial recruits")?;
        let recruited = event("recruit").ok_or("missing recruitment lesson")?;
        let goal = children(recruited, "filter_condition").flat_map(|cfg| children(cfg, "have_unit")).next()
            .ok_or("missing tutorial recruit goal")?;
        let fighter = goal.attribute("type").ok_or("missing tutorial fighter type")?.to_owned();
        let count = goal.attribute("count").ok_or("missing tutorial recruit count")?.parse()?;
        let mut instruction = children(arrival, "hint_message").next().ok_or("missing recruiting hint")?.clone();
        let mut crowns = gender_command(recruited, hero, "hint_message").ok_or("missing leader hint")?.clone();
        for hint in [&mut instruction, &mut crowns] { put(hint, "speaker", "narrator"); }

        // The stock moveto event is installed at refresh, after healing. Preserve that
        // lifetime even if a previous lesson's move already passed through the keep.
        let mut refresh = node(&[("name", "turn 3 refresh")]);
        refresh.children.push(("set_variable".into(), node(&[("name", "tutorial_return_to_keep"), ("value", "yes")])));
        events.push(("event".into(), refresh));
        let mut moveto = node(&[("name", "moveto"), ("first_time_only", "no")]);
        let mut student = filter.clone();
        put(&mut student, "id", "student");
        moveto.children.push(("filter".into(), student));
        let mut conditional = Node::default();
        conditional.children.push(("variable".into(), node(&[("name", "tutorial_return_to_keep"), ("equals", "yes")])));
        let mut commands = Node::default();
        commands.children.push(("set_recruit".into(), roster.clone()));
        commands.children.push(("clear_variable".into(), node(&[("name", "tutorial_return_to_keep")])));
        conditional.children.push(("then".into(), commands));
        moveto.children.push(("if".into(), conditional));
        events.push(("event".into(), moveto));

        Ok(Self {
            keep, label: label.into(), fighter, count, instruction, crowns,
            keep_prompt: gender_command(third, hero, "print").and_then(|cfg| cfg.attribute("text"))
                .ok_or("missing return to keep instruction")?.into(),
            recruit_prompt: children(arrival, "print").find_map(|cfg| cfg.attribute("text").filter(|s| !s.is_empty()))
                .ok_or("missing recruitment instruction")?.into(),
        })
    }

    pub fn recruited(&self, session: &Session<'_>) -> bool {
        session.observation().is_some_and(|view| view.units.iter()
            .filter(|unit| unit.side == view.side && unit.type_id == self.fighter).count() >= self.count)
    }
}
