//! Select playable scenarios from preprocessed Wesnoth campaign definitions.

use std::{error::Error, io};

use crate::{scenario_map, wml::{Node, Value}};

/// Option values are stored in scalar scenario variables.
pub fn accepts_option_id(id: &str) -> bool { crate::scenario_events::variables::scalar_name(id) }

/// Append preprocessed single-player modification events in selection order.
pub fn apply_modification(scenario: &mut Node, modification: &Node) -> Result<(), io::Error> {
    let sides = scenario.children.iter().filter(|(tag, _)| tag == "side").count();
    for (tag, event) in &modification.children {
        if tag == "event" && !crate::scenario_events::ScenarioEvents::accepts_event(scenario, sides, event) {
            return Err(io::Error::new(io::ErrorKind::Unsupported, format!("{}: unsupported event '{}'",
                modification.attribute("name").unwrap_or("Modification"), event.attribute("name").unwrap_or(""))));
        }
    }
    scenario.children.extend(modification.children.iter().filter(|(tag, _)| tag == "event").cloned());
    Ok(())
}

/// Scenario IDs and display names in the order of the original campaign file.
pub fn scenarios(campaign: &Node) -> impl Iterator<Item = (&str, &str)> {
    campaign.children.iter().filter(|(tag, _)| tag == "scenario").filter_map(|(_, node)| {
        Some((node.attribute("id")?, node.attribute("name").unwrap_or_default()))
    })
}

/// Keep the selected scenario and shared definitions, embedding its map for launch or export.
pub fn select(campaign: &Node, id: &str, read_map: impl FnOnce(&str) -> io::Result<String>) -> Result<Node, Box<dyn Error>> {
    let mut document = Node {
        attributes: campaign.attributes.clone(),
        children: campaign.children.iter().filter(|(tag, node)| tag != "campaign"
            && (tag != "scenario" || node.attribute("id") == Some(id))).cloned().collect(),
    };
    let mut selected = document.children.iter_mut().filter(|(tag, _)| tag == "scenario");
    let (_, scenario) = selected.next().ok_or_else(|| format!("unknown scenario {id}"))?;
    if selected.next().is_some() {
        return Err(format!("multiple scenarios have id {id}").into());
    }
    let map = scenario_map::read_data(scenario, read_map)?.into_owned();
    scenario.attributes.remove("map_file");
    scenario.attributes.insert("map_data".to_owned(), Value { text: map, translations: Vec::new() });
    Ok(document)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wml;

    #[test]
    fn embeds_the_selected_map_and_keeps_its_events_and_custom_types() {
        let campaign = wml::parse("[units]\n[unit_type]\nid=Lord\n[/unit_type]\n[/units]\n\
            [scenario]\nid=intro\nmap_file=intro.map\n[/scenario]\n\
            [scenario]\nid=battle\nmap_file=battle.map\n[event]\nname=start\n[/event]\n[/scenario]").unwrap();
        assert_eq!(scenarios(&campaign).collect::<Vec<_>>(), [("intro", ""), ("battle", "")]);
        assert!(select(&campaign, "missing", |_| unreachable!()).is_err());
        let bundle = select(&campaign, "battle", |file| {
            assert_eq!(file, "battle.map");
            Ok("Gg,Kh".into())
        }).unwrap();
        assert_eq!(bundle.children.len(), 2);
        assert_eq!(bundle.children[0], campaign.children[0]);
        let scenario = &bundle.children[1].1;
        assert_eq!(scenario.attribute("map_data"), Some("Gg,Kh"));
        assert_eq!(scenario.attribute("map_file"), None);
        assert_eq!(scenario.children[0].0, "event");
    }
}
