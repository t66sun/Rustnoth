//! Galdrad's recruiting drill runs on the units actually placed by the referee.
use super::{Hero, briefing, opening::{children, node}, put};
use engine_lite::wml::Node;

const FIRST_TURN: &str = "rustnoth_tutorial_recruit_turn";

pub(super) fn prepare(scenario: &mut Node, hero: Hero) {
    let mut initial = None;
    for (tag, event) in &mut scenario.children {
        if tag != "event" { continue; }
        match event.attribute("name") {
            Some("prestart") => {
                if let Some(i) = event.children.iter().position(|(tag, _)| tag == "disallow_end_turn") {
                    initial = Some(event.children.remove(i).1);
                }
            }
            Some("recruit") => project_event(event, hero, "rustnoth_tutorial_explained_healers"),
            Some("recruit,recall") => project_event(event, hero, "rustnoth_tutorial_filled_castle"),
            Some("capture") if children(event, "on_undo").next().is_some() => project_event(event, hero, ""),
            _ => {}
        }
    }
    if let Some(denial) = initial {
        let mut start = node(&[("name", "prestart")]);
        start.children.push(("rustnoth_tutorial_disallow_end_turn".into(), denial));
        start.children.push(("set_variable".into(), node(&[("name", FIRST_TURN), ("value", "1")])));
        scenario.children.push(("event".into(), start));
        let mut end = node(&[("name", "side 1 turn 1 end")]);
        end.children.push(("clear_variable".into(), node(&[("name", FIRST_TURN)])));
        end.children.push(("rustnoth_tutorial_print".into(), node(&[("text", "")])));
        scenario.children.push(("event".into(), end));
    }
}

fn project_event(event: &mut Node, hero: Hero, seen: &str) {
    let mut condition = None;
    let mut body = Node::default();
    for (tag, cfg) in std::mem::take(&mut event.children) {
        match tag.as_str() {
            "filter" => event.children.push((tag, cfg)),
            "filter_condition" => condition = Some(cfg),
            _ => body.children.push((tag, cfg)),
        }
    }
    controls(&mut body);
    briefing::project(&mut body, hero);
    if let Some(mut guard) = condition {
        for (tag, cfg) in &mut guard.children {
            if tag == "variable" && cfg.attribute("name") == Some("turn_number") {
                put(cfg, "name", FIRST_TURN);
            }
        }
        if event.attribute("first_time_only") != Some("no") {
            // A failed filter does not consume a once-only recruit event.
            guard.children.push(("variable".into(), node(&[("name", seen), ("not_equals", "done")])));
            body.children.push(("set_variable".into(), node(&[("name", seen), ("value", "done")])));
            put(event, "first_time_only", "no");
        }
        guard.children.push(("then".into(), body));
        event.children.push(("if".into(), guard));
    } else {
        event.children.extend(body.children);
    }
}

fn controls(body: &mut Node) {
    for (tag, mut cfg) in std::mem::take(&mut body.children) {
        match tag.as_str() {
            "if" => {
                // The capture branch checks [not][have_unit] before the early-village warning.
                if let Some(i) = cfg.children.iter().position(|(tag, test)| tag == "not"
                    && test.children.len() == 1 && test.children[0].0 == "have_unit") {
                    let (_, negated) = cfg.children.remove(i);
                    cfg.children.extend(negated.children);
                    for (branch, _) in &mut cfg.children {
                        if branch == "then" { *branch = "else".into(); }
                        else if branch == "else" { *branch = "then".into(); }
                    }
                }
                for (tag, branch) in &mut cfg.children {
                    if matches!(tag.as_str(), "then" | "else") { controls(branch); }
                }
                body.children.push((tag, cfg));
            }
            "allow_end_turn" | "disallow_end_turn" => body.children.push((format!("rustnoth_tutorial_{tag}"), cfg)),
            // The turn-end print is cleared by the first-turn handler above.
            "event" | "allow_undo" | "on_undo" => {}
            _ => body.children.push((tag, cfg)),
        }
    }
}

#[cfg(test)]
mod tests;
