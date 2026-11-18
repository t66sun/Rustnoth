use std::{env, error::Error, ffi::OsString, fs, io, path::Path, process::ExitCode};

use engine_lite::{scenario_map, wml::{self, Node, Value}, wml_emit};

const USAGE: &str = "rustnoth-campaign --list <preprocessed-campaign.cfg>\n\
rustnoth-campaign <preprocessed-campaign.cfg> <scenario-id> <campaign-maps-directory>\n\
Writes the selected scenario and its campaign definitions to standard output.";

fn select(mut campaign: Node, id: &str, read_map: impl FnOnce(&str) -> io::Result<String>) -> Result<Node, Box<dyn Error>> {
    campaign.children.retain(|(tag, node)| tag != "campaign"
        && (tag != "scenario" || node.attribute("id") == Some(id)));
    let mut selected = campaign.children.iter_mut().filter(|(tag, _)| tag == "scenario");
    let (_, scenario) = selected.next().ok_or_else(|| format!("unknown scenario {id}"))?;
    if selected.next().is_some() {
        return Err(format!("multiple scenarios have id {id}").into());
    }
    let map = scenario_map::read_data(scenario, read_map)?.into_owned();
    scenario.attributes.remove("map_file");
    scenario.attributes.insert("map_data".to_owned(), Value { text: map, translations: Vec::new() });
    Ok(campaign)
}

fn run(args: &[OsString]) -> Result<(), Box<dyn Error>> {
    match args {
        [flag] if flag == "--help" || flag == "-h" => println!("{USAGE}"),
        [flag, input] if flag == "--list" => {
            let campaign = wml::parse(fs::read(input)?)?;
            for (_, node) in campaign.children.iter().filter(|(tag, _)| tag == "scenario") {
                if let Some(id) = node.attribute("id") {
                    println!("{id}\t{}", node.attribute("name").unwrap_or_default());
                }
            }
        }
        [input, id, maps] => {
            let id = id.to_str().ok_or("scenario id must be UTF-8")?;
            let campaign = wml::parse(fs::read(input)?)?;
            let bundle = select(campaign, id, |file| fs::read_to_string(Path::new(maps).join(file)))?;
            wml_emit::write_config(io::stdout().lock(), &bundle)?;
        }
        _ => return Err(USAGE.into()),
    }
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<_> = env::args_os().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => { eprintln!("{error}"); ExitCode::FAILURE }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embeds_the_selected_map_and_keeps_its_events_and_custom_types() {
        let campaign = wml::parse("[units]\n[unit_type]\nid=Lord\n[/unit_type]\n[/units]\n\
            [scenario]\nid=intro\nmap_file=intro.map\n[/scenario]\n\
            [scenario]\nid=battle\nmap_file=battle.map\n[event]\nname=start\n[/event]\n[/scenario]").unwrap();
        assert!(select(campaign.clone(), "missing", |_| unreachable!()).is_err());
        let bundle = select(campaign.clone(), "battle", |file| {
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
