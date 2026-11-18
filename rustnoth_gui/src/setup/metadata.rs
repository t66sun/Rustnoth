use engine_lite::wml;
use std::{collections::HashMap, fs, path::Path};

#[derive(Default)]
pub(super) struct Description {
    pub title: String,
    pub body: String,
}

pub(super) fn read_all(root: &Path) -> HashMap<String, Description> {
    let mut maps = HashMap::new();
    let Ok(files) = fs::read_dir(root.join("data/multiplayer/scenarios")) else {
        return maps;
    };
    for file in files.flatten() {
        let path = file.path();
        if path.extension().is_none_or(|e| e != "cfg") {
            continue;
        }
        let Ok(source) = fs::read_to_string(&path) else {
            continue;
        };
        if let Some((key, meta)) = read(&source) {
            maps.entry(key).or_insert(meta);
        }
    }
    maps
}

fn read(source: &str) -> Option<(String, Description)> {
    let (_, body) = source.split_once("[multiplayer]")?;
    let mut lines = body.lines();
    let mut attrs = String::new();
    while let Some(line) = lines.next() {
        let line = line.trim();
        if line.starts_with('[') {
            break;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if !matches!(key.trim(), "name" | "description" | "map_file") {
            continue;
        }
        attrs.push_str(line);
        attrs.push('\n');
        if value.bytes().filter(|c| *c == b'"').count() % 2 == 1 {
            for line in lines.by_ref() {
                attrs.push_str(line);
                attrs.push('\n');
                if line.bytes().filter(|c| *c == b'"').count() % 2 == 1 {
                    break;
                }
            }
        }
    }
    let node = wml::parse(attrs).ok()?;
    let key = Path::new(node.attribute("map_file")?)
        .file_stem()?
        .to_str()?
        .replace('_', " ");
    let title = node.attribute("name")?.into();
    let body = node.attribute("description").unwrap_or("").to_owned();
    Some((key, Description { title, body }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scenario_card_uses_the_map_file_and_preserves_its_original_multiline_description() {
        let (key, entry)=read("[multiplayer]\nname= _ \"2p — Hamlets\"\ndescription= _ \"Fields.\nVillages.\"\n{DEFAULT_SCHEDULE}\nmap_file=multiplayer/maps/2p_Hamlets.map\n[side]\nname=West\n[/side]\n[/multiplayer]").unwrap();
        assert_eq!(key, "2p Hamlets");
        assert_eq!(entry.title, "2p — Hamlets");
        assert_eq!(entry.body, "Fields.\nVillages.");
    }
}
