//! Static gold actions on scenario and turn events.

use std::collections::BTreeSet;

use crate::{turn::Turn, wml::Node};

#[derive(Debug)]
struct GoldChange {
    sides: Vec<usize>,
    amount: i32,
}

impl GoldChange {
    fn read(cfg: &Node, side_count: usize) -> Option<Self> {
        if !cfg.children.is_empty() || cfg.attributes.keys().any(|k| !matches!(k.as_str(), "side" | "amount")) {
            return None;
        }
        let amount = cfg.attribute("amount")?.trim().parse().ok()?;
        let mut sides: Vec<_> = (0..side_count).collect();
        if let Some(selector) = cfg.attribute("side") {
            let mut ranges = Vec::new();
            for part in selector.split(',').map(str::trim) {
                let (first, last) = part.split_once('-').unwrap_or((part, part));
                ranges.push(first.trim().parse::<usize>().ok()?..=last.trim().parse::<usize>().ok()?);
            }
            sides.retain(|index| ranges.iter().any(|r| r.contains(&(index + 1))));
        }
        Some(Self { sides, amount })
    }
}

#[derive(Debug)]
struct Handler {
    names: Vec<String>,
    once: bool,
    changes: Vec<GoldChange>,
}

#[derive(Debug)]
pub(crate) struct ScenarioEvents {
    handlers: Vec<Handler>,
    pub(crate) new_round_at_start: bool,
}

impl ScenarioEvents {
    pub(crate) fn read(scenario: &Node, side_count: usize) -> Self {
        let mut handlers = Vec::new();
        let mut ids = BTreeSet::new();
        for (_, event) in scenario.children.iter().filter(|(tag, _)| tag == "event") {
            if let Some(id) = event.attribute("id").filter(|id| !id.is_empty())
                && !ids.insert(id)
            {
                continue;
            }
            // A filtered or scripted handler cannot be interpreted as unconditional gold.
            if event.attributes.keys().any(|k| k.starts_with("filter")) || event.children.is_empty()
                || !event.children.iter().all(|(tag, _)| tag == "gold")
            {
                continue;
            }
            let Some(names) = event.attribute("name") else { continue };
            let Some(changes) = event.children.iter().map(|(_, cfg)| GoldChange::read(cfg, side_count))
                .collect::<Option<Vec<_>>>() else { continue };
            let once = match event.attribute("first_time_only").unwrap_or("yes") {
                "yes" | "true" | "1" => true,
                "no" | "false" | "0" => false,
                _ => continue,
            };
            handlers.push(Handler {
                names: names.split(',').map(|name| name.trim().replace(' ', "_")).collect(),
                once,
                changes,
            });
        }
        Self {
            handlers,
            new_round_at_start: !matches!(scenario.attribute("it_is_a_new_turn"), Some("no" | "false" | "0")),
        }
    }

    pub(crate) fn fire(&mut self, name: &str, gold: &mut [i64]) {
        self.handlers.retain(|handler| {
            if !handler.names.iter().any(|n| n == name) {
                return true;
            }
            for change in &handler.changes {
                for &side in &change.sides {
                    gold[side] += i64::from(change.amount);
                }
            }
            !handler.once
        });
    }

    pub(crate) fn begin_turn(&mut self, turn: Turn, new_round: bool, gold: &mut [i64]) {
        if self.handlers.is_empty() { return; }
        let Turn { side, round } = turn;
        if new_round {
            self.fire(&format!("turn_{round}"), gold);
            self.fire("new_turn", gold);
        }
        self.fire("side_turn", gold);
        self.fire(&format!("side_{side}_turn"), gold);
        self.fire(&format!("side_turn_{round}"), gold);
        self.fire(&format!("side_{side}_turn_{round}"), gold);
    }

    pub(crate) fn refresh(&mut self, turn: Turn, gold: &mut [i64]) {
        if self.handlers.is_empty() { return; }
        let Turn { side, round } = turn;
        self.fire("turn_refresh", gold);
        self.fire(&format!("side_{side}_turn_refresh"), gold);
        self.fire(&format!("turn_{round}_refresh"), gold);
        self.fire(&format!("side_{side}_turn_{round}_refresh"), gold);
    }

    pub(crate) fn end_turn(&mut self, turn: Turn, end_round: bool, gold: &mut [i64]) {
        if self.handlers.is_empty() { return; }
        let Turn { side, round } = turn;
        self.fire("side_turn_end", gold);
        self.fire(&format!("side_{side}_turn_end"), gold);
        self.fire(&format!("side_turn_{round}_end"), gold);
        self.fire(&format!("side_{side}_turn_{round}_end"), gold);
        if end_round {
            self.fire("turn_end", gold);
            self.fire(&format!("turn_{round}_end"), gold);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{fixed_match::Action, scenario_bundle::Bundle, wml};

    fn event(name: &str, once: &str, side: &str, amount: &str) -> String {
        format!("[event]\nname={name}\nfirst_time_only={once}\n[gold]\nside={side}\namount={amount}\n[/gold]\n[/event]\n")
    }

    #[test]
    fn matches_original_campaign_gold_at_four_side_transitions() {
        let events = event("prestart", "yes", "1", "7") + &event("start", "yes", "2", "13")
            + &event("side 2 turn", "no", "2", "11") + &event("turn 2", "yes", "1", "-5");
        let config = format!(r#"[units]
[unit_type]
id=Guard
hitpoints=30
[/unit_type]
[/units]
[scenario]
turns=5
map_data="Xv,Xv,Xv,Xv
Xv,1 Gg,2 Gg,Xv
Xv,Xv,Xv,Xv"
[side]
side=1
type=Guard
gold=50
[/side]
[side]
side=2
type=Guard
gold=100
[/side]
{events}
[/scenario]"#);
        let bundle = Bundle::parse(config.as_bytes()).unwrap();
        let mut game = bundle.start(7, |_| unreachable!()).unwrap();
        assert_eq!(game.audit_snapshot().gold, [57, 113]);
        for expected in [[57,124], [54,124], [54,137], [56,137]] {
            game.step(Action::EndTurn).unwrap();
            assert_eq!(game.audit_snapshot().gold, expected);
        }
    }

    #[test]
    fn refresh_and_end_events_match_only_their_sides_and_fire_once_across_names() {
        let text = event("turn refresh,side 1 turn end", "yes", "1,3", "2")
            + &event("side_turn_end", "no", "1-2", "-3")
            + &event("turn 1 end", "yes", "2", "7")
            + &event("side 2 turn 1 refresh", "yes", "3", "5")
            + &event("side turn", "no", "$side_number", "101")
            + &event("side turn", "no", "1", "$amount")
            + &event("side turn", "no", "1", "103").replace("[gold]", "[filter_condition]\n[false]\n[/false]\n[/filter_condition]\n[gold]");
        let mut events = ScenarioEvents::read(&wml::parse(text).unwrap(), 3);
        let mut gold = [0; 3];
        for side in 1..=2 {
            let turn = Turn { round: 1, side };
            events.begin_turn(turn, side == 1, &mut gold);
            events.refresh(turn, &mut gold);
            events.end_turn(turn, side == 2, &mut gold);
        }
        assert_eq!(gold, [-4, 1, 7]);
    }
}
