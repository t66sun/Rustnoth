//! Static campaign actions on scenario and turn events.

use std::collections::BTreeSet;

use crate::{scenario_sides::SideConfig, turn::Turn, wml::Node};

#[derive(Debug)]
enum SideAction {
    Gold(i32),
    AllowRecruit(Vec<String>),
    DisallowRecruit(Option<Vec<String>>),
}

#[derive(Debug)]
struct EventAction {
    sides: Vec<usize>,
    action: SideAction,
}

pub(crate) struct EventState<'a> {
    pub(crate) gold: &'a mut [i64],
    pub(crate) sides: &'a mut [SideConfig],
}

impl EventAction {
    fn read(tag: &str, cfg: &Node, side_count: usize) -> Option<Self> {
        let types = |value: &str| {
            value.split(',').map(str::trim).filter(|id| !id.is_empty())
                .map(str::to_owned).collect::<Vec<_>>()
        };
        let (value_field, action) = match tag {
            "gold" => ("amount", SideAction::Gold(cfg.attribute("amount")?.trim().parse().ok()?)),
            "allow_recruit" => ("type", SideAction::AllowRecruit(types(cfg.attribute("type")?))),
            "disallow_recruit" => ("type", SideAction::DisallowRecruit(cfg.attribute("type").map(types))),
            _ => return None,
        };
        if !cfg.children.is_empty() || cfg.attributes.keys().any(|k| k != "side" && k != value_field)
            || cfg.attribute(value_field).is_some_and(|v| v.contains('$'))
        {
            return None;
        }
        let mut sides: Vec<_> = (0..side_count).collect();
        if let Some(selector) = cfg.attribute("side") {
            let mut ranges = Vec::new();
            for part in selector.split(',').map(str::trim) {
                let (first, last) = part.split_once('-').unwrap_or((part, part));
                ranges.push(first.trim().parse::<usize>().ok()?..=last.trim().parse::<usize>().ok()?);
            }
            sides.retain(|index| ranges.iter().any(|r| r.contains(&(index + 1))));
        }
        Some(Self { sides, action })
    }

    fn apply(&self, state: &mut EventState<'_>) {
        for &index in &self.sides {
            match &self.action {
                SideAction::Gold(amount) => state.gold[index] += i64::from(*amount),
                SideAction::AllowRecruit(types) => state.sides[index].recruits.extend(types.iter().cloned()),
                SideAction::DisallowRecruit(Some(types)) => {
                    for id in types { state.sides[index].recruits.remove(id); }
                }
                SideAction::DisallowRecruit(None) => state.sides[index].recruits.clear(),
            }
        }
    }
}

#[derive(Debug)]
struct Handler {
    names: Vec<String>,
    once: bool,
    changes: Vec<EventAction>,
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
            // Filtered or scripted handlers cannot be interpreted as unconditional actions.
            if event.attributes.keys().any(|k| k.starts_with("filter")) || event.children.is_empty()
            {
                continue;
            }
            let Some(names) = event.attribute("name") else { continue };
            let Some(changes) = event.children.iter().map(|(tag, cfg)| EventAction::read(tag, cfg, side_count))
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

    pub(crate) fn fire(&mut self, name: &str, state: &mut EventState<'_>) {
        self.handlers.retain(|handler| {
            if !handler.names.iter().any(|n| n == name) {
                return true;
            }
            for change in &handler.changes {
                change.apply(state);
            }
            !handler.once
        });
    }

    pub(crate) fn begin_turn(&mut self, turn: Turn, new_round: bool, state: &mut EventState<'_>) {
        if self.handlers.is_empty() { return; }
        let Turn { side, round } = turn;
        if new_round {
            self.fire(&format!("turn_{round}"), state);
            self.fire("new_turn", state);
        }
        self.fire("side_turn", state);
        self.fire(&format!("side_{side}_turn"), state);
        self.fire(&format!("side_turn_{round}"), state);
        self.fire(&format!("side_{side}_turn_{round}"), state);
    }

    pub(crate) fn refresh(&mut self, turn: Turn, state: &mut EventState<'_>) {
        if self.handlers.is_empty() { return; }
        let Turn { side, round } = turn;
        self.fire("turn_refresh", state);
        self.fire(&format!("side_{side}_turn_refresh"), state);
        self.fire(&format!("turn_{round}_refresh"), state);
        self.fire(&format!("side_{side}_turn_{round}_refresh"), state);
    }

    pub(crate) fn end_turn(&mut self, turn: Turn, end_round: bool, state: &mut EventState<'_>) {
        if self.handlers.is_empty() { return; }
        let Turn { side, round } = turn;
        self.fire("side_turn_end", state);
        self.fire(&format!("side_{side}_turn_end"), state);
        self.fire(&format!("side_turn_{round}_end"), state);
        self.fire(&format!("side_{side}_turn_{round}_end"), state);
        if end_round {
            self.fire("turn_end", state);
            self.fire(&format!("turn_{round}_end"), state);
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
    fn campaign_events_change_recruit_choices_and_placement_in_command_order() {
        let mut config = String::from("[units]\n");
        for id in ["Guard", "Archer", "Scout Cavalry"] {
            config.push_str(&format!("[unit_type]\nid={id}\nhitpoints=18\ncost=7\n[/unit_type]\n"));
        }
        config.push_str(r#"[/units]
[terrain_type]
string=Kh
recruit_from=yes
recruit_onto=yes
[/terrain_type]
[terrain_type]
string=Ch
recruit_onto=yes
[/terrain_type]
[scenario]
map_data="Xv,Xv,Xv,Xv,Xv,Xv,Xv,Xv
Xv,1 Kh,Ch,Ch,Gg,Ch,2 Kh,Xv
Xv,Xv,Xv,Xv,Xv,Xv,Xv,Xv"
[side]
type=Guard
recruit=Guard
[/side]
[side]
type=Guard
recruit=Guard
[/side]
[event]
name=prestart
[disallow_recruit]
[/disallow_recruit]
[allow_recruit]
side=1
type=Archer, Scout Cavalry
[/allow_recruit]
[disallow_recruit]
side=1
type=Archer
[/disallow_recruit]
[gold]
side=1
amount=5
[/gold]
[/event]
[event]
name=side 2 turn
[allow_recruit]
side=2
type=Archer
[/allow_recruit]
[/event]
[/scenario]"#);
        let bundle = Bundle::parse(config.as_bytes()).unwrap();
        let mut game = bundle.start(12, |_| unreachable!()).unwrap();
        let recruit = |id: &str, x| Action::Recruit {
            unit_type: id.to_owned(), to: crate::hex::Hex { x, y: 0 },
        };
        let actions = game.decision().unwrap().1;
        assert!(actions.contains(&recruit("Scout Cavalry", 1)));
        assert!(!actions.contains(&recruit("Archer", 1)));
        assert!(game.step(recruit("Archer", 1)).is_err());
        game.step(recruit("Scout Cavalry", 1)).unwrap();
        assert_eq!(game.audit_snapshot().gold[0], 98);
        game.step(Action::EndTurn).unwrap();
        let actions = game.decision().unwrap().1;
        assert!(actions.contains(&recruit("Archer", 4)));
        assert!(!actions.contains(&recruit("Guard", 4)));
        assert!(!actions.contains(&recruit("Scout Cavalry", 4)));
        game.step(recruit("Archer", 4)).unwrap();
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
        let mut sides = crate::scenario_sides::read_sides(&wml::parse("[side]\n[/side]\n".repeat(3)).unwrap());
        let mut state = EventState { gold: &mut gold, sides: &mut sides };
        for side in 1..=2 {
            let turn = Turn { round: 1, side };
            events.begin_turn(turn, side == 1, &mut state);
            events.refresh(turn, &mut state);
            events.end_turn(turn, side == 2, &mut state);
        }
        assert_eq!(gold, [-4, 1, 7]);
    }
}
