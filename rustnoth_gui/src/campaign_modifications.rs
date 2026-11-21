//! Discover installed single-player modifications and expand their WML into each scenario.
use engine_lite::{campaign, wml::{self, Node}};
use rustnoth_gui::save_replay::set;
use std::{fs, io, path::{Path, PathBuf}};

pub(super) fn eligible(node: &Node) -> bool {
    node.attribute("id").is_some_and(|id| !id.is_empty())
        && matches!(node.attribute("type"), None | Some("sp" | "hybrid" | ""))
}

// [modification] can be in a file included by the add-on's _main.cfg.
fn config_files(dir: &Path, files: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let path = entry.path();
        if kind.is_dir() {
            config_files(&path, files)?;
        } else if kind.is_file() && path.extension().is_some_and(|ext| ext == "cfg") {
            files.push(path);
        }
    }
    Ok(())
}

pub(crate) fn load_installed(catalog: &mut Node, root: &Path, userdata: &Path) -> io::Result<()> {
    let addons = userdata.join("data/add-ons");
    if !addons.is_dir() { return Ok(()); }
    let mut packages = fs::read_dir(addons)?.collect::<io::Result<Vec<_>>>()?;
    packages.sort_by_key(|entry| entry.file_name());
    for package in packages {
        let dir = package.path();
        let entry = dir.join("_main.cfg");
        if !entry.is_file() { continue; }
        let mut files = Vec::new();
        config_files(&dir, &mut files)?;
        if !files.iter().any(|path| fs::read_to_string(path).is_ok_and(|s|
            s.lines().any(|line| line.trim_start().starts_with("[modification]")))) { continue; }
        files.sort();
        let prepared = crate::desktop::prepare_addon(root, userdata, &entry, &files)
            .and_then(|bytes| wml::parse(bytes).map_err(io::Error::other));
        match prepared {
            Ok(document) => catalog.children.extend(document.children.into_iter()
                .filter(|(tag, node)| tag == "modification" && eligible(node))),
            Err(_) => {
                let mut row = Node::default();
                let name = package.file_name();
                set(&mut row, "id", name.to_string_lossy());
                set(&mut row, "name", name.to_string_lossy());
                set(&mut row, "rustnoth_load_error", "yes");
                catalog.children.push(("modification".into(), row));
            }
        }
    }
    Ok(())
}

fn contains_lua(node: &Node) -> bool {
    node.children.iter().any(|(tag, child)| tag == "lua" || contains_lua(child))
}

fn option_default<'a>(kind: &str, option: &'a Node) -> Option<(&'a str, &'a str)> {
    let id = option.attribute("id")?;
    if !engine_lite::campaign::accepts_option_id(id) { return None; }
    let value = match kind {
        "checkbox" => match option.attribute("default").unwrap_or("no") {
            "yes" | "true" | "1" => "yes",
            "no" | "false" | "0" => "no",
            _ => return None,
        },
        "entry" => option.attribute("default").unwrap_or(""),
        "slider" => option.attribute("default").or_else(|| option.attribute("min")).unwrap_or("0"),
        "choice" => option.attribute("default").or_else(|| option.children.iter()
            .find(|(tag, _)| tag == "item").and_then(|(_, item)| item.attribute("value")))?,
        _ => return None,
    };
    Some((id, value))
}

pub(super) fn unsupported(node: &Node) -> Option<&'static str> {
    if node.attribute("rustnoth_load_error").is_some() { return Some("Could not read add-on WML"); }
    if contains_lua(node) { return Some("Requires Lua"); }
    if node.attribute("define").is_some_and(|s| !s.is_empty()) { return Some("Requires a WML define"); }
    if ["require_modification", "disallow_modification", "allow_modification", "allow_scenario",
        "disallow_scenario", "allow_era", "disallow_era"].iter().any(|key| node.attribute(key).is_some_and(|s| !s.is_empty())) {
        return Some("Requires dependency resolution");
    }
    for (tag, child) in &node.children {
        match tag.as_str() {
            "event" => (),
            "options" if child.children.iter().all(|(kind, option)| kind == "spacer" || option_default(kind, option).is_some()) => (),
            "options" => return Some("Requires unsupported options"),
            _ => return Some("Requires unsupported WML"),
        }
    }
    None
}

pub(crate) fn apply(scenario: &mut Node, modifications: &[Node]) -> io::Result<()> {
    for node in modifications {
        if let Some(reason) = unsupported(node) { return Err(io::Error::other(reason)); }
        campaign::apply_modification(scenario, node)?;
        for (_, options) in node.children.iter().filter(|(tag, _)| tag == "options") {
            for (kind, option) in options.children.iter().filter(|(tag, _)| tag != "spacer") {
                let (id, value) = option_default(kind, option).expect("validated above");
                let index = scenario.children.iter().position(|(tag, _)| tag == "variables").unwrap_or_else(|| {
                    scenario.children.push(("variables".into(), Node::default()));
                    scenario.children.len() - 1
                });
                set(&mut scenario.children[index].1, id, value);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_scenario_receives_enabled_events_and_option_variables() {
        let document = wml::parse("[modification]\nid=bonus\n[event]\nname=prestart\n[gold]\nside=1\namount=3\n[/gold]\n[/event]\n[options]\n[checkbox]\nid=bonus_enabled\ndefault=yes\n[/checkbox]\n[/options]\n[/modification]").unwrap();
        let modification = document.children[0].1.clone();
        for id in ["first", "second"] {
            let mut scenario = Node::default();
            set(&mut scenario, "id", id);
            scenario.children.push(("side".into(), Node::default()));
            apply(&mut scenario, std::slice::from_ref(&modification)).unwrap();
            assert_eq!(scenario.children[1].1.attribute("name"), Some("prestart"));
            assert_eq!(scenario.children[2].1.attribute("bonus_enabled"), Some("yes"));
        }
    }

    #[test]
    fn hybrid_is_the_native_default_and_scripted_modifications_cannot_be_enabled() {
        let mut modification = Node::default();
        set(&mut modification, "id", "example");
        assert!(eligible(&modification));
        set(&mut modification, "type", "mp");
        assert!(!eligible(&modification));
        let mut event = Node::default();
        event.children.push(("lua".into(), Node::default()));
        modification.children.push(("event".into(), event));
        assert_eq!(unsupported(&modification), Some("Requires Lua"));
        assert!(apply(&mut Node::default(), &[modification]).is_err());
    }
}
