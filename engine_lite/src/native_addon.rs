//! Install the original Wesnoth scenario used by the referee audit.

use std::{error::Error, fs, io, path::Path};

use crate::{
    fixed_match::Action,
    native_actions, native_capture, native_script,
    scenario_map,
    scenario_time::integer,
    wml::{self, Node, Value},
    wml_emit,
};

/// Package one preprocessed scenario and its actions in the isolated run's
/// userdata. The original Wesnoth process loads the resulting test add-on.
pub fn install(
    run_dir: &Path,
    bundle: &[u8],
    actions: &[Action],
    seed: u32,
    read_map_file: impl FnOnce(&str) -> io::Result<String>,
) -> Result<(), Box<dyn Error>> {
    install_driver(run_dir, bundle, Driver::Script(actions), seed, read_map_file)
}

/// Run the supplied native chooser until it reaches an action limit or the match ends.
/// The chooser module returns a function of the seed, next step and actions taken this turn.
pub fn install_exploration(
    run_dir: &Path,
    source: &[u8],
    limit: usize,
    seed: u32,
    chooser_lua: &str,
    read_map_file: impl FnOnce(&str) -> io::Result<String>,
) -> Result<(), Box<dyn Error>> {
    install_driver(run_dir, source, Driver::Explore { limit, chooser_lua }, seed, read_map_file)
}

#[derive(Clone, Copy)]
enum Driver<'a> {
    Script(&'a [Action]),
    Explore { limit: usize, chooser_lua: &'a str },
}

fn install_driver(
    run_dir: &Path,
    bundle: &[u8],
    driver: Driver<'_>,
    seed: u32,
    read_map_file: impl FnOnce(&str) -> io::Result<String>,
) -> Result<(), Box<dyn Error>> {
    let (document, script) = prepare(bundle, driver, seed, read_map_file)?;
    let mut config = Vec::new();
    wml_emit::write_config(&mut config, &document)?;

    let addon = run_dir.join("userdata/data/add-ons/RustnothAudit");
    fs::create_dir_all(&addon)?;
    fs::write(addon.join("actions.lua"), native_actions::LUA_SOURCE)?;
    fs::write(addon.join("capture.lua"), native_capture::LUA_SOURCE)?;
    if let Driver::Explore { chooser_lua, .. } = driver {
        fs::write(addon.join("choice.lua"), chooser_lua)?;
    }
    fs::write(addon.join("script.lua"), script)?;
    fs::write(addon.join("_main.cfg"), config)?;
    Ok(())
}

fn prepare(
    bundle: &[u8],
    driver: Driver<'_>,
    seed: u32,
    read_map_file: impl FnOnce(&str) -> io::Result<String>,
) -> Result<(Node, String), Box<dyn Error>> {
    let mut document = wml::parse(bundle)?;
    let _ = unique_block(&document, "units")?;
    let index = unique_block(&document, "scenario")?
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing [scenario] block"))?;
    let (tag, scenario) = &mut document.children[index];

    let mut campaign_end = false;
    for (_, event) in scenario.children.iter_mut().filter(|(tag, _)| tag == "event") {
        campaign_end |= mark_campaign_ends(event)?;
    }
    let manual_victory_sides = crate::scenario_start::manual_victory_sides(scenario);
    let mut sides = Vec::new();
    for (_, side) in scenario.children.iter_mut().filter(|(tag, _)| tag == "side") {
        let expected = sides.len() + 1;
        let declared = integer(side, "side").and_then(|number| usize::try_from(number).ok());
        if side.attribute("side").is_some() && declared != Some(expected) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("native audit expected side {expected} at roster position {expected}"),
            )
            .into());
        }
        // endlevel.lua determines the actual player result from the local human controllers.
        if !campaign_end {
            set_attribute(side, "controller", "human");
        }
        sides.push(expected);
    }
    if sides.len() < 2 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "native audit requires at least two sides",
        )
        .into());
    }

    let first_round = integer(scenario, "turn_at").unwrap_or(1);
    let last_round = integer(scenario, "turns").filter(|&turns| turns != -1);
    fn modifies_turns(node: &Node) -> bool {
        node.children.iter().any(|(tag, child)| tag == "modify_turns" || modifies_turns(child))
    }
    let script = match driver {
        Driver::Script(actions) if modifies_turns(scenario) => native_script::generate_with_turn_changes(actions, first_round, &sides)?,
        Driver::Script(actions) => native_script::generate_for_sides(actions, first_round, last_round, &sides)?,
        Driver::Explore { limit, .. } => native_script::generate_exploration(limit, seed),
    };
    let map = scenario_map::read_data(scenario, read_map_file)?.into_owned();

    *tag = "test".to_owned();
    set_attribute(scenario, "id", "rustnoth_trace");
    set_attribute(scenario, "victory_when_enemies_defeated", "no");
    set_attribute(scenario, "random_seed", format!("{seed:08x}"));
    set_attribute(scenario, "random_calls", "0");
    set_attribute(scenario, "map_data", map);
    scenario.attributes.remove("map_file");
    if campaign_end {
        let expected = match driver {
            Driver::Script(actions) => actions.len().to_string(),
            Driver::Explore { .. } => "nil".to_owned(),
        };
        let mut hook = Node::default();
        set_attribute(&mut hook, "name", "local_victory,local_defeat");
        set_attribute(&mut hook, "priority", "1000000");
        let mut lua = Node::default();
        set_attribute(&mut lua, "code", format!(
            "wesnoth.require('~add-ons/RustnothAudit/capture.lua').campaign_end({expected})"
        ));
        hook.children.push(("lua".into(), lua));
        scenario.children.push(("event".into(), hook));
    }
    scenario
        .children
        .extend(script.events.into_iter().map(|event| ("event".to_owned(), event)));
    let mut lua = String::new();
    if !manual_victory_sides.is_empty() {
        let indices = manual_victory_sides.iter().map(|side| format!("[{side}]=true"))
            .collect::<Vec<_>>().join(",");
        lua.push_str(&format!("wesnoth.require('~add-ons/RustnothAudit/capture.lua').manual_victory_sides = {{{indices}}}\n"));
    }
    lua.push_str(&script.lua);
    Ok((document, lua))
}

/// Mark authored endings on the native test copy. The marker is independent of victory or defeat.
fn mark_campaign_ends(node: &mut Node) -> io::Result<bool> {
    let mut found = false;
    for (tag, child) in &mut node.children {
        if tag == "endlevel" {
            if child.attribute("test_result").is_some() {
                return Err(io::Error::new(io::ErrorKind::InvalidInput,
                    "campaign audit requires [endlevel] without an authored test_result"));
            }
            set_attribute(child, "test_result", "rustnoth_campaign");
            found = true;
        } else {
            found |= mark_campaign_ends(child)?;
        }
    }
    Ok(found)
}

fn unique_block(bundle: &Node, name: &str) -> io::Result<Option<usize>> {
    let mut indices = bundle
        .children
        .iter()
        .enumerate()
        .filter_map(|(index, (tag, _))| (tag == name).then_some(index));
    match (indices.next(), indices.next()) {
        (index, None) => Ok(index),
        (_, Some(_)) => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("duplicate [{name}] blocks"),
        )),
    }
}

fn set_attribute(node: &mut Node, name: &str, text: impl Into<String>) {
    node.attributes.insert(
        name.to_owned(),
        Value {
            text: text.into(),
            ..Value::default()
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAP: &str = "Xv,Xv,Xv,Xv\nXv,1 Kh,2 Kh,Xv\nXv,Xv,Xv,Xv";

    #[test]
    fn campaign_end_audit_preserves_native_side_results_and_captures_the_actual_terminal_frame() {
        let source = br#"[scenario]
map_file=board.map
[side]
controller=human
[/side]
[side]
controller=ai
[/side]
[event]
name=side 1 turn end
[if]
[true]
[/true]
[then]
[endlevel]
result=victory
[result]
side=1
result=defeat
[/result]
[/endlevel]
[/then]
[/if]
[/event]
[/scenario]"#;
        let (doc, lua) = prepare(source, Driver::Script(&[Action::EndTurn]), 9, |_| Ok(MAP.into())).unwrap();
        let scenario = &doc.children[0].1;
        assert_eq!(scenario.children[1].1.attribute("controller"), Some("ai"));
        let child = |node: &Node, tag: &str| node.children.iter().position(|(name, _)| name == tag).unwrap();
        let mut action = scenario;
        for tag in ["event", "if", "then", "endlevel"] {
            action = &action.children[child(action, tag)].1;
        }
        assert_eq!(action.attribute("test_result"), Some("rustnoth_campaign"));
        assert_eq!(action.attribute("result"), Some("victory"));
        assert_eq!(action.children[0].1.attribute("result"), Some("defeat"));
        let hook = scenario.children.iter().find(|(tag, event)| tag == "event"
            && event.attribute("name") == Some("local_victory,local_defeat")).unwrap();
        assert!(hook.1.children[0].1.attribute("code").unwrap().ends_with(".campaign_end(1)"));
        assert!(lua.contains("if capture.campaign_pending() then return true end"));
    }

    #[test]
    fn campaign_turn_changes_schedule_actual_refresh_and_expiration_events() {
        let config = b"[scenario]\nturns=1\nmap_file=test.map\n[side]\n[/side]\n[side]\n[/side]\n\
            [event]\nname=time over\n[modify_turns]\nadd=1\n[/modify_turns]\n[/event]\n[/scenario]";
        let actions = vec![Action::EndTurn; 4];
        let (document, lua) = prepare(config, Driver::Script(&actions), 1, |_| Ok(MAP.into())).unwrap();
        let scenario = &document.children[0].1;
        let hooks: Vec<_> = scenario.children.iter().filter(|(tag, _)| tag == "event")
            .map(|(_, event)| event).collect();
        assert!(hooks.iter().any(|e| e.attribute("name") == Some("side 1 turn 3 refresh")));
        assert!(hooks.iter().any(|e| e.attribute("name") == Some("time over") && e.attribute("first_time_only") == Some("no")));
        assert!(lua.contains("pending_end = 4"));
    }

    #[test]
    fn installs_the_bounded_chooser_with_repeating_refresh_and_terminal_draw_hooks() {
        let bundle = br#"
            [scenario]
                map_file=test.map
                [side]
                [/side]
                [side]
                [/side]
            [/scenario]
        "#;
        let chooser = "return function() return {'end_turn'} end";
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("rustnoth-explore-{}-{nonce}", std::process::id()));
        fs::create_dir(&root).unwrap();
        install_exploration(&root, bundle, 4, 123, chooser, |name| {
            assert_eq!(name, "test.map");
            Ok(MAP.to_owned())
        }).unwrap();
        let addon = root.join("userdata/data/add-ons/RustnothAudit");
        let config = wml::parse(fs::read(addon.join("_main.cfg")).unwrap()).unwrap();
        let installed_choice = fs::read_to_string(addon.join("choice.lua")).unwrap();
        let driver = fs::read_to_string(addon.join("script.lua")).unwrap();
        fs::remove_dir_all(&root).unwrap();

        assert_eq!(installed_choice, chooser);
        assert!(driver.starts_with("local seed = 123\nlocal limit = 4\n"));
        assert!(driver.contains("local command = choose(seed, step, turn_action_count)"));
        let scenario = &config.children[0].1;
        assert_eq!(scenario.attribute("map_data"), Some(MAP));
        let events: Vec<_> = scenario.children.iter().filter(|(tag, _)| tag == "event").collect();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].1.attribute("name"), Some("turn refresh"));
        assert_eq!(events[0].1.attribute("first_time_only"), Some("no"));
        assert_eq!(events[1].1.attribute("name"), Some("time over"));
        for (event, time_over) in events.iter().zip([false, true]) {
            let code = event.1.children[0].1.attribute("code").unwrap();
            assert!(code.ends_with(&format!(".run({time_over})")));
        }
    }

    #[test]
    fn packages_the_third_side_before_the_final_round_ends() {
        let bundle = br#"
            [scenario]
                turns=1
                map_file="three.map"
                [side]
                    side=1
                [/side]
                [side]
                    side=2
                [/side]
                [side]
                    side=3
                    gold=42
                    controller=ai
                [/side]
            [/scenario]
        "#;
        let (document, script) = prepare(
            bundle,
            Driver::Script(&[Action::EndTurn, Action::EndTurn, Action::EndTurn]),
            0,
            |filename| {
                assert_eq!(filename, "three.map");
                Ok("Xv,Xv,Xv,Xv,Xv\nXv,1 Kh,2 Kh,3 Kh,Xv\nXv,Xv,Xv,Xv,Xv".to_owned())
            },
        )
        .unwrap();
        let scenario = &document.children[0].1;
        let third = &scenario.children[2].1;
        assert_eq!(third.attribute("side"), Some("3"));
        assert_eq!(third.attribute("gold"), Some("42"));
        assert_eq!(third.attribute("controller"), Some("human"));
        let events: Vec<_> = scenario
            .children
            .iter()
            .filter(|(tag, _)| tag == "event")
            .map(|(_, event)| event.attribute("name").unwrap())
            .collect();
        assert_eq!(
            events,
            ["side 1 turn 1 refresh", "side 2 turn 1 refresh", "side 3 turn 1 refresh", "time over"]
        );
        assert!(script.contains(concat!(
            "groups[3] = function()\n",
            "    if checkpoint(2, false) then return end\n",
            "    actions.end_turn()\n",
        )));
    }

    #[test]
    fn packages_a_seeded_native_test_with_the_inline_map_and_authored_events() {
        let bundle = format!(
            r#"
            [terrain_type]
                id=keep
            [/terrain_type]
            [units]
            [/units]
            [scenario]
                id=authored
                map_file=unused.map
                map_data="{MAP}"
                turn_at=4
                turns=4
                random_seed=12345678
                random_calls=9
                [side]
                    side=1
                    controller=ai
                    gold=44
                [/side]
                [side]
                    side=2
                    controller=network
                [/side]
                [event]
                    name=start
                    [message]
                        speaker=narrator
                        message=authored
                    [/message]
                [/event]
            [/scenario]
            "#
        );
        let (document, script) = prepare(
            bundle.as_bytes(),
            Driver::Script(&[Action::EndTurn, Action::EndTurn]),
            0x12ab_0005,
            |_| panic!("inline map should not read a file"),
        )
        .unwrap();
        let mut config = Vec::new();
        wml_emit::write_config(&mut config, &document).unwrap();
        assert_eq!(wml::parse(config).unwrap(), document);

        let tags: Vec<_> = document.children.iter().map(|(tag, _)| tag.as_str()).collect();
        assert_eq!(tags, ["terrain_type", "units", "test"]);
        let scenario = &document.children[2].1;
        assert_eq!(scenario.attribute("id"), Some("rustnoth_trace"));
        assert_eq!(scenario.attribute("victory_when_enemies_defeated"), Some("no"));
        assert_eq!(scenario.attribute("random_seed"), Some("12ab0005"));
        assert_eq!(scenario.attribute("random_calls"), Some("0"));
        assert_eq!(scenario.attribute("map_data"), Some(MAP));
        assert_eq!(scenario.attribute("map_file"), None);

        let west = &scenario.children[0].1;
        let east = &scenario.children[1].1;
        assert_eq!(west.attribute("controller"), Some("human"));
        assert_eq!(west.attribute("gold"), Some("44"));
        assert_eq!(east.attribute("controller"), Some("human"));
        let events: Vec<_> = scenario
            .children
            .iter()
            .filter(|(tag, _)| tag == "event")
            .map(|(_, event)| event.attribute("name").unwrap())
            .collect();
        assert_eq!(
            events,
            ["start", "side 1 turn 4 refresh", "side 2 turn 4 refresh", "time over"]
        );
        assert_eq!(scenario.children[2].1.children[0].0, "message");
        assert!(script.contains("local last_step = 2"));
    }

    #[test]
    fn packages_a_core_only_scenario_and_inlines_its_external_map() {
        let bundle = br#"
            [scenario]
                map_data=""
                map_file="shared.map"
                turns=-1
                [side]
                [/side]
                [side]
                [/side]
            [/scenario]
        "#;
        let (document, _) = prepare(bundle, Driver::Script(&[]), 0, |filename| {
            assert_eq!(filename, "shared.map");
            Ok(MAP.to_owned())
        })
        .unwrap();
        let [(tag, scenario)] = document.children.as_slice() else {
            panic!("expected one native test scenario");
        };
        assert_eq!(tag, "test");
        assert_eq!(scenario.attribute("map_data"), Some(MAP));
        assert_eq!(scenario.attribute("map_file"), None);
        let event = &scenario.children[2].1;
        assert_eq!(event.attribute("name"), Some("side 1 turn 1 refresh"));
    }

    #[test]
    fn rejects_ambiguous_bundle_blocks_before_reading_maps() {
        let bundle = b"[units]\n[/units]\n[units]\n[/units]\n[scenario]\n[/scenario]";
        let error = prepare(bundle, Driver::Script(&[]), 0, |_| unreachable!()).unwrap_err();
        assert_eq!(error.to_string(), "duplicate [units] blocks");
    }

    #[test]
    fn rejects_a_declared_side_number_outside_roster_order() {
        let bundle = br#"
            [scenario]
                [side]
                    side=1
                [/side]
                [side]
                    side=3
                [/side]
                [side]
                    side=2
                [/side]
            [/scenario]
        "#;
        let error = prepare(bundle, Driver::Script(&[]), 0, |_| unreachable!()).unwrap_err();
        assert_eq!(error.to_string(), "native audit expected side 2 at roster position 2");
    }
}
