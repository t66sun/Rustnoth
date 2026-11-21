//! Authored, once-only weapon advice in the fourth-turn fighter lesson.
use std::error::Error;

use engine_lite::{fixed_match::Action, hex::Hex, wml::Node};
use rustnoth_gui::session::Session;

use super::{Hero, healing::lines, opening::children};

#[cfg(test)]
pub(super) mod tests;

struct Feedback {
    fighter: Option<String>,
    weapon: String,
    trait_id: String,
    with_trait: Vec<Node>,
    otherwise: Vec<Node>,
}

pub(super) struct Exercise {
    target: String,
    opening: Vec<Node>,
    pub prompt: String,
    pub prompting: bool,
    select_student: Option<Node>,
    feedback: Vec<Feedback>,
}

impl Exercise {
    pub fn read(scenario: &Node, hero: Hero) -> Result<Self, Box<dyn Error>> {
        let turn = children(scenario, "event").find(|e| e.attribute("name") == Some("turn 4"))
            .ok_or("missing fourth-turn weapon lesson")?;
        let target = children(turn, "filter_condition").flat_map(|c| children(c, "have_unit"))
            .find_map(|u| u.attribute("id")).ok_or("missing weapon lesson target")?.to_owned();
        let prompt = children(turn, "print").filter_map(|p| p.attribute("text")).find(|p| !p.is_empty())
            .ok_or("missing fighter attack instruction")?.to_owned();
        let student = children(turn, "event").find(|e| e.attribute("name") == Some("select")
            && children(e, "filter").any(|f| f.attribute("id") == Some("student")))
            .and_then(|e| lines(e, hero).into_iter().next());
        let mut feedback = Vec::new();
        for event in children(turn, "event").filter(|e| e.attribute("name") == Some("attack_end")) {
            let Some(weapon) = children(event, "filter_attack").find_map(|f| f.attribute("name")) else { continue };
            let condition = children(event, "if").next().ok_or("missing weapon trait condition")?;
            let trait_id = children(condition, "have_unit").find(|u| u.attribute("id") == Some("$unit.id"))
                .and_then(|u| u.attribute("trait")).ok_or("missing attacking unit trait")?;
            let branch = |tag| children(condition, tag).flat_map(|c| lines(c, hero)).collect();
            feedback.push(Feedback {
                fighter: children(event, "filter").find_map(|f| f.attribute("type")).map(str::to_owned),
                weapon: weapon.into(), trait_id: trait_id.into(),
                with_trait: branch("then"), otherwise: branch("else"),
            });
        }
        Ok(Self { target, opening: lines(turn, hero), prompt, prompting: false, select_student: student, feedback })
    }

    pub fn start(&mut self, session: &mut Session<'_>) -> bool {
        let alive = session.observation().is_some_and(|v| v.units.iter().any(|u| u.side == 2
            && session.unit_report(u.position, u.position).is_some_and(|r| r.id.as_deref() == Some(&self.target))));
        if !alive {
            self.feedback.clear();
            return false;
        }
        self.prompting = true;
        for message in &self.opening { session.present_message(message); }
        true
    }

    pub fn selected(&mut self, position: Hex, session: &mut Session<'_>) {
        if self.prompting && self.select_student.is_some()
            && session.unit_report(position, position).is_some_and(|r| r.id.as_deref() == Some("student"))
            && let Some(message) = self.select_student.take() {
            session.present_message(&message);
        }
    }

    // Resolve weapon identity before the fight. The recruit may die or advance during it.
    pub fn weapon(&self, session: &Session<'_>, action: &Action) -> Option<usize> {
        let Action::Attack(a) = action else { return None };
        let attacker = session.observation()?.units.iter().find(|u| u.position == a.from)?;
        let report = session.unit_report(a.from, a.from)?;
        let weapon = &report.weapons.get(a.weapon)?.id;
        self.feedback.iter().position(|line| &line.weapon == weapon
            && line.fighter.as_ref().is_none_or(|kind| *kind == attacker.type_id))
    }

    pub fn explain(&mut self, index: usize, from: Hex, session: &mut Session<'_>) {
        let advice = self.feedback.remove(index);
        let has_trait = session.unit_report(from, from)
            .is_some_and(|report| report.trait_ids.contains(&advice.trait_id));
        for line in if has_trait { &advice.with_trait } else { &advice.otherwise } {
            session.present_message(line);
        }
        self.prompting = false;
    }
}
