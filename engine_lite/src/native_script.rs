use std::{fmt::{Display, Write}, io};

use crate::{
    fixed_match::Action,
    hex::Hex,
    native_turns,
    wml::{Node, Value},
};

pub struct NativeScript {
    /// Contents of the WML [event] children that run the script.
    pub events: Vec<Node>,
    pub lua: String,
}

const LUA_HEADER: &str = r#"local capture = wesnoth.require('~add-ons/RustnothAudit/capture.lua')
local actions = wesnoth.require('~add-ons/RustnothAudit/actions.lua')
local groups = {}

local function checkpoint(step, time_over)
    if capture.campaign_pending() then return true end
    local terminal = capture.snapshot(step, time_over)
    if terminal and step ~= last_step then
        wesnoth.log("err", "audit script has actions after terminal step " .. step)
        wesnoth.wml_actions.endlevel { test_result = "fail" }
        return true
    end
    if step == last_step then
        wesnoth.wml_actions.endlevel { test_result = "pass" }
        return true
    end
    return false
end

"#;

/// Generates native audit events for sides 1 and 2.
pub fn generate(
    actions: &[Action],
    first_round: i32,
    last_round: Option<i32>,
) -> io::Result<NativeScript> {
    generate_for_sides(actions, first_round, last_round, &[1, 2])
}

/// Generates native side-turn refresh events in active side order and records
/// the initial frame and the frame after each command.
pub fn generate_for_sides(
    actions: &[Action],
    first_round: i32,
    last_round: Option<i32>,
    sides: &[usize],
) -> io::Result<NativeScript> {
    generate_groups(actions, first_round, last_round, sides, false)
}

/// Campaign events can change the limit after the native script is installed.
pub(crate) fn generate_with_turn_changes(actions: &[Action], first_round: i32, sides: &[usize]) -> io::Result<NativeScript> {
    generate_groups(actions, first_round, None, sides, true)
}

fn generate_groups(
    actions: &[Action], first_round: i32, last_round: Option<i32>, sides: &[usize], dynamic_limit: bool,
) -> io::Result<NativeScript> {
    let groups = native_turns::partition_for_sides(actions, first_round, sides)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let mut events = Vec::with_capacity(groups.len());
    let mut lua = format!("local last_step = {}\n{LUA_HEADER}", actions.len());

    if dynamic_limit {
        let mut expired = event("time over".into(), 0);
        expired.attributes.insert("first_time_only".into(), value("no".into()));
        expired.attributes.insert("priority".into(), value("-1000".into()));
        events.push(expired);
        lua.push_str("local pending_end = 0\ngroups[0] = function()\n    local limit = wesnoth.scenario.turns\n    if limit ~= -1 and wesnoth.current.turn > limit then checkpoint(pending_end, true) end\nend\n");
    }

    for (index, group) in groups.iter().enumerate() {
        let index = index + 1;
        let time_over = last_round.is_some_and(|last| group.round > last);
        if time_over && (group.before_step == 0 || group.before_step != actions.len()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "audit group starting at step {} exceeds the round limit",
                    group.before_step
                ),
            ));
        }

        let event_name = if time_over {
            "time over".to_owned()
        } else {
            format!("side {} turn {} refresh", group.side, group.round)
        };
        events.push(event(event_name, index));
        writeln!(lua, "groups[{index}] = function()").unwrap();
        write_checkpoint(&mut lua, group.before_step, time_over);
        for (offset, action) in group.actions.iter().enumerate() {
            let step = group.before_step + offset + 1;
            write_action(&mut lua, action, step)?;
            write_checkpoint(&mut lua, step, false);
        }
        if group.end_turn {
            if dynamic_limit {
                writeln!(lua, "    pending_end = {}", group.before_step + group.actions.len() + 1).unwrap();
            }
            lua.push_str("    actions.end_turn()\n");
        }
        lua.push_str("end\n\n");
    }
    lua.push_str("return { run = function(group) groups[group]() end }\n");
    Ok(NativeScript { events, lua })
}

pub(crate) fn generate_exploration(limit: usize, seed: u32) -> NativeScript {
    let mut refresh = event("turn refresh".to_owned(), false);
    refresh.attributes.insert("first_time_only".to_owned(), value("no".to_owned()));
    let mut time_over = event("time over".to_owned(), true);
    time_over.attributes.insert("first_time_only".into(), value("no".into()));
    time_over.attributes.insert("priority".into(), value("-1000".into()));
    let driver = include_str!("native_explore_driver.lua");
    NativeScript {
        events: vec![refresh, time_over],
        lua: format!("local seed = {seed}\nlocal limit = {limit}\n{driver}"),
    }
}

fn event(name: String, argument: impl Display) -> Node {
    let code = format!("wesnoth.require('~add-ons/RustnothAudit/script.lua').run({argument})");
    Node {
        attributes: [("name".to_owned(), value(name))].into(),
        children: vec![(
            "lua".to_owned(),
            Node {
                attributes: [("code".to_owned(), value(code))].into(),
                ..Node::default()
            },
        )],
    }
}

fn value(text: String) -> Value {
    Value {
        text,
        ..Value::default()
    }
}

fn write_checkpoint(lua: &mut String, step: usize, time_over: bool) {
    writeln!(lua, "    if checkpoint({step}, {time_over}) then return end").unwrap();
}

fn write_action(lua: &mut String, action: &Action, step: usize) -> io::Result<()> {
    match action {
        Action::Move { from, to } => {
            let (from_x, from_y) = coordinates(*from)?;
            let (to_x, to_y) = coordinates(*to)?;
            writeln!(lua, "    actions.move({from_x}, {from_y}, {to_x}, {to_y})").unwrap();
        }
        Action::Attack(attack) => {
            let (from_x, from_y) = coordinates(attack.from)?;
            let (to_x, to_y) = coordinates(attack.to)?;
            writeln!(
                lua,
                "    wesnoth.log(\"info\", \"RUSTNOTH_AUDIT_ATTACK_BEGIN {step}\", false)"
            )
            .unwrap();
            writeln!(
                lua,
                "    actions.attack({from_x}, {from_y}, {to_x}, {to_y}, {})",
                attack.weapon
            )
            .unwrap();
            writeln!(
                lua,
                "    wesnoth.log(\"info\", \"RUSTNOTH_AUDIT_ATTACK_END {step}\", false)"
            )
            .unwrap();
        }
        Action::Recruit { unit_type, to } => {
            let (x, y) = coordinates(*to)?;
            writeln!(
                lua,
                "    wesnoth.log(\"info\", \"RUSTNOTH_AUDIT_RECRUIT_BEGIN {step}\", false)"
            )
            .unwrap();
            write!(lua, "    local trait_draws = actions.recruit({x}, {y}, ").unwrap();
            write_string(lua, unit_type);
            lua.push_str(")\n");
            writeln!(
                lua,
                "    wesnoth.log(\"info\", \"RUSTNOTH_AUDIT_RECRUIT_END {step} \" .. trait_draws, false)"
            )
            .unwrap();
        }
        Action::Recall { unit_id, to } => {
            let (x, y) = coordinates(*to)?;
            write!(lua, "    actions.recall({x}, {y}, ").unwrap();
            write_string(lua, unit_id);
            lua.push_str(")\n");
        }
        Action::Disband { unit_id } => {
            lua.push_str("    actions.disband(");
            write_string(lua, unit_id);
            lua.push_str(")\n");
        }
        Action::EndTurn => unreachable!("turn groups exclude end turns"),
    }
    Ok(())
}

fn coordinates(hex: Hex) -> io::Result<(i32, i32)> {
    hex.to_wml()
        .filter(|&(x, y)| x > 0 && y > 0)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid audit WML coordinates"))
}

fn write_string(lua: &mut String, value: &str) {
    lua.push('"');
    for character in value.chars() {
        match character {
            '"' => lua.push_str("\\\""),
            '\\' => lua.push_str("\\\\"),
            '\x00'..='\x1f' | '\x7f' => write!(lua, "\\{:03}", character as u32).unwrap(),
            other => lua.push(other),
        }
    }
    lua.push('"');
}

#[cfg(test)]
mod tests {
    use crate::board::AttackChoice;

    use super::*;

    #[test]
    fn records_each_step_and_recaptures_after_a_turn_refresh() {
        let native = generate(
            &[
                Action::Move {
                    from: Hex { x: 0, y: 1 },
                    to: Hex { x: 1, y: 2 },
                },
                Action::EndTurn,
            ],
            3,
            None,
        )
        .unwrap();
        assert_eq!(
            native
                .events
                .iter()
                .map(|event| event.attribute("name"))
                .collect::<Vec<_>>(),
            [Some("side 1 turn 3 refresh"), Some("side 2 turn 3 refresh")]
        );
        assert_eq!(native.events[1].children[0].0, "lua");
        assert_eq!(
            native.events[1].children[0].1.attribute("code"),
            Some("wesnoth.require('~add-ons/RustnothAudit/script.lua').run(2)")
        );
        assert!(native.lua.contains(concat!(
            "groups[1] = function()\n",
            "    if checkpoint(0, false) then return end\n",
            "    actions.move(1, 2, 2, 3)\n",
            "    if checkpoint(1, false) then return end\n",
            "    actions.end_turn()\n"
        )));
        assert!(native.lua.contains("groups[2] = function()\n    if checkpoint(2, false)"));
        assert!(native.lua.contains("if terminal and step ~= last_step then"));
    }

    #[test]
    fn recorded_recalls_use_the_unit_id_and_do_not_draw_recruit_traits() {
        let actions = crate::audit_actions::parse("recall\t2\t1\tveteran").unwrap();
        let script = generate(&actions, 1, None).unwrap();
        assert!(script.lua.contains("actions.recall(2, 1, \"veteran\")"));
        assert!(!script.lua.contains("RUSTNOTH_AUDIT_RECRUIT_BEGIN"));
    }

    #[test]
    fn writes_disband_as_an_ordinary_command_with_its_exact_unit_id() {
        let actions = crate::audit_actions::parse("disband\tveteran \"A\"").unwrap();
        assert_eq!(actions, [Action::Disband { unit_id: "veteran \"A\"".into() }]);
        let script = generate(&actions, 1, None).unwrap();
        assert!(script.lua.contains(r#"actions.disband("veteran \"A\"")"#));
        assert!(!script.lua.contains("RUSTNOTH_AUDIT_RECRUIT_BEGIN"));
        assert!(crate::audit_actions::parse("disband\t ").is_err());
    }

    #[test]
    fn preserves_quoted_utf8_unit_types_in_lua() {
        let native = generate(
            &[Action::Recruit {
                unit_type: "Bandit \"暗\"\\\n".to_owned(),
                to: Hex { x: 2, y: 3 },
            }],
            1,
            None,
        )
        .unwrap();
        assert!(native.lua.contains(r#"actions.recruit(3, 4, "Bandit \"暗\"\\\010")"#));
        assert!(native.lua.contains(concat!(
            "    wesnoth.log(\"info\", \"RUSTNOTH_AUDIT_RECRUIT_BEGIN 1\", false)\n",
            "    local trait_draws = actions.recruit(",
        )));
        assert!(native.lua.contains(
            "    wesnoth.log(\"info\", \"RUSTNOTH_AUDIT_RECRUIT_END 1 \" .. trait_draws, false)\n"
        ));
    }

    #[test]
    fn bounds_native_attack_logs_by_one_based_script_step() {
        let native = generate(
            &[
                Action::EndTurn,
                Action::Attack(AttackChoice {
                    from: Hex { x: 0, y: 1 },
                    to: Hex { x: 1, y: 1 },
                    weapon: 0,
                }),
            ],
            1,
            None,
        )
        .unwrap();
        assert!(native.lua.contains(concat!(
            "    wesnoth.log(\"info\", \"RUSTNOTH_AUDIT_ATTACK_BEGIN 2\", false)\n",
            "    actions.attack(1, 2, 2, 2, 0)\n",
            "    wesnoth.log(\"info\", \"RUSTNOTH_AUDIT_ATTACK_END 2\", false)\n",
            "    if checkpoint(2, false) then return end\n",
        )));
    }

    #[test]
    fn uses_the_native_time_over_event_for_the_final_draw_frame() {
        let actions = [Action::EndTurn, Action::EndTurn];
        let native = generate(&actions, 7, Some(7)).unwrap();
        assert_eq!(native.events[2].attribute("name"), Some("time over"));
        assert!(native.lua.contains("groups[3] = function()\n    if checkpoint(2, true)"));

        let too_long = [Action::EndTurn, Action::EndTurn, Action::EndTurn];
        assert!(matches!(
            generate(&too_long, 7, Some(7)),
            Err(error) if error.kind() == io::ErrorKind::InvalidInput
        ));
    }

    #[test]
    fn schedules_the_third_side_before_the_final_round_expires() {
        let actions = [
            Action::EndTurn,
            Action::EndTurn,
            Action::Move {
                from: Hex { x: 0, y: 1 },
                to: Hex { x: 1, y: 2 },
            },
            Action::EndTurn,
        ];
        let native = generate_for_sides(&actions, 7, Some(7), &[1, 2, 3]).unwrap();
        let names: Vec<_> = native.events.iter().map(|event| event.attribute("name")).collect();
        assert_eq!(
            names,
            [
                Some("side 1 turn 7 refresh"),
                Some("side 2 turn 7 refresh"),
                Some("side 3 turn 7 refresh"),
                Some("time over"),
            ],
        );
        assert!(native.lua.contains(concat!(
            "groups[3] = function()\n",
            "    if checkpoint(2, false) then return end\n",
            "    actions.move(1, 2, 2, 3)\n",
            "    if checkpoint(3, false) then return end\n",
            "    actions.end_turn()\n",
        )));
        assert!(native.lua.contains("groups[4] = function()\n    if checkpoint(4, true)"));
    }
}
