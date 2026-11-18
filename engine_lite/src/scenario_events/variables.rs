//! Scalar variables owned by one running scenario.

use std::collections::BTreeMap;

use crate::wml::Node;

pub(super) type Scalars = BTreeMap<String, String>;

fn scalar_name(name: &str) -> bool {
    // Coordinates and turn counters belong to the active event context.
    !matches!(name, "" | "x1" | "x2" | "y1" | "y2" | "turn_number" | "side_number")
        && name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
}

pub(super) fn initial(scenario: &Node) -> Scalars {
    scenario.children.iter().find(|(tag, _)| tag == "variables")
        .into_iter().flat_map(|(_, node)| &node.attributes)
        .filter(|(name, value)| scalar_name(name) && value.translations.is_empty())
        .map(|(name, value)| (name.clone(), value.text.clone()))
        .collect()
}

#[derive(Clone, Debug)]
pub(super) enum ScalarChange {
    Set { name: String, initial: Option<String>, arithmetic: Vec<Arithmetic> },
    Clear(Vec<String>),
}

#[derive(Clone, Debug)]
pub(super) enum Arithmetic { Add(f64), Subtract(f64), Multiply(f64), Divide(f64), Modulo(f64) }

impl Arithmetic {
    fn apply(&self, value: f64) -> f64 {
        match *self {
            Self::Add(n) => value + n,
            Self::Subtract(n) => value - n,
            Self::Multiply(n) => value * n,
            Self::Divide(n) => value / n,
            Self::Modulo(n) => value - n * (value / n).floor(),
        }
    }
}

impl ScalarChange {
    pub(super) fn read(tag: &str, cfg: &Node) -> Option<Self> {
        if !cfg.children.is_empty() || cfg.attributes.values().any(|value| !value.translations.is_empty()) {
            return None;
        }
        let name = cfg.attribute("name")?;
        if tag == "clear_variable" && cfg.attributes.len() == 1 {
            let names: Vec<_> = name.split(',').map(str::trim).collect();
            if names.iter().all(|name| scalar_name(name)) {
                return Some(Self::Clear(names.into_iter().map(str::to_owned).collect()));
            }
        }
        if tag != "set_variable" || !scalar_name(name)
            || cfg.attributes.keys().any(|key| !matches!(key.as_str(), "name" | "value" | "literal" | "add" | "sub" | "multiply" | "divide" | "modulo"))
        {
            return None;
        }
        // The literal assignment follows value in the original Lua action.
        let initial = cfg.attribute("literal").or_else(|| cfg.attribute("value"));
        if cfg.attribute("literal").is_none() && initial.is_some_and(|v| v.contains('$')) { return None; }
        let initial = initial.map(|text| match text { "true" => "yes", "false" => "no", other => other }.to_owned());
        // set_variable.lua applies numeric attributes in this order regardless of their WML order.
        let mut arithmetic = Vec::new();
        for key in ["add", "sub", "multiply", "divide", "modulo"] {
            let Some(operand) = cfg.attribute(key) else { continue };
            if operand.contains('$') { return None; }
            let n = number(operand);
            let op = match key {
                "add" => Arithmetic::Add(n),
                "sub" => Arithmetic::Subtract(n),
                "multiply" => Arithmetic::Multiply(n),
                "divide" if n != 0.0 => Arithmetic::Divide(n),
                "modulo" if n != 0.0 => Arithmetic::Modulo(n),
                _ => return None,
            };
            arithmetic.push(op);
        }
        (initial.is_some() || !arithmetic.is_empty()).then(|| Self::Set { name: name.to_owned(), initial, arithmetic })
    }

    pub(super) fn apply(&self, variables: &mut Scalars) {
        match self {
            Self::Set { name, initial, arithmetic } => {
                if let Some(value) = initial { variables.insert(name.clone(), value.clone()); }
                if !arithmetic.is_empty() {
                    let mut value = variables.get(name).map_or(0.0, |s| number(s));
                    for op in arithmetic { value = op.apply(value); }
                    variables.insert(name.clone(), value.to_string());
                }
            }
            Self::Clear(names) => {
                for name in names { variables.remove(name); }
            }
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct Comparison {
    name: String,
    test: Test,
}

#[derive(Clone, Debug)]
enum Test {
    Equals(String), NotEquals(String), Contains(String),
    Boolean(bool),
    NumberEquals(f64), NumberDiffers(f64),
    Less(f64), AtMost(f64), Greater(f64), AtLeast(f64),
}

fn boolean(value: &str) -> bool { matches!(value, "yes" | "true") }

fn number(value: &str) -> f64 { value.trim().parse().unwrap_or(0.0) }

impl Comparison {
    pub(super) fn read(cfg: &Node) -> Option<Self> {
        if cfg.attributes.len() != 2 || !cfg.children.is_empty()
            || cfg.attributes.values().any(|v| !v.translations.is_empty() || v.text.contains('$'))
        {
            return None;
        }
        let name = cfg.attribute("name").filter(|name| scalar_name(name))?.to_owned();
        let (op, value) = cfg.attributes.iter().find(|(key, _)| key.as_str() != "name")?;
        let v = &value.text;
        let test = match op.as_str() {
            "equals" => Test::Equals(v.clone()),
            "not_equals" => Test::NotEquals(v.clone()),
            "contains" => Test::Contains(v.clone()),
            "boolean_equals" => Test::Boolean(boolean(v)),
            "boolean_not_equals" => Test::Boolean(!boolean(v)),
            "numerical_equals" => Test::NumberEquals(number(v)),
            "numerical_not_equals" => Test::NumberDiffers(number(v)),
            "less_than" => Test::Less(number(v)),
            "less_than_equal_to" => Test::AtMost(number(v)),
            "greater_than" => Test::Greater(number(v)),
            "greater_than_equal_to" => Test::AtLeast(number(v)),
            _ => return None,
        };
        Some(Self { name, test })
    }

    pub(super) fn matches(&self, variables: &Scalars) -> bool {
        let actual = variables.get(&self.name).map_or("", String::as_str);
        match &self.test {
            Test::Equals(value) => actual == value,
            Test::NotEquals(value) => actual != value,
            Test::Contains(value) => actual.contains(value),
            Test::Boolean(value) => boolean(actual) == *value,
            Test::NumberEquals(n) => number(actual) == *n,
            Test::NumberDiffers(n) => number(actual) != *n,
            Test::Less(n) => number(actual) < *n,
            Test::AtMost(n) => number(actual) <= *n,
            Test::Greater(n) => number(actual) > *n,
            Test::AtLeast(n) => number(actual) >= *n,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{fixed_match::Action, scenario_bundle::Bundle};

    #[test]
    fn campaign_counter_arithmetic_uses_lua_order_and_modulo() {
        let config = br#"[units]
[unit_type]
id=Guard
hitpoints=18
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
name=prestart
[set_variable]
name=score
multiply=2
add=3
[/set_variable]
[/event]
[event]
name=side 2 turn
[set_variable]
name=score
modulo=4
divide=2
sub=11
[/set_variable]
[if]
[variable]
name=score
numerical_equals=1.5
[/variable]
[then]
[gold]
side=1
amount=17
[/gold]
[/then]
[/if]
[/event]
[/scenario]"#;
        let bundle = Bundle::parse(config).unwrap();
        let mut game = bundle.start(7, |_| unreachable!()).unwrap();
        game.step(Action::EndTurn).unwrap();
        assert_eq!(game.audit_snapshot().gold[0], 117);
    }

    #[test]
    fn campaign_variables_choose_branches_in_command_order_across_events() {
        let config = br#"[units]
[unit_type]
id=Guard
hitpoints=21
[/unit_type]
[/units]
[scenario]
map_data="Xv,Xv,Xv,Xv
Xv,1 Kh,2 Kh,Xv
Xv,Xv,Xv,Xv"
[side]
type=Guard
gold=40
[/side]
[side]
type=Guard
[/side]
[variables]
route=coast
rounds_held=18
[/variables]
[event]
name=start
[set_variable]
name=leader_attacked
value=false
[/set_variable]
[if]
    [variable]
        name=route
        equals=coast
    [/variable]
    [variable]
        name=leader_attacked
        boolean_not_equals=yes
    [/variable]
    [then]
        [set_variable]
            name=leader_attacked
            value=yes
        [/set_variable]
        [if]
            [variable]
                name=rounds_held
                greater_than_equal_to=18
            [/variable]
            [then]
                [gold]
                    side=1
                    amount=7
                [/gold]
            [/then]
        [/if]
    [/then]
    [else]
        [gold]
            side=1
            amount=99
        [/gold]
    [/else]
[/if]
[/event]
[event]
name=side 2 turn
[if]
    [variable]
        name=leader_attacked
        boolean_equals=true
    [/variable]
    [then]
        [clear_variable]
            name=route,leader_attacked
        [/clear_variable]
    [/then]
[/if]
[if]
    [variable]
        name=route
        equals=coast
    [/variable]
    [then]
        [gold]
            side=1
            amount=99
        [/gold]
    [/then]
    [else]
        [gold]
            side=1
            amount=3
        [/gold]
    [/else]
[/if]
[/event]
[/scenario]"#;
        let bundle = Bundle::parse(config).unwrap();
        let start = || bundle.start(1, |_| unreachable!()).unwrap();
        let mut game = start();
        assert_eq!(game.audit_snapshot().gold[0], 47);
        game.step(Action::EndTurn).unwrap();
        assert_eq!(game.audit_snapshot().gold[0], 50);
        assert_eq!(start().audit_snapshot().gold[0], 47);
    }
}
