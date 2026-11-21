//! Battle Training's first enemy turn and the subsequent healing exercise.
use std::error::Error;

use engine_lite::{hex::Hex, wml::Node};
use rustnoth_gui::session::Session;

use super::{Hero, opening::{children, dialogue, gender_command}, put};

pub(super) struct Exercises {
    pub end_prompt: String,
    pub village_prompt: String,
    pub villages: Vec<Hex>,
    pub after_attack: Vec<Node>,
    pub second_turn: Vec<Node>,
    pub capture: Vec<Node>,
    pub healed: Vec<Node>,
}

pub(super) fn lines(event: &Node, hero: Hero) -> Vec<Node> {
    event.children.iter().filter_map(|(tag, command)| {
        command.attribute("message")?;
        let mut line = dialogue(command, hero);
        match tag.as_str() {
            "message" => {}
            "hint_message" => put(&mut line, "speaker", "narrator"),
            _ => return None,
        }
        Some(line)
    }).collect()
}

impl Exercises {
    pub fn read(scenario: &Node, attack: &Node, hero: Hero) -> Result<Self, Box<dyn Error>> {
        let event = |name: &str| children(scenario, "event").find(|event| event.attribute("name") == Some(name));
        let turn_two = event("turn 2").ok_or("missing tutorial healing lesson")?;
        let village = children(turn_two, "label").next().ok_or("missing tutorial village markers")?;
        let coords = |key| -> Result<Vec<i32>, Box<dyn Error>> {
            Ok(village.attribute(key).ok_or("missing village coordinates")?.split(',')
                .map(|n| n.trim().parse()).collect::<Result<_, _>>()?)
        };
        let (xs, ys) = (coords("x")?, coords("y")?);
        if xs.is_empty() || xs.len() != ys.len() { return Err("unpaired tutorial village coordinates".into()); }
        let villages = xs.into_iter().zip(ys).map(|(x,y)| Hex::from_wml(x,y)).collect::<Option<_>>()
            .ok_or("invalid tutorial village location")?;
        let captured = children(turn_two, "event").find(|cfg| cfg.attribute("name") == Some("capture"))
            .ok_or("missing village capture lesson")?;
        let third = event("turn 3 refresh").ok_or("missing post-healing explanation")?;
        let healed = lines(third, hero);
        if healed.is_empty() { return Err("missing post-healing message".into()); }
        Ok(Self {
            villages,
            end_prompt: children(attack, "print").find_map(|cfg| cfg.attribute("text").filter(|s| !s.is_empty()))
                .ok_or("missing end turn instruction")?.into(),
            village_prompt: gender_command(turn_two, hero, "print").and_then(|cfg| cfg.attribute("text"))
                .ok_or("missing move to village instruction")?.into(),
            after_attack: lines(attack, hero),
            second_turn: lines(turn_two, hero),
            capture: lines(captured, hero),
            healed,
        })
    }
}

pub(super) fn student_hp(session: &Session<'_>) -> Option<i32> {
    session.observation()?.units.iter().find(|u| u.side == 1 && u.can_recruit).map(|u| u.hitpoints)
}

pub(super) fn say(lines: &[Node], session: &mut Session<'_>, before_healing: Option<i32>) {
    let Some(hp) = student_hp(session) else { return };
    for line in lines {
        let mut cfg = line.clone();
        if let Some(text) = line.attribute("message") {
            let value = text.replace("$student_hp_heal_amount", &(hp - before_healing.unwrap_or(hp)).to_string())
                .replace("$($student_hp-15)", &(hp - 15).to_string())
                .replace("$student_hp", &hp.to_string());
            put(&mut cfg, "message", &value);
        }
        session.present_message(&cfg);
    }
}
