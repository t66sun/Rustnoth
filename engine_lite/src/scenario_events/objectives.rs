//! Render campaign objective text when WML sets or refreshes it.
use super::{HaveUnit, ScenarioEvents, variables::{Comparison, Scalars}};
use crate::{board::Battlefield, scenario_sides::{ScenarioObjectives, SideConfig}, wml::Node};

#[derive(Clone, Debug, Default)]
pub(crate) struct Definition {
    summary: String,
    entries: Vec<Entry>,
}

#[derive(Clone, Debug)]
struct Entry {
    victory: bool,
    description: String,
    condition: Condition,
    show_turn_counter: bool,
}

#[derive(Clone, Debug, Default)]
struct Condition {
    units: Vec<HaveUnit>,
    variables: Vec<Comparison>,
    literal: Option<bool>,
    subconditions: Vec<(Operator, Condition)>,
}

#[derive(Clone, Copy, Debug)]
enum Operator { And, Or, Not }

impl Condition {
    fn read(node: &Node) -> Option<Self> {
        let mut cond = Self::default();
        // conditional_wml::internal_conditional_passed checks true before false,
        // then conditional_passed applies and/or/not in declaration order.
        cond.literal = [("true", true), ("false", false)].into_iter()
            .find_map(|(tag, value)| node.children.iter().any(|(t, _)| t == tag).then_some(value));
        for (tag, child) in &node.children {
            let operator = match tag.as_str() {
                "and" => Operator::And,
                "or" => Operator::Or,
                "not" => Operator::Not,
                _ if cond.literal.is_some() => continue,
                "have_unit" => { cond.units.push(HaveUnit::read(child)?); continue; }
                "variable" => { cond.variables.push(Comparison::read(child)?); continue; }
                _ => return None,
            };
            cond.subconditions.push((operator, Self::read(child)?));
        }
        Some(cond)
    }

    fn matches(&self, board: &Battlefield<'_>, sides: usize, variables: &Scalars) -> bool {
        let mut matches = self.literal.unwrap_or_else(|| {
            self.variables.iter().all(|test| test.matches(variables))
                && self.units.iter().all(|test| test.matches(board, sides))
        });
        for (operator, child) in &self.subconditions {
            matches = match operator {
                Operator::And => matches && child.matches(board, sides, variables),
                Operator::Or => matches || child.matches(board, sides, variables),
                Operator::Not => matches && !child.matches(board, sides, variables),
            };
        }
        matches
    }
}

impl Definition {
    pub(super) fn read(node: &Node) -> Self {
        let entries = node.children.iter().filter(|(tag, _)| tag == "objective").filter_map(|(_, entry)| {
            let victory = match entry.attribute("condition")? {
                "win" => true,
                "lose" => false,
                _ => return None,
            };
            let description = entry.attribute("description")?.to_owned();
            if description.trim().is_empty() || description.contains('$') { return None; }
            let condition = match entry.children.iter().find(|(t, _)| t == "show_if") {
                Some((_, node)) => Condition::read(node)?,
                None => Condition::default(),
            };
            let show_turn_counter = crate::scenario_sides::bool_attribute(entry, "show_turn_counter", false);
            Some(Entry { victory, description, condition, show_turn_counter })
        }).collect();
        let summary = node.attribute("summary").filter(|text| !text.contains('$')).unwrap_or_default().to_owned();
        Self { summary, entries }
    }

    fn evaluate(&self, board: &Battlefield<'_>, sides: usize, variables: &Scalars, remaining: Option<u64>) -> ScenarioObjectives {
        let mut text = ScenarioObjectives { summary: self.summary.clone(), ..Default::default() };
        for entry in &self.entries {
            if entry.condition.matches(board, sides, variables) {
                let (group, counters) = if entry.victory {
                    (&mut text.victory, &mut text.victory_turns)
                } else {
                    (&mut text.defeat, &mut text.defeat_turns)
                };
                if entry.show_turn_counter && let Some(turns) = remaining { counters.insert(group.len(), turns); }
                group.push(entry.description.clone());
            }
        }
        text
    }
}

impl ScenarioEvents {
    fn objective_definition(&self, side: usize) -> Option<&Definition> {
        self.objective_definitions.get(&side).or_else(|| self.objective_definitions.get(&0))
    }

    pub(crate) fn has_objectives(&self, side: usize) -> bool {
        self.objective_definition(side).is_some()
    }

    pub(crate) fn evaluated_objectives(&self, side: usize, board: &Battlefield<'_>, sides: usize, remaining: Option<u64>) -> Option<ScenarioObjectives> {
        self.objective_definition(side).map(|definition| definition.evaluate(board, sides, &self.variables, remaining))
    }

    pub(crate) fn set_objectives(&mut self, definition: Definition, indices: &[usize], notify: bool,
        board: &Battlefield<'_>, sides: &mut [SideConfig], remaining: Option<u64>) {
        let text = definition.evaluate(board, sides.len(), &self.variables, remaining);
        let global = indices.len() == sides.len();
        for &index in indices {
            sides[index].objectives.clone_from(&text);
            sides[index].objectives_changed = notify;
            if !global { self.objective_definitions.insert(index + 1, definition.clone()); }
        }
        // objectives.lua retains side-specific overrides when updating the global list.
        if global { self.objective_definitions.insert(0, definition); }
    }

    pub(crate) fn refresh_objectives(&self, index: usize, board: &Battlefield<'_>, sides: &mut [SideConfig], remaining: Option<u64>) {
        if let Some(content) = self.evaluated_objectives(index + 1, board, sides.len(), remaining) {
            sides[index].objectives = content;
        }
        sides[index].objectives_changed = true;
    }
}

#[cfg(test)]
mod tests {
    use crate::{fixed_match::Action, scenario_bundle::Bundle};

    #[test]
    fn objective_turn_counters_follow_the_limit_when_wml_or_the_player_refreshes() {
        let config = r#"[units]
[unit_type]
id=Guard
hitpoints=20
[/unit_type]
[/units]
[scenario]
turns=2
map_data="Xv,Xv,Xv,Xv
Xv,1 Gg,2 Gg,Xv
Xv,Xv,Xv,Xv"
[side]
type=Guard
controller=human
[/side]
[side]
type=Guard
[/side]
[event]
name=start
[objectives]
side=1
[objective]
condition=win
description=Hold the ford
show_turn_counter=yes
[/objective]
[objective]
condition=lose
description=Death of the commander
[/objective]
[objective]
condition=lose
description=Turns run out
show_turn_counter=yes
[/objective]
[/objectives]
[/event]
[event]
name=side 2 turn 2
[show_objectives]
side=1
[/show_objectives]
[/event]
[/scenario]"#;
        let bundle = Bundle::parse(config.as_bytes()).unwrap();
        let mut game = bundle.start(1, |_| unreachable!()).unwrap();
        let first = game.objectives_for(1).unwrap().clone();
        assert_eq!(first.victory_turns, [(0, 2)].into());
        assert_eq!(first.defeat_turns, [(1, 2)].into());
        game.step(Action::EndTurn).unwrap();
        game.step(Action::EndTurn).unwrap();
        assert_eq!(game.objectives_for(1), Some(&first));
        let manual = game.evaluate_objectives_for(1).unwrap();
        assert_eq!(manual.victory_turns, [(0, 1)].into());
        assert_eq!(manual.defeat_turns, [(1, 1)].into());
        game.step(Action::EndTurn).unwrap();
        assert_eq!(game.objectives_for(1), Some(&manual));

        let unlimited = Bundle::parse(config.replace("turns=2", "turns=-1").as_bytes()).unwrap();
        let game = unlimited.start(1, |_| unreachable!()).unwrap();
        let text = game.objectives_for(1).unwrap();
        assert_eq!(text.defeat, ["Death of the commander", "Turns run out"]);
        assert!(text.victory_turns.is_empty() && text.defeat_turns.is_empty());
    }

    #[test]
    fn south_guard_objectives_are_regenerated_at_the_scripted_refresh() {
        let bundle = Bundle::parse(br#"
[units]
[unit_type]
id=Guard
hitpoints=20
[/unit_type]
[/units]
[scenario]
map_data="Xv,Xv,Xv,Xv,Xv
Xv,1 Gg,Gg,2 Gg,Xv
Xv,Xv,Xv,Xv,Xv"
[side]
type=Guard
id=Deoran
[/side]
[side]
type=Guard
id=Urza Mathin
[/side]
[event]
name=prestart
[objectives]
side=1
[objective]
description=Investigate the river fort
condition=win
[show_if]
[not]
[have_unit]
id=Sir Gerrick
[/have_unit]
[/not]
[/show_if]
[/objective]
[objective]
description=Defeat Urza Mathin
condition=win
[show_if]
[have_unit]
id=Urza Mathin
[/have_unit]
[/show_if]
[/objective]
[objective]
description=Death of Sir Gerrick
condition=lose
[show_if]
[have_unit]
id=Sir Gerrick
[/have_unit]
[variable]
name=rescued
boolean_equals=yes
[/variable]
[/show_if]
[/objective]
[/objectives]
[/event]
[event]
name=side 2 turn
[unit]
side=1
type=Guard
id=Sir Gerrick
x,y=2,1
[/unit]
[set_variable]
name=rescued
value=yes
[/set_variable]
[/event]
[event]
name=side 2 turn end
[show_objectives]
side=1
[/show_objectives]
[/event]
[/scenario]"#).unwrap();
        let mut game = bundle.start(7, |_| unreachable!()).unwrap();
        let initial = game.objectives_for(1).unwrap().clone();
        assert_eq!(initial.victory, ["Investigate the river fort", "Defeat Urza Mathin"]);
        assert!(initial.defeat.is_empty());
        game.step(Action::EndTurn).unwrap();
        assert_eq!(game.objectives_for(1), Some(&initial));
        let manual = game.evaluate_objectives_for(1).unwrap();
        assert_eq!(manual.victory, ["Defeat Urza Mathin"]);
        assert_eq!(manual.defeat, ["Death of Sir Gerrick"]);
        game.step(Action::EndTurn).unwrap();
        let refreshed = game.objectives_for(1).unwrap();
        assert_eq!(refreshed.victory, ["Defeat Urza Mathin"]);
        assert_eq!(refreshed.defeat, ["Death of Sir Gerrick"]);
        assert!(game.objectives_for(2).is_none());
    }

    #[test]
    fn global_refresh_restores_each_sides_last_specific_definition() {
        let goal = |side, text| format!(
            "[objectives]\nside={side}\n[objective]\ncondition=win\ndescription={text}\n[/objective]\n[/objectives]"
        );
        let config = format!(r#"[units]
[unit_type]
id=Guard
hitpoints=20
[/unit_type]
[/units]
[scenario]
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
name=start
{}
{}
[/event]
[event]
name=side 2 turn
[show_objectives]
[/show_objectives]
[/event]
[/scenario]"#, goal(1, "Hold the bridge"), goal(0, "Defeat the commander"));
        let bundle = Bundle::parse(config.as_bytes()).unwrap();
        let mut game = bundle.start(1, |_| unreachable!()).unwrap();
        assert_eq!(game.objectives_for(1).unwrap().victory, ["Defeat the commander"]);
        game.step(Action::EndTurn).unwrap();
        assert_eq!(game.objectives_for(1).unwrap().victory, ["Hold the bridge"]);
        assert_eq!(game.objectives_for(2).unwrap().victory, ["Defeat the commander"]);
    }
}
