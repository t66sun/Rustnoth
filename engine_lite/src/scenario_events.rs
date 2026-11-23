//! Static campaign actions on scenario and turn events.

use std::collections::{BTreeMap, BTreeSet};

mod endlevel;
pub(crate) mod kill;
mod items;
mod have_unit;
mod wounded;
mod objectives;
use have_unit::HaveUnit;
pub(crate) mod variables;
mod time_areas;
pub(crate) use time_areas::TimeAreaChange;
mod personal_recruits;
pub(crate) use personal_recruits::PersonalRecruits;
use items::{ItemChange, MapItems};
use variables::{Comparison, ScalarChange, Scalars};

use crate::{
    hex::Hex, map::ScenarioMap, scenario_sides::{SideConfig, SideController}, scenario_time::{coordinate_ranges, location_ranges},
    time_of_day::WmlRectangle, turn::Turn, wml::Node,
};

#[derive(Clone, Debug)]
pub(crate) enum SideAction {
    Gold(i32),
    Modify {
        controller: Option<SideController>,
        defeat_condition: Option<crate::scenario_sides::DefeatCondition>,
        gold: Option<i32>,
        income: Option<i32>,
        village_gold: Option<i32>,
        village_support: Option<i32>,
    },
    AllowRecruit(Vec<String>),
    SetRecruit(Vec<String>),
    DisallowRecruit(Option<Vec<String>>),
}

#[derive(Clone, Debug)]
enum EventAction {
    Sides { sides: Vec<usize>, action: SideAction },
    Board(BoardChange),
    Item(ItemChange),
    Variable(ScalarChange),
    If { tests: Vec<Comparison>, board_tests: Vec<BoardTest>, on_match: Vec<EventAction>, otherwise: Vec<EventAction> },
}

#[derive(Clone, Copy)]
struct EventBoard<'a, 'rules> {
    units: &'a crate::board::Battlefield<'rules>,
    villages: &'a BTreeMap<Hex, usize>,
}

#[derive(Clone, Debug)]
enum BoardTest {
    Unit(HaveUnit, bool),
    LocationNotOwned { hex: Hex, side: usize },
}

impl BoardTest {
    // A single map coordinate with an excluded owner, as used by Battle Training II.
    fn location(cfg: &Node) -> Option<Self> {
        if cfg.attributes.len() != 2 { return None; }
        let hex = Hex::from_wml(cfg.attribute("x")?.parse().ok()?, cfg.attribute("y")?.parse().ok()?)?;
        let [(tag, owner)] = cfg.children.as_slice() else { return None };
        if tag != "not" || !owner.children.is_empty() || owner.attributes.len() != 1 { return None; }
        let side = owner.attribute("owner_side")?.parse().ok()?;
        (side > 0).then_some(Self::LocationNotOwned { hex, side })
    }

    fn matches(&self, board: EventBoard<'_, '_>, sides: usize) -> bool {
        match self {
            Self::Unit(test, expected) => test.matches(board.units, sides) == *expected,
            Self::LocationNotOwned { hex, side } =>
                board.units.map().is_playable(*hex) && board.villages.get(hex) != Some(side),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct PendingAction(Box<EventAction>);

#[derive(Clone, Debug)]
pub(crate) enum BoardChange {
    SetObjectives { sides: Vec<usize>, definition: objectives::Definition, notify: bool },
    ShowObjectives(Vec<usize>),
    HealUnit(crate::board::HealUnit),
    SetUnitMoves { filter: UnitFilter, moves: i32 },
    Dialogue(crate::fixed_match::dialogue::Message),
    TutorialHint(Node),
    TutorialPrint(Node),
    TutorialUndoPrint(crate::wml::Value),
    TutorialEndTurn(Option<Node>),
    CaptureVillage(VillageCapture),
    TimeArea(TimeAreaChange),
    PersonalRecruits(PersonalRecruits),
    Kill(kill::KillAction, Option<Hex>),
    Resume(PendingAction, Option<Hex>),
    TurnLimit(TurnLimitChange),
    EndLevel { victory: bool, report: bool, linger: bool, replay_save: bool, next_scenario: Option<String>, gold: Vec<crate::fixed_match::carryover::GoldOverride> },
    PlaceUnit(Box<crate::starting_units::AuthoredStartingUnit>),
    MoveUnit { id: String, destination: Hex },
    Schedule { entries: Vec<crate::time_of_day::TimeOfDay>, current_time: i32 },
    Terrain { locations: Vec<WmlRectangle>, code: String, layer: crate::terrain_aliases::TerrainLayer, fallback: bool },
}

#[derive(Clone, Debug)]
pub(crate) enum TurnLimitChange {
    Set(i32),
    Add(i32),
    Percent(i32),
}

impl TurnLimitChange {
    fn read(cfg: &Node) -> Option<Self> {
        if !cfg.children.is_empty() || cfg.attributes.iter().any(|(key, value)| {
            !matches!(key.as_str(), "value" | "add") || !value.translations.is_empty() || value.text.contains('$')
        }) { return None; }
        match cfg.attribute("add").map(str::trim).filter(|s| !s.is_empty()) {
            Some(add) => match add.strip_suffix('%') {
                Some(percent) => Some(Self::Percent(percent.parse().ok()?)),
                None => Some(Self::Add(add.parse().ok()?)),
            },
            None => Some(Self::Set(cfg.attribute("value")?.trim().parse().ok()?)),
        }
    }

    pub(crate) fn apply(&self, limit: Option<i32>) -> Option<i32> {
        let old = limit.unwrap_or(-1);
        let new = match *self {
            Self::Set(value) => value,
            Self::Add(amount) => old.saturating_add(amount),
            Self::Percent(percent) => crate::trait_effects::increase_percent(old, percent, -1),
        };
        (new >= 0).then_some(new)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct VillageCapture {
    pub(crate) side: i32,
    x: String,
    y: String,
    start_radius: Option<(usize, u64)>,
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
        Some(Self { side, x, y, start_radius: None })
    }

    /// The stock STARTING_VILLAGES macro scopes its saved coordinates to one event.
    fn from_starting_location(children: &[&(String, Node)]) -> Option<Self> {
        let [(store_tag, store), (capture_tag, capture), (clear_tag, clear)] = children else { return None };
        if store_tag != "store_starting_location" || capture_tag != "capture_village" || clear_tag != "clear_variable"
            || !store.children.is_empty() || !clear.children.is_empty()
            || store.attributes.keys().any(|k| k != "variable" && k != "side")
            || clear.attributes.len() != 1 || capture.attribute("terrain") != Some("*^V*")
            || capture.attributes.keys().any(|k| k != "side" && k != "terrain")
        { return None; }
        let variable = store.attribute("variable")?;
        let [(tag, area)] = capture.children.as_slice() else { return None };
        if variable.is_empty() || clear.attribute("name")? != variable || tag != "and"
            || !area.children.is_empty() || area.attributes.len() != 3
            || area.attribute("x")? != ["$", variable, ".x"].concat()
            || area.attribute("y")? != ["$", variable, ".y"].concat()
        { return None; }
        let center = store.attribute("side")?.trim().parse().ok()?;
        Some(Self {
            side: capture.attribute("side")?.trim().parse().ok()?, x: String::new(), y: String::new(),
            start_radius: Some((center, area.attribute("radius")?.trim().parse().ok()?)),
        })
    }

    pub(crate) fn for_each_hex(&self, map: &ScenarioMap, mut visit: impl FnMut(Hex)) {
        let center = match self.start_radius {
            Some((side, radius)) => match map.special_location(&side.to_string()) {
                Some(hex) => Some((hex, radius)),
                None => return,
            },
            None => None,
        };
        let rectangles = if self.x.is_empty() && self.y.is_empty() {
            vec![(1..=map.width(), 1..=map.height())]
        } else {
            location_ranges(&self.x, &self.y, map)
        };
        for (xs, ys) in rectangles {
            for x in xs {
                for y in ys.clone() {
                    let hex = Hex { x: x - 1, y: y - 1 };
                    if let Some((origin, radius)) = center {
                        if origin.distance(hex) > radius || !map.terrain_at(hex)
                            .and_then(|code| code.split_once('^')).is_some_and(|(_, overlay)| overlay.starts_with('V'))
                        { continue; }
                    }
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
    pub(crate) board_changes: Vec<BoardChange>,
}

impl EventAction {
    fn read(tag: &str, cfg: &Node, side_count: usize) -> Option<Self> {
        if tag == "modify_unit" {
            let [(tag, filter)] = cfg.children.as_slice() else { return None };
            if tag != "filter" || cfg.attributes.len() != 1 { return None; }
            if ["x", "y"].iter().any(|key| filter.attribute(key) == Some("recall")) { return None; }
            let moves: i32 = cfg.attribute("moves")?.trim().parse().ok()?;
            return Some(Self::Board(BoardChange::SetUnitMoves { filter: UnitFilter::read(filter)?, moves }));
        }
        if tag == "heal_unit" {
            return crate::board::HealUnit::read(cfg).map(|heal| Self::Board(BoardChange::HealUnit(heal)));
        }
        if tag == "message" {
            return crate::fixed_match::dialogue::Message::read(cfg).map(|message| Self::Board(BoardChange::Dialogue(message)));
        }
        if tag == "kill" { return kill::KillAction::read(cfg).map(|kill| Self::Board(BoardChange::Kill(kill, None))); }
        if tag == "modify_turns" {
            return TurnLimitChange::read(cfg).map(|change| Self::Board(BoardChange::TurnLimit(change)));
        }
        if matches!(tag, "item" | "remove_item") {
            return ItemChange::read(tag, cfg).map(Self::Item);
        }
        if tag == "unit" {
            return crate::starting_units::read_event_unit(cfg, side_count)
                .map(|unit| Self::Board(BoardChange::PlaceUnit(Box::new(unit))));
        }
        if matches!(tag, "time_area" | "remove_time_area") {
            return TimeAreaChange::read(tag, cfg).map(|area| Self::Board(BoardChange::TimeArea(area)));
        }
        if matches!(tag, "allow_extra_recruit" | "disallow_extra_recruit" | "set_extra_recruit") {
            return PersonalRecruits::read(tag, cfg).map(|change| Self::Board(BoardChange::PersonalRecruits(change)));
        }
        if tag == "replace_schedule" {
            if cfg.attributes.keys().any(|k| k != "current_time") || cfg.children.iter().any(|(tag, time)| {
                tag != "time" || ["id", "lawful_bonus"].iter().any(|key| time.attribute(key).is_some_and(|v| v.contains('$')))
            }) { return None; }
            let entries = crate::scenario_time::time_entries(cfg);
            let current_time = cfg.attribute("current_time").unwrap_or("0").trim().parse::<i32>().ok()?;
            if !(0..entries.len()).contains(&usize::try_from(current_time).ok()?) { return None; }
            return Some(Self::Board(BoardChange::Schedule { entries, current_time }));
        }
        if tag == "capture_village" {
            return VillageCapture::read(cfg).map(|v| Self::Board(BoardChange::CaptureVillage(v)));
        }
        if tag == "terrain" {
            if !cfg.children.is_empty() || cfg.attributes.keys().any(|k| !matches!(k.as_str(), "x" | "y" | "terrain" | "layer" | "replace_if_failed")) {
                return None;
            }
            if cfg.attributes.values().any(|v| !v.translations.is_empty() || v.text.contains('$')) {
                return None;
            }
            let x = cfg.attribute("x").unwrap_or_default();
            let y = cfg.attribute("y").unwrap_or_default();
            let mut locations = coordinate_ranges(x, y, i32::MAX, i32::MAX);
            if locations.is_empty() {
                locations.push((1..=i32::MAX, 1..=i32::MAX));
            }
            let code = cfg.attribute("terrain")?.trim();
            if code.is_empty() || code.contains(['$', '*', ',', ' ']) { return None; }
            use crate::terrain_aliases::TerrainLayer;
            let layer = match cfg.attribute("layer").unwrap_or("both") {
                "both" => TerrainLayer::Both,
                "base" => TerrainLayer::Base,
                "overlay" => TerrainLayer::Overlay,
                _ => return None,
            };
            let fallback = matches!(cfg.attribute("replace_if_failed"), Some("yes" | "true" | "1"));
            let code = code.trim_end_matches('^').to_owned();
            return Some(Self::Board(BoardChange::Terrain { locations, code, layer, fallback }));
        }
        if tag == "objectives" || tag == "show_objectives" {
            if tag == "show_objectives" && (cfg.attributes.keys().any(|k| k != "side") || !cfg.children.is_empty()) {
                return None;
            }
            // Unknown side filters must not publish private objectives to other sides.
            if cfg.attributes.keys().any(|key| !matches!(key.as_str(), "side" | "silent" | "summary" |
                "victory_string" | "defeat_string" | "notes_string" | "note" | "gold_carryover_string" |
                "bullet" | "delayed_variable_substitution"))
                || cfg.children.iter().any(|(tag, _)| !matches!(tag.as_str(), "objective" | "note" | "gold_carryover"))
            { return None; }
            let mut sides = Self::side_indices(cfg, side_count)?;
            // objectives.lua uses the global list when the side filter matches no side (including side=0).
            if sides.is_empty() { sides.extend(0..side_count); }
            return Some(if tag == "show_objectives" {
                Self::Board(BoardChange::ShowObjectives(sides))
            } else {
                Self::Board(BoardChange::SetObjectives {
                    sides, definition: objectives::Definition::read(cfg),
                    notify: !crate::scenario_sides::bool_attribute(cfg, "silent", false),
                })
            });
        }
        if tag == "modify_side" {
            if !cfg.children.is_empty() || cfg.attributes.iter().any(|(key, value)| {
                // Campaigns often reveal the side in the native status table as they activate its AI.
                !matches!(key.as_str(), "side" | "controller" | "hidden" | "defeat_condition" | "gold" | "income" | "village_gold" | "village_support")
                    || !value.translations.is_empty() || value.text.contains('$')
            }) { return None; }
            let number = |key| cfg.attribute(key).map(|value| value.trim().parse::<i32>()).transpose();
            let action = SideAction::Modify {
                controller: cfg.attribute("controller").and_then(SideController::from_wml),
                defeat_condition: match cfg.attribute("defeat_condition") {
                    Some(value) => Some(crate::scenario_sides::DefeatCondition::from_wml(value)?),
                    None => None,
                },
                gold: number("gold").ok()?,
                income: number("income").ok()?,
                village_gold: number("village_gold").ok()?,
                village_support: number("village_support").ok()?,
            };
            return Self::side_action(cfg, side_count, action);
        }
        let types = |value: &str| {
            value.split(',').map(str::trim).filter(|id| !id.is_empty())
                .map(str::to_owned).collect::<Vec<_>>()
        };
        let (value_field, action) = match tag {
            "gold" => ("amount", SideAction::Gold(cfg.attribute("amount")?.trim().parse().ok()?)),
            "allow_recruit" => ("type", SideAction::AllowRecruit(types(cfg.attribute("type")?))),
            "set_recruit" => ("recruit", SideAction::SetRecruit(types(cfg.attribute("recruit")?))),
            "disallow_recruit" => ("type", SideAction::DisallowRecruit(cfg.attribute("type").map(types))),
            _ => return None,
        };
        if !cfg.children.is_empty() || cfg.attributes.keys().any(|k| k != "side" && k != value_field)
            || cfg.attribute(value_field).is_some_and(|v| v.contains('$'))
        {
            return None;
        }
        Self::side_action(cfg, side_count, action)
    }

    fn side_action(cfg: &Node, side_count: usize, action: SideAction) -> Option<Self> {
        Some(Self::Sides { sides: Self::side_indices(cfg, side_count)?, action })
    }

    fn side_indices(cfg: &Node, side_count: usize) -> Option<Vec<usize>> {
        let mut sides: Vec<_> = (0..side_count).collect();
        if let Some(selector) = cfg.attribute("side") {
            let mut ranges = Vec::new();
            for part in selector.split(',').map(str::trim) {
                let (first, last) = part.split_once('-').unwrap_or((part, part));
                ranges.push(first.trim().parse::<usize>().ok()?..=last.trim().parse::<usize>().ok()?);
            }
            sides.retain(|index| ranges.iter().any(|r| r.contains(&(index + 1))));
        }
        Some(sides)
    }

    fn apply(&self, state: &mut EventState<'_>, items: &mut MapItems, variables: &mut Scalars,
        primary: Option<Hex>, board: Option<EventBoard<'_, '_>>, active_side: usize) {
        if !matches!(self, Self::Board(_)) && (!state.board_changes.is_empty()
            || matches!(self, Self::If { board_tests, .. } if !board_tests.is_empty() && board.is_none())) {
            state.board_changes.push(BoardChange::Resume(PendingAction(Box::new(self.clone())), primary));
            return;
        }
        let (sides, action) = match self {
            Self::Variable(change) => {
                change.apply(variables);
                return;
            }
            Self::If { tests, board_tests, on_match, otherwise } => {
                let matched = tests.iter().all(|test| test.matches(variables))
                    && board_tests.iter().all(|test| test.matches(board.expect("board conditions resume on the referee"), state.sides.len()));
                let branch = if matched { on_match } else { otherwise };
                for change in branch { change.apply(state, items, variables, primary, board, active_side); }
                return;
            }
            Self::Item(change) => {
                items.apply(change);
                return;
            }
            Self::Board(BoardChange::Kill(kill, _)) => {
                state.board_changes.push(BoardChange::Kill(kill.clone(), primary));
                return;
            }
            Self::Board(change) => {
                state.board_changes.push(change.clone());
                return;
            }
            Self::Sides { sides, action } => (sides, action),
        };
        action.apply(sides, state, active_side);
    }
}

impl SideAction {
    fn apply(&self, sides: &[usize], state: &mut EventState<'_>, active_side: usize) {
        let teams = &mut state.sides;
        let funds = &mut state.gold;
        for &index in sides {
            match self {
                SideAction::Gold(amount) => funds[index] += i64::from(*amount),
                SideAction::Modify { controller, defeat_condition, gold, income, village_gold, village_support } => {
                    if let Some(rule) = defeat_condition { teams[index].defeat_condition = *rule; }
                    let side = &mut teams[index];
                    // team::change_controller_by_wml keeps the currently playing side active.
                    if let Some(controller) = controller
                        && (*controller != SideController::Empty || side.side != active_side) {
                        side.controller = *controller;
                    }
                    if let Some(value) = gold { funds[index] = i64::from(*value); }
                    if let Some(value) = income { side.income_bonus = *value; }
                    if let Some(value) = village_gold { side.village_gold = *value; }
                    if let Some(value) = village_support { side.village_support = *value; }
                }
                SideAction::AllowRecruit(types) => teams[index].recruits.extend(types.iter().cloned()),
                SideAction::SetRecruit(types) => teams[index].recruits = types.iter().cloned().collect(),
                SideAction::DisallowRecruit(Some(types)) => {
                    for id in types { teams[index].recruits.remove(id); }
                }
                SideAction::DisallowRecruit(None) => teams[index].recruits.clear(),
            }
        }
    }
}

fn read_actions<'a>(
    commands: impl IntoIterator<Item = &'a (String, Node)>,
    scenario: &Node,
    side_count: usize,
) -> Option<Vec<EventAction>> {
    let mut actions = Vec::new();
    for (tag, cfg) in commands {
        let action = match tag.as_str() {
            "move_unit" if cfg.children.is_empty() && cfg.attributes.len() == 3 => {
                let id = cfg.attribute("id")?;
                if id.is_empty() || id.contains([',', '$', '*', '?']) { return None; }
                let destination = Hex::from_wml(cfg.attribute("to_x")?.parse().ok()?, cfg.attribute("to_y")?.parse().ok()?)?;
                EventAction::Board(BoardChange::MoveUnit { id: id.to_owned(), destination })
            }
            "rustnoth_tutorial_print" => EventAction::Board(BoardChange::TutorialPrint(cfg.clone())),
            "rustnoth_tutorial_undo" => EventAction::Board(BoardChange::TutorialUndoPrint(cfg.attributes.get("text")?.clone())),
            "rustnoth_tutorial_disallow_end_turn" => EventAction::Board(BoardChange::TutorialEndTurn(Some(cfg.clone()))),
            "rustnoth_tutorial_allow_end_turn" => EventAction::Board(BoardChange::TutorialEndTurn(None)),
            "set_variable" | "clear_variable" => EventAction::Variable(ScalarChange::read(tag, cfg)?),
            "rustnoth_tutorial_hint" if cfg.children.is_empty() && cfg.attributes.iter().all(|(key, value)|
                    matches!(key.as_str(), "caption" | "message" | "remove") && !value.text.contains('$')) =>
                EventAction::Board(BoardChange::TutorialHint(cfg.clone())),
            "if" => {
                if !cfg.attributes.is_empty() { return None; }
                let mut tests = Vec::new();
                let mut board_tests = Vec::new();
                let mut on_match = Vec::new();
                let mut otherwise = Vec::new();
                let mut has_branch = false;
                for (tag, child) in &cfg.children {
                    match tag.as_str() {
                        "variable" => tests.push(Comparison::read(child)?),
                        "have_unit" => board_tests.push(BoardTest::Unit(HaveUnit::read(child)?, true)),
                        "have_location" => board_tests.push(BoardTest::location(child)?),
                        "not" if child.attributes.is_empty() => {
                            let [(predicate, unit)] = child.children.as_slice() else { return None };
                            if predicate != "have_unit" { return None; }
                            board_tests.push(BoardTest::Unit(HaveUnit::read(unit)?, false));
                        }
                        "then" | "else" if child.attributes.is_empty() => {
                            has_branch = true;
                            let branch = if tag == "then" { &mut on_match } else { &mut otherwise };
                            branch.extend(read_actions(&child.children, scenario, side_count)?);
                        }
                        _ => return None,
                    }
                }
                if (tests.is_empty() && board_tests.is_empty()) || !has_branch { return None; }
                EventAction::If { tests, board_tests, on_match, otherwise }
            }
            "endlevel" => EventAction::Board(BoardChange::EndLevel {
                victory: endlevel::player_victory(cfg, scenario)?,
                report: crate::scenario_sides::bool_attribute(cfg, "carryover_report", true),
                linger: crate::scenario_sides::bool_attribute(cfg, "linger_mode", true),
                replay_save: crate::scenario_sides::bool_attribute(cfg, "replay_save", true),
                next_scenario: cfg.attribute("next_scenario").map(str::to_owned),
                gold: crate::fixed_match::carryover::GoldOverride::endlevel(cfg, side_count),
            }),
            _ => match EventAction::read(tag, cfg, side_count) {
                Some(action) => action,
                None if tag == "objectives" || presentation_only(tag, cfg) => continue,
                None => return None,
            },
        };
        actions.push(action);
    }
    Some(actions)
}

/// Ignore display-only commands without descending through conditions or player choices.
fn presentation_only(tag: &str, cfg: &Node) -> bool {
    match tag {
        "message" | "move_unit_fake" | "scroll_to" | "scroll_to_unit" | "item" | "redraw" | "delay" | "remove_item" => {
            cfg.children.is_empty()
        }
        "repeat" => cfg.children.iter().all(|(tag, body)| {
            tag == "do" && body.children.iter().all(|(tag, child)| presentation_only(tag, child))
        }),
        _ => false,
    }
}

#[derive(Clone, Debug)]
pub(crate) struct UnitFilter {
    locations: Vec<WmlRectangle>,
    excluded: Vec<Self>,
    sides: Vec<usize>,
    ids: Vec<String>,
    types: Vec<String>,
}

impl UnitFilter {
    pub(crate) fn read(cfg: &Node) -> Option<Self> {
        if cfg.attributes.iter().any(|(key, value)| {
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
        let excluded = cfg.children.iter().map(|(tag, child)| {
            if tag == "not" { Self::read(child) } else { None }
        }).collect::<Option<_>>()?;
        Some(Self {
            locations,
            excluded,
            sides: values("side").map(str::parse).collect::<Result<_, _>>().ok()?,
            ids: values("id").map(str::to_owned).collect(),
            types: values("type").map(str::to_owned).collect(),
        })
    }

    pub(crate) fn matches(&self, unit: &ArrivingUnit<'_>) -> bool {
        self.matches_fields(Some(unit.position), unit.side, unit.id.as_deref(), unit.type_id)
    }

    pub(crate) fn matches_fields(&self, position: Option<Hex>, side: usize, id: Option<&str>, type_id: &str) -> bool {
        (self.locations.is_empty() || position.and_then(Hex::to_wml).is_some_and(|(x, y)|
            self.locations.iter().any(|(xs, ys)| xs.contains(&x) && ys.contains(&y))))
            && (self.sides.is_empty() || self.sides.contains(&side))
            && (self.ids.is_empty() || id.is_some_and(|id| self.ids.iter().any(|expected| expected == id)))
            && (self.types.is_empty() || self.types.iter().any(|id| id == type_id))
            && !self.excluded.iter().any(|filter| filter.matches_fields(position, side, id, type_id))
    }
}

#[derive(Debug)]
enum EventFilter {
    Unit(UnitFilter),
    SecondUnit(UnitFilter),
    Side { numbers: Vec<usize>, team_name: Option<String> },
}

impl EventFilter {
    fn read(tag: &str, cfg: &Node, side_count: usize) -> Option<Self> {
        if tag == "filter_second" { return UnitFilter::read(cfg).map(Self::SecondUnit); }
        if tag == "filter" {
            return UnitFilter::read(cfg).map(Self::Unit);
        }
        if !cfg.children.is_empty() || cfg.attributes.iter().any(|(key, value)| {
            !matches!(key.as_str(), "side" | "side_in" | "team_name")
                || !value.translations.is_empty() || value.text.contains('$')
        }) {
            return None;
        }
        let mut numbers: Vec<_> = (1..=side_count).collect();
        for selector in ["side", "side_in"].into_iter().filter_map(|key| cfg.attribute(key)) {
            let mut ranges = Vec::new();
            for part in selector.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                let (start, end) = part.split_once('-').unwrap_or((part, part));
                let start = start.trim().parse::<usize>().ok()?;
                let end = match end.trim() {
                    "infinity" => usize::MAX,
                    value => value.parse::<usize>().ok()?.max(start),
                };
                ranges.push(start..=end);
            }
            numbers.retain(|side| ranges.iter().any(|range| range.contains(side)));
        }
        Some(Self::Side {
            numbers,
            team_name: cfg.attribute("team_name").filter(|s| !s.is_empty()).map(str::to_owned),
        })
    }

    fn matches(&self, side: &SideConfig, unit: Option<&ArrivingUnit<'_>>, second: Option<&ArrivingUnit<'_>>) -> bool {
        match self {
            Self::Unit(filter) => unit.is_some_and(|unit| filter.matches(unit)),
            Self::SecondUnit(filter) => second.is_some_and(|unit| filter.matches(unit)),
            Self::Side { numbers, team_name } => numbers.contains(&side.side)
                && team_name.as_ref().is_none_or(|name| side.team_names.contains(name)),
        }
    }
}

pub(crate) struct ArrivingUnit<'a> {
    pub(crate) position: Hex,
    pub(crate) side: usize,
    pub(crate) id: Option<String>,
    pub(crate) type_id: &'a str,
}

#[derive(Debug)]
struct Handler {
    names: Vec<String>,
    once: bool,
    filters: Vec<EventFilter>,
    changes: Vec<EventAction>,
}

#[derive(Debug)]
pub(crate) struct ScenarioEvents {
    pub(crate) wounded: Option<wounded::WoundedLesson>,
    objective_definitions: BTreeMap<usize, objectives::Definition>,
    handlers: Vec<Handler>,
    pub(crate) active_side: usize,
    variables: Scalars,
    pub(crate) items: MapItems,
    pub(crate) new_round_at_start: bool,
}

impl ScenarioEvents {
    pub(crate) fn read(scenario: &Node, side_count: usize) -> Self {
        let mut result = Self::from_events(scenario, side_count, scenario.children.iter()
            .filter(|(tag, _)| tag == "event").map(|(_, event)| event));
        result.wounded = wounded::WoundedLesson::read(scenario);
        result
    }

    pub(crate) fn accepts_event(scenario: &Node, side_count: usize, event: &Node) -> bool {
        !Self::from_events(scenario, side_count, [event]).handlers.is_empty()
    }

    fn from_events<'a>(scenario: &Node, side_count: usize, events: impl IntoIterator<Item=&'a Node>) -> Self {
        let mut handlers = Vec::new();
        let mut ids = BTreeSet::new();
        for event in events {
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
            let (conditions, actions): (Vec<_>, Vec<_>) = event.children.iter()
                .partition(|(tag, _)| matches!(tag.as_str(), "filter" | "filter_side" | "filter_second"));
            let Some(filters) = conditions.into_iter().map(|(tag, cfg)| EventFilter::read(tag, cfg, side_count))
                .collect::<Option<Vec<_>>>() else { continue };
            let changes = if let Some(capture) = VillageCapture::from_starting_location(&actions) {
                let mut changes = vec![EventAction::Board(BoardChange::CaptureVillage(capture))];
                if let Some((tag, clear)) = actions.last()
                    && let Some(change) = ScalarChange::read(tag, clear)
                {
                    changes.push(EventAction::Variable(change));
                }
                changes
            } else {
                let Some(changes) = read_actions(actions, scenario, side_count) else { continue };
                changes
            };
            let once = match event.attribute("first_time_only").unwrap_or("yes") {
                "yes" | "true" | "1" => true,
                "no" | "false" | "0" => false,
                _ => continue,
            };
            let priority = event.attribute("priority").and_then(|s| s.trim().parse::<f64>().ok())
                .filter(|value| value.is_finite()).unwrap_or(0.0);
            handlers.push((priority, Handler {
                names: names.split(',').map(|name| name.trim().replace(' ', "_")).collect(),
                once,
                filters,
                changes,
            }));
        }
        // Equal priorities retain registration order.
        handlers.sort_by(|(a, _), (b, _)| b.partial_cmp(a).unwrap());
        Self {
            handlers: handlers.into_iter().map(|(_, handler)| handler).collect(),
            wounded: None,
            objective_definitions: BTreeMap::new(),
            active_side: 1,
            variables: variables::initial(scenario),
            items: MapItems::default(),
            new_round_at_start: !matches!(scenario.attribute("it_is_a_new_turn"), Some("no" | "false" | "0")),
        }
    }

    pub(crate) fn resume(&mut self, action: &PendingAction, primary: Option<Hex>, units: &crate::board::Battlefield<'_>,
        villages: &BTreeMap<Hex, usize>, state: &mut EventState<'_>) {
        let board = EventBoard { units, villages };
        action.0.apply(state, &mut self.items, &mut self.variables, primary, Some(board), self.active_side);
    }

    pub(crate) fn has_handler(&self, name: &str) -> bool {
        self.handlers.iter().any(|handler| handler.names.iter().any(|candidate| candidate == name))
    }

    pub(crate) fn fire(&mut self, name: &str, state: &mut EventState<'_>) {
        self.dispatch(name, None, None, state);
    }

    pub(crate) fn dispatch(&mut self, name: &str, unit: Option<ArrivingUnit<'_>>, second: Option<ArrivingUnit<'_>>, state: &mut EventState<'_>) {
        if state.board_changes.iter().any(|c| matches!(c, BoardChange::EndLevel { .. })) { return; }
        self.handlers.retain(|handler| {
            if !handler.names.iter().any(|n| n == name)
                || !handler.filters.iter().all(|filter| filter.matches(&state.sides[self.active_side - 1], unit.as_ref(), second.as_ref()))
            {
                return true;
            }
            for change in &handler.changes {
                change.apply(state, &mut self.items, &mut self.variables, unit.as_ref().map(|unit| unit.position), None, self.active_side);
            }
            !handler.once
        });
    }

    pub(crate) fn begin_turn(&mut self, turn: Turn, new_round: bool, state: &mut EventState<'_>) {
        self.active_side = turn.side;
        if new_round && let Some(lesson) = &mut self.wounded { lesson.begin_round(turn.round); }
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

    pub(crate) fn end_side_turn(&mut self, turn: Turn, state: &mut EventState<'_>) {
        if self.handlers.is_empty() { return; }
        let Turn { side, round } = turn;
        self.fire("side_turn_end", state);
        self.fire(&format!("side_{side}_turn_end"), state);
        self.fire(&format!("side_turn_{round}_end"), state);
        self.fire(&format!("side_{side}_turn_{round}_end"), state);
    }

    pub(crate) fn end_round(&mut self, round: i32, state: &mut EventState<'_>) {
        if self.handlers.is_empty() { return; }
        self.fire("turn_end", state);
        self.fire(&format!("turn_{round}_end"), state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{fixed_match::Action, scenario_bundle::Bundle, wml};

    #[test]
    fn side_filters_follow_the_turn_owner_independently_of_the_event_unit() {
        let scenario = wml::parse(r#"
[side]
team_name=Knights
[/side]
[side]
team_name=Allies,Scouts
[/side]
[event]
name=capture
[filter_side]
side=1-3
side_in=2
team_name=Scouts
[/filter_side]
[gold]
side=1
amount=7
[/gold]
[filter]
side=1
[/filter]
[/event]
[event]
name=capture
[filter_side]
controller=human
[/filter_side]
[gold]
side=1
amount=500
[/gold]
[/event]"#).unwrap();
        let mut sides = crate::scenario_sides::read_sides(&scenario);
        let mut gold = [0, 0];
        let mut state = EventState { gold: &mut gold, sides: &mut sides, board_changes: Vec::new() };
        let mut events = ScenarioEvents::read(&scenario, 2);
        let capture = |side| ArrivingUnit { position: Hex { x: 0, y: 0 }, side, id: None, type_id: "Guard" };

        events.dispatch("capture", Some(capture(1)), None, &mut state);
        assert_eq!(state.gold[0], 0);
        events.begin_turn(Turn { round: 1, side: 2 }, false, &mut state);
        events.dispatch("capture", Some(capture(2)), None, &mut state);
        assert_eq!(state.gold[0], 0);
        events.dispatch("capture", Some(capture(1)), None, &mut state);
        assert_eq!(state.gold[0], 7);
        events.dispatch("capture", Some(capture(1)), None, &mut state);
        assert_eq!(state.gold[0], 7);
    }

    fn event(name: &str, once: &str, side: &str, amount: &str) -> String {
        format!("[event]\nname={name}\nfirst_time_only={once}\n[gold]\nside={side}\namount={amount}\n[/gold]\n[/event]\n")
    }

    #[test]
    fn higher_priority_handlers_run_first_and_equal_priorities_keep_document_order() {
        let mut source = String::from("[side]\nrecruit=Guard\n[/side]\n");
        for (priority, tag, kind) in [
            ("-1", "disallow_recruit", "Archer"),
            ("0", "allow_recruit", "Scout"),
            ("0", "disallow_recruit", "Scout"),
            ("2.5", "allow_recruit", "Archer"),
        ] {
            source.push_str(&format!("[event]\nname=start\npriority={priority}\n[{tag}]\ntype={kind}\n[/{tag}]\n[/event]\n"));
        }
        let config = wml::parse(source).unwrap();
        let mut events = ScenarioEvents::read(&config, 1);
        let mut sides = crate::scenario_sides::read_sides(&config);
        let mut gold = [0];
        events.fire("start", &mut EventState { gold: &mut gold, sides: &mut sides, board_changes: Vec::new() });
        assert_eq!(sides[0].recruits, BTreeSet::from(["Guard".into()]));
    }

    #[test]
    fn opening_event_places_a_veteran_with_authored_traits_and_claims_its_village() {
        let placement = |id: &str, x: i32| format!(r#"
[unit]
    side=1
    type=Veteran
    id={id}
    x,y={x},1
    [modifications]
        [trait]
            id=loyal
            [effect]
                apply_to=loyal
            [/effect]
        [/trait]
        [trait]
            id=resilient
            [effect]
                apply_to=hitpoints
                increase_total=4
            [/effect]
            [effect]
                apply_to=hitpoints
                increase_total=1
                times=per level
            [/effect]
        [/trait]
    [/modifications]
[/unit]"#);
        let visible = placement("gatekeeper", 2);
        let hidden = placement("conditional", 3);
        let config = format!(r#"
[terrain_type]
    string=Vh
    gives_income=yes
[/terrain_type]
[units]
    [unit_type]
        id=Veteran
        level=2
        hitpoints=55
    [/unit_type]
[/units]
[scenario]
    map_data="Xv,Xv,Xv,Xv,Xv,Xv
Xv,1 Gg,Vh,Vh,2 Gg,Xv
Xv,Xv,Xv,Xv,Xv,Xv"
    [side]
        type=Veteran
    [/side]
    [side]
        type=Veteran
    [/side]
    [event]
        name=start
        [capture_village]
            side=2
            x,y=2,1
        [/capture_village]
        [message]
            speaker=narrator
            message=A veteran arrives.
        [/message]
        [move_unit_fake]
            type=Veteran
            side=1
            x=1,2
            y=1,1
        [/move_unit_fake]
        {visible}
        [repeat]
            times=3
            [do]
                [redraw]
                [/redraw]
            [/do]
        [/repeat]
    [/event]
    [event]
        name=start
        [filter]
            side=1
        [/filter]
        {hidden}
    [/event]
    [event]
        name=prestart
        [if]
            [false]
            [/false]
            [then]
                {hidden}
            [/then]
        [/if]
    [/event]
[/scenario]"#);
        let bundle = Bundle::parse(config.as_bytes()).unwrap();
        let game = bundle.start(1, |_| unreachable!()).unwrap();
        let snapshot = game.audit_snapshot();
        let village = Hex { x: 1, y: 0 };
        let veteran = snapshot.units.iter().find(|unit| unit.position == village).unwrap();
        assert_eq!(snapshot.units.len(), 3);
        assert_eq!((veteran.side, veteran.hitpoints), (1, 61));
        assert_eq!(veteran.traits, ["loyal", "resilient"]);
        assert_eq!(snapshot.villages, [(village, 1)]);
    }

    #[test]
    fn campaign_can_extend_a_final_round_from_time_over() {
        use crate::fixed_match::MatchState;
        let bundle = Bundle::parse(br#"[units]
[unit_type]
id=Guard
hitpoints=22
[/unit_type]
[/units]
[scenario]
turns=1
map_data="Xv,Xv,Xv,Xv
Xv,1 Gg,2 Gg,Xv
Xv,Xv,Xv,Xv"
[side]
type=Guard
[/side]
[side]
type=Guard
[/side]
[event]
name=time over
[modify_turns]
add=100%
[/modify_turns]
[/event]
[/scenario]"#).unwrap();
        let mut game = bundle.start(1, |_| unreachable!()).unwrap();
        game.step(Action::EndTurn).unwrap();
        assert_eq!(game.step(Action::EndTurn).unwrap(), MatchState::Playing(Turn { round: 2, side: 1 }));
        game.step(Action::EndTurn).unwrap();
        assert_eq!(game.step(Action::EndTurn).unwrap(), MatchState::Draw);
        assert_eq!(TurnLimitChange::Set(-1).apply(Some(4)), None);
        assert_eq!(TurnLimitChange::Add(3).apply(Some(4)), Some(7));
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
    fn terrain_events_apply_paired_coordinate_ranges_and_clip_at_the_map_edge() {
        let bundle = Bundle::parse(br#"[terrain_type]
string=Hh
[/terrain_type]
[units]
[unit_type]
id=Guard
hitpoints=20
[/unit_type]
[/units]
[scenario]
map_data="Xv,Xv,Xv,Xv,Xv,Xv
Xv,1 Gg,Gg,Gg,2 Gg,Xv
Xv,Gg,Gg,Gg,Gg,Xv
Xv,Xv,Xv,Xv,Xv,Xv"
[side]
type=Guard
[/side]
[side]
type=Guard
[/side]
[event]
name=prestart
[terrain]
x=2-3,4-1000000
y=1,2
terrain=Hh
[/terrain]
[/event]
[/scenario]"#).unwrap();
        let game = bundle.start(1, |_| unreachable!()).unwrap();
        let observation = game.observation_for(1).unwrap();
        let hills: BTreeSet<_> = observation.tiles.iter().filter(|tile| tile.terrain == "Hh")
            .map(|tile| tile.position).collect();
        assert_eq!(hills, [Hex { x: 1, y: 0 }, Hex { x: 2, y: 0 }, Hex { x: 3, y: 1 }].into());
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
    fn set_recruit_replaces_only_selected_sides_in_command_order() {
        let scenario = wml::parse(r#"
[side]
recruit=Guard
[/side]
[side]
recruit=Scout
[/side]
[event]
name=start
[set_recruit]
side=1
recruit=Archer, White Mage
[/set_recruit]
[/event]
[event]
name=turn 2
[set_recruit]
side=1
recruit=""
[/set_recruit]
[/event]"#).unwrap();
        let mut events = ScenarioEvents::read(&scenario, 2);
        let mut sides = crate::scenario_sides::read_sides(&scenario);
        let mut gold = [0; 2];
        let mut state = EventState { gold: &mut gold, sides: &mut sides, board_changes: Vec::new() };
        events.fire("start", &mut state);
        assert_eq!(state.sides[0].recruits.iter().map(String::as_str).collect::<Vec<_>>(), ["Archer", "White Mage"]);
        events.fire("turn_2", &mut state);
        assert!(state.sides[0].recruits.is_empty());
        assert_eq!(state.sides[1].recruits, BTreeSet::from(["Scout".to_owned()]));
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
    fn modify_side_economy_changes_selected_sides_before_income() {
        let source = r#"
[side]
[/side]
[side]
[/side]
[event]
name=turn 2
[modify_side]
side=2
gold=19
income=-3
village_gold=5
village_support=2
[/modify_side]
[/event]"#;
        let scenario = wml::parse(source).unwrap();
        let mut events = ScenarioEvents::read(&scenario, 2);
        let mut sides = crate::scenario_sides::read_sides(&scenario);
        let mut gold = [44; 2];
        let mut state = EventState { gold: &mut gold, sides: &mut sides, board_changes: Vec::new() };
        events.fire("turn_2", &mut state);
        assert_eq!(gold, [44, 19]);
        assert_eq!(crate::income::net_income(&sides[1], 2, 1, 3), 3);
        assert_eq!(crate::income::net_income(&sides[0], 2, 1, 3), 1);
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
        let mut state = EventState { gold: &mut gold, sides: &mut sides, board_changes: Vec::new() };
        for side in 1..=2 {
            let turn = Turn { round: 1, side };
            events.begin_turn(turn, side == 1, &mut state);
            events.refresh(turn, &mut state);
            events.end_side_turn(turn, &mut state);
            if side == 2 { events.end_round(turn.round, &mut state); }
        }
        assert_eq!(gold, [-4, 1, 7]);
    }
}

#[cfg(test)]
mod starting_villages_test {
    use super::*;

    #[test]
    fn assigns_only_village_overlays_within_the_stored_starting_radius() {
        let event = crate::wml::parse("[store_starting_location]\nside=1\nvariable=origin\n[/store_starting_location]\n\
            [capture_village]\nside=2\nterrain=*^V*\n[and]\nx=$origin.x\ny=$origin.y\nradius=2\n[/and]\n[/capture_village]\n\
            [clear_variable]\nname=origin\n[/clear_variable]").unwrap();
        let map = ScenarioMap::parse("Xv,Xv,Xv,Xv,Xv,Xv\nXv,1 Kh,Gg^Ve,Gg^Ff,Gg^Vh,Xv\nXv,Xv,Xv,Xv,Xv,Xv").unwrap();
        let actions: Vec<_> = event.children.iter().collect();
        let capture = VillageCapture::from_starting_location(&actions).unwrap();
        assert_eq!(capture.side, 2);
        let mut tiles = Vec::new();
        capture.for_each_hex(&map, |hex| tiles.push(hex));
        assert_eq!(tiles, [Hex { x: 1, y: 0 }]);
    }
}

#[cfg(test)]
mod terrain_event_test {
    use crate::{scenario_bundle::Bundle, hex::Hex};

    #[test]
    fn campaign_terrain_layers_keep_the_other_layer_and_fallback_uses_the_default_base() {
        let source = br#"[terrain_type]
string=Gg
[/terrain_type]
[terrain_type]
string=Hh
[/terrain_type]
[terrain_type]
string=^Vh
default_base=Gg
gives_income=yes
[/terrain_type]
[units]
[unit_type]
id=Guard
hitpoints=20
[/unit_type]
[/units]
[scenario]
map_data="Xv,Xv,Xv,Xv,Xv,Xv
Xv,1 Gg,Gg^Vh,Zz,2 Hh,Xv
Xv,Xv,Xv,Xv,Xv,Xv"
[side]
type=Guard
[/side]
[side]
type=Guard
[/side]
[event]
name=prestart
[terrain]
x,y=2,1
terrain=Hh
layer=base
[/terrain]
[terrain]
x,y=1,1
terrain=^Vh
layer=overlay
[/terrain]
[terrain]
x,y=3,1
terrain=^Vh
layer=overlay
replace_if_failed=yes
[/terrain]
[terrain]
x,y=4,1
terrain=^Z
layer=overlay
[/terrain]
[/event]
[/scenario]"#;
        let bundle = Bundle::parse(source).unwrap();
        let game = bundle.start(1, |_| unreachable!()).unwrap();
        let view = game.observation_for(1).unwrap();
        assert_eq!(view.tiles.iter().map(|tile| tile.terrain.as_str()).collect::<Vec<_>>(),
                   ["Gg^Vh", "Hh^Vh", "Gg^Vh", "Hh"]);
    }

    #[test]
    fn a_new_campaign_village_can_be_claimed_later_in_the_same_event() {
        let source = br#"[terrain_type]
string=Gg^Vh
gives_income=yes
[/terrain_type]
[units]
[unit_type]
id=Guard
hitpoints=10
[/unit_type]
[/units]
[scenario]
map_data="Xv,Xv,Xv,Xv,Xv
Xv,1 Gg,Gg,2 Gg,Xv
Xv,Xv,Xv,Xv,Xv"
[side]
type=Guard
[/side]
[side]
type=Guard
[/side]
[event]
name=prestart
[objectives]
side=1
[/objectives]
[terrain]
terrain=Gg^Vh
x,y=2,1
[/terrain]
[capture_village]
side=2
x,y=2,1
[/capture_village]
[/event]
[/scenario]"#;
        let bundle = Bundle::parse(source).unwrap();
        let game = bundle.start(1, |_| unreachable!()).unwrap();
        assert_eq!(game.audit_snapshot().villages, [(Hex { x: 1, y: 0 }, 2)]);
        assert_eq!(game.observation_for(1).unwrap().tiles[1].terrain, "Gg^Vh");
    }
}

#[cfg(test)]
mod reinforcements {
    use crate::{fixed_match::Action, hex::Hex, scenario_bundle::Bundle};

    #[test]
    fn turn_reinforcements_take_vacancies_and_reuse_or_enter_the_recall_list() {
        let source = br#"[terrain_type]
string=Vh
gives_income=yes
[/terrain_type]
[units]
[unit_type]
id=Guard
hitpoints=22
movement=5
[/unit_type]
[/units]
[scenario]
map_data="Xv,Xv,Xv,Xv,Xv,Xv
Xv,1 Gg,Vh,2 Gg,Gg,Xv
Xv,Xv,Xv,Xv,Xv,Xv"
[side]
type=Guard
[/side]
[side]
type=Guard
[unit]
id=elder
type=Guard
placement=recall
moves=3
experience=7
[/unit]
[/side]
[event]
name=turn 2
[message]
speaker=narrator
message=Help has arrived.
[/message]
[unit]
type=Guard
side=2
x,y=3,1
[/unit]
[unit]
type=Guard
side=2
id=elder
x,y=3,1
[/unit]
[unit]
type=Guard
side=2
x,y=3,1
experience=11
[/unit]
[/event]
[event]
name=capture
[filter]
id=Guard-4
[/filter]
[gold]
side=2
amount=9
[/gold]
[/event]
[/scenario]"#;
        let bundle = Bundle::parse(source).unwrap();
        let mut game = bundle.start(7, |_| unreachable!()).unwrap();
        assert_eq!(game.audit_snapshot().units.len(), 2);
        for _ in 0..2 { game.step(Action::EndTurn).unwrap(); }
        let snapshot = game.audit_snapshot();
        let village = Hex { x: 1, y: 0 };
        assert_eq!(snapshot.villages, [(village, 2)]);
        assert_eq!(snapshot.gold[1], 109);
        let arrival = snapshot.units.iter().find(|u| u.position == village).unwrap();
        assert_eq!((arrival.side, arrival.moves_left, arrival.attacks_left), (2, 5, 1));
        let elder = snapshot.units.iter().find(|u| u.position == Hex { x: 3, y: 0 }).unwrap();
        assert_eq!((elder.moves_left, elder.experience), (3, 7));
        assert_eq!(snapshot.reserves[1].len(), 1);
        assert_eq!(snapshot.reserves[1][0].experience, 11);
    }
}
