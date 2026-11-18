//! Static campaign actions on scenario and turn events.

use std::collections::BTreeSet;

use crate::{
    hex::Hex, map::ScenarioMap, scenario_sides::SideConfig, scenario_time::{coordinate_ranges, location_ranges},
    time_of_day::WmlRectangle, turn::Turn, wml::Node,
};

#[derive(Debug)]
enum SideAction {
    Gold(i32),
    AllowRecruit(Vec<String>),
    DisallowRecruit(Option<Vec<String>>),
}

#[derive(Debug)]
enum EventAction {
    Sides { sides: Vec<usize>, action: SideAction },
    CaptureVillage(VillageCapture),
}

#[derive(Clone, Debug)]
pub(crate) struct VillageCapture {
    pub(crate) side: i32,
    x: String,
    y: String,
}

impl VillageCapture {
    fn read(cfg: &Node) -> Option<Self> {
        if !cfg.children.is_empty()
            || cfg.attributes.keys().any(|k| !matches!(k.as_str(), "side" | "x" | "y" | "fire_event"))
            || cfg.attributes.values().any(|v| !v.translations.is_empty() || v.text.contains('$'))
        {
            return None;
        }
        let side = cfg.attribute("side").unwrap_or("0").trim().parse().ok()?;
        let x = cfg.attribute("x").unwrap_or_default().trim().to_owned();
        let y = cfg.attribute("y").unwrap_or_default().trim().to_owned();
        Some(Self { side, x, y })
    }

    pub(crate) fn for_each_hex(&self, map: &ScenarioMap, mut visit: impl FnMut(Hex)) {
        let rectangles = if self.x.is_empty() && self.y.is_empty() {
            vec![(1..=map.width(), 1..=map.height())]
        } else {
            location_ranges(&self.x, &self.y, map)
        };
        for (xs, ys) in rectangles {
            for x in xs {
                for y in ys.clone() {
                    let hex = Hex { x: x - 1, y: y - 1 };
                    if map.is_playable(hex) {
                        visit(hex);
                    }
                }
            }
        }
    }
}

pub(crate) struct EventState<'a> {
    pub(crate) gold: &'a mut [i64],
    pub(crate) sides: &'a mut [SideConfig],
    pub(crate) captures: Vec<VillageCapture>,
}

impl EventAction {
    fn read(tag: &str, cfg: &Node, side_count: usize) -> Option<Self> {
        if tag == "capture_village" {
            return VillageCapture::read(cfg).map(Self::CaptureVillage);
        }
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
        Some(Self::Sides { sides, action })
    }

    fn apply(&self, state: &mut EventState<'_>) {
        let (sides, action) = match self {
            Self::CaptureVillage(capture) => {
                state.captures.push(capture.clone());
                return;
            }
            Self::Sides { sides, action } => (sides, action),
        };
        for &index in sides {
            match action {
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
struct UnitFilter {
    locations: Vec<WmlRectangle>,
    sides: Vec<usize>,
    ids: Vec<String>,
    types: Vec<String>,
}

impl UnitFilter {
    fn read(cfg: &Node) -> Option<Self> {
        if !cfg.children.is_empty() || cfg.attributes.iter().any(|(key, value)| {
            !matches!(key.as_str(), "x" | "y" | "side" | "id" | "type")
                || !value.translations.is_empty() || value.text.contains('$')
        }) {
            return None;
        }
        let values = |key| cfg.attribute(key).unwrap_or_default().split(',')
            .map(str::trim).filter(|item| !item.is_empty());
        let x = cfg.attribute("x").unwrap_or_default();
        let y = cfg.attribute("y").unwrap_or_default();
        let locations = coordinate_ranges(x, y, i32::MAX, i32::MAX);
        Some(Self {
            locations,
            sides: values("side").map(str::parse).collect::<Result<_, _>>().ok()?,
            ids: values("id").map(str::to_owned).collect(),
            types: values("type").map(str::to_owned).collect(),
        })
    }

    fn matches(&self, unit: &ArrivingUnit<'_>) -> bool {
        let Some((x, y)) = unit.position.to_wml() else { return false };
        (self.locations.is_empty() || self.locations.iter().any(|(xs, ys)| xs.contains(&x) && ys.contains(&y)))
            && (self.sides.is_empty() || self.sides.contains(&unit.side))
            && (self.ids.is_empty() || unit.id.is_some_and(|id| self.ids.iter().any(|expected| expected == id)))
            && (self.types.is_empty() || self.types.iter().any(|id| id == unit.type_id))
    }
}

pub(crate) struct ArrivingUnit<'a> {
    pub(crate) position: Hex,
    pub(crate) side: usize,
    pub(crate) id: Option<&'a str>,
    pub(crate) type_id: &'a str,
}

#[derive(Debug)]
struct Handler {
    names: Vec<String>,
    once: bool,
    filters: Vec<UnitFilter>,
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
            let Some(filters) = event.children.iter().filter(|(tag, _)| tag == "filter")
                .map(|(_, cfg)| UnitFilter::read(cfg)).collect::<Option<Vec<_>>>() else { continue };
            let Some(changes) = event.children.iter().filter(|(tag, _)| tag != "filter")
                .map(|(tag, cfg)| EventAction::read(tag, cfg, side_count))
                .collect::<Option<Vec<_>>>() else { continue };
            let once = match event.attribute("first_time_only").unwrap_or("yes") {
                "yes" | "true" | "1" => true,
                "no" | "false" | "0" => false,
                _ => continue,
            };
            handlers.push(Handler {
                names: names.split(',').map(|name| name.trim().replace(' ', "_")).collect(),
                once,
                filters,
                changes,
            });
        }
        Self {
            handlers,
            new_round_at_start: !matches!(scenario.attribute("it_is_a_new_turn"), Some("no" | "false" | "0")),
        }
    }

    pub(crate) fn fire(&mut self, name: &str, state: &mut EventState<'_>) {
        self.dispatch(name, None, state);
    }

    pub(crate) fn fire_for_unit(&mut self, name: &str, unit: ArrivingUnit<'_>, state: &mut EventState<'_>) {
        self.dispatch(name, Some(unit), state);
    }

    fn dispatch(&mut self, name: &str, unit: Option<ArrivingUnit<'_>>, state: &mut EventState<'_>) {
        self.handlers.retain(|handler| {
            if !handler.names.iter().any(|n| n == name)
                || !handler.filters.iter().all(|f| unit.as_ref().is_some_and(|u| f.matches(u)))
            {
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
    fn capture_events_assign_and_clear_only_matching_villages() {
        let config = br#"[terrain_type]
string=Vh
gives_income=yes
[/terrain_type]
[units]
[unit_type]
id=Guard
hitpoints=30
[/unit_type]
[/units]
[scenario]
map_data="Xv,Xv,Xv,Xv,Xv,Xv,Xv
Xv,1 Gg,Vh,Vh,2 Gg,Vh,Xv
Xv,Xv,Xv,Xv,Xv,Xv,Xv"
[side]
type=Guard
village_gold=4
[/side]
[side]
type=Guard
[/side]
[side]
no_leader=yes
[/side]
[event]
name=prestart
[capture_village]
side=1
x=1-2,5
y=1,1
[/capture_village]
[capture_village]
side=2
x=3
[/capture_village]
[capture_village]
side=3
x=5
y=1
[/capture_village]
[/event]
[event]
name=start
[capture_village]
x=$where
[/capture_village]
[/event]
[event]
name=start
[capture_village]
[filter]
side=1
[/filter]
[/capture_village]
[/event]
[event]
name=side 2 turn end
[capture_village]
side=0
x=2
y=1
[/capture_village]
[capture_village]
side=9
x=3
y=1
[/capture_village]
[/event]
[/scenario]"#;
        let bundle = Bundle::parse(config).unwrap();
        let mut game = bundle.start(11, |_| unreachable!()).unwrap();
        let a = Hex { x: 1, y: 0 };
        let b = Hex { x: 2, y: 0 };
        let c = Hex { x: 4, y: 0 };
        assert_eq!(game.audit_snapshot().villages, [(a, 1), (b, 2), (c, 1)]);
        let view = game.observation_for(1).unwrap();
        assert_eq!(view.tiles.iter().find(|tile| tile.position == b).unwrap().village_owner, Some(2));
        game.step(Action::EndTurn).unwrap();
        game.step(Action::EndTurn).unwrap();
        assert_eq!(game.audit_snapshot().villages, [(c, 1)]);
        game.step(Action::EndTurn).unwrap();
        assert_eq!(game.audit_snapshot().gold[0], 106);
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
        let mut state = EventState { gold: &mut gold, sides: &mut sides, captures: Vec::new() };
        for side in 1..=2 {
            let turn = Turn { round: 1, side };
            events.begin_turn(turn, side == 1, &mut state);
            events.refresh(turn, &mut state);
            events.end_turn(turn, side == 2, &mut state);
        }
        assert_eq!(gold, [-4, 1, 7]);
    }
}
