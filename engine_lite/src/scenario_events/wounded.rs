use crate::{board::UnitState, fixed_match::dialogue::Message, wml::Node};

#[cfg(test)]
mod tests;

fn children<'a>(node: &'a Node, tag: &'a str) -> impl Iterator<Item = &'a Node> {
    node.children.iter().filter(move |(name, _)| name == tag).map(|(_, child)| child)
}

/// Battle Training registers its repeatable warning inside the first surviving
/// island-guard attack after the turn-three briefing. The registering attack
/// itself only delivers the two lines about counterattacks and shamans.
#[derive(Debug)]
pub(crate) struct WoundedLesson {
    first_round: i32,
    guard: String,
    side: usize,
    health_divisor: i32,
    experience_to_level: i32,
    student: String,
    guard_lines: [Vec<Message>; 2],
    warnings: Vec<[Message; 2]>,
    advancement: Message,
    stage: Stage,
}

#[derive(Debug)]
enum Stage { BeforeBriefing, Guard, Warnings(usize) }

impl WoundedLesson {
    pub(super) fn read(level: &Node) -> Option<Self> {
        let events = || children(level, "event");
        let warning = events().find(|e| e.attribute("name") == Some("warn_about_units_with_low_health"))?;
        let (first_round, attack, repeat) = events().find_map(|turn| {
            let round = turn.attribute("name")?.strip_prefix("turn ")?.parse().ok()?;
            children(turn, "event").filter(|e| e.attribute("name") == Some("attack_end")).find_map(|outer| {
                let inner = children(outer, "event").find(|e| e.attribute("name") == Some("attack_end")
                    && children(e, "fire_event").any(|f| f.attribute("name") == warning.attribute("name")))?;
                Some((round, outer, inner))
            })
        })?;
        let guard = children(attack, "filter_second").next()?.attribute("id")?.into();
        let filter = children(repeat, "filter").next()?;
        let side = filter.attribute("side")?.parse().ok()?;
        let student = children(filter, "not").next()?.attribute("id")?.into();
        let branch = children(attack, "if").next()?;
        let lines = |tag| children(children(branch, tag).next()?, "message").map(Message::tutorial_speech).collect::<Option<Vec<_>>>();
        let guard_lines = [lines("else")?, lines("then")?];
        let mut messages = children(warning, "message");
        let warning_line = messages.next()?;
        let mut advance = messages.next()?.clone();
        let conditions = children(warning, "filter_condition").next()?;
        let comparison = |root: &Node, name: &str, key: &str| children(root, "variable")
            .filter(|v| v.attribute("name") == Some(name)).find_map(|v| v.attribute(key).map(|s| s.replace(' ', "")));
        let health = comparison(conditions, "unit.hitpoints", "less_than")?;
        let health_divisor = health.strip_prefix("$($unit.max_hitpoints/")?.strip_suffix(')')?.parse::<i32>().ok()?;
        if health_divisor <= 0 { return None; }
        let last: usize = comparison(conditions, "lhpu_msg_i", "less_than_equal_to")?.parse().ok()?;
        let experience = comparison(children(&advance, "show_if").next()?, "unit.experience", "greater_than")?;
        let experience_to_level = experience.strip_prefix("$($unit.max_experience-")?.strip_suffix(')')?.parse().ok()?;
        advance.children.retain(|(tag, _)| tag != "show_if");
        let advancement = Message::tutorial_speech(&advance)?;
        let vars: Vec<_> = events().filter(|e| e.attribute("name") == Some("prestart"))
            .flat_map(|e| children(e, "set_variable")).collect();
        // The prestart variables contain complete translated sentences, including
        // separate grammatical forms for the speaking unit.
        let line = |index, field| {
            let name = warning_line.attribute(field)?.strip_prefix('$')?.replace("$lhpu_msg_i", &format!("{index}"));
            let text = vars.iter().find(|v| v.attribute("name") == Some(&name))?.attribute("value")?;
            let mut cfg = warning_line.clone();
            cfg.attributes.remove("female_message");
            cfg.set_literal("message", text.strip_prefix("female^").unwrap_or(text));
            Message::tutorial_speech(&cfg)
        };
        if last >= vars.len() { return None; }
        let warnings = (0..=last).map(|i| Some([line(i, "message")?, line(i, "female_message")?])).collect::<Option<Vec<_>>>()?;
        Some(Self { first_round, guard, side, student, health_divisor, experience_to_level, guard_lines,
            warnings, advancement, stage: Stage::BeforeBriefing })
    }

    pub(super) fn begin_round(&mut self, round: i32) {
        if round == self.first_round { self.stage = Stage::Guard; }
    }

    pub(crate) fn after_attack(&mut self, unit: &UnitState, opponent: &UnitState, prior_experience: i32) -> Vec<Message> {
        if opponent.hitpoints <= 0 { return Vec::new(); }
        let is_student = unit.id.as_deref() == Some(&self.student);
        let index = match self.stage {
            Stage::Guard if opponent.id.as_deref() == Some(&self.guard) => {
                self.stage = Stage::Warnings(0);
                return self.guard_lines[usize::from(is_student)].clone();
            }
            Stage::Warnings(next) => next,
            _ => return Vec::new(),
        };
        if is_student || unit.side != self.side || unit.hitpoints <= 0 || unit.hitpoints >= unit.max_hitpoints() / self.health_divisor {
            return Vec::new();
        }
        let Some(variants) = self.warnings.get(index) else { return Vec::new(); };
        self.stage = Stage::Warnings(index + 1);
        let mut result = vec![variants[usize::from(unit.female)].clone()];
        if prior_experience > unit.max_experience() - self.experience_to_level { result.push(self.advancement.clone()); }
        result
    }
}
