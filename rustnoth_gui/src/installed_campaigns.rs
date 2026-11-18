//! Single-player campaign descriptors listed by the original title-screen configuration.
use engine_lite::wml::{Node, Value};
use std::{collections::HashMap, error::Error, fs, path::Path};

#[derive(Clone)]
pub(super) struct Campaign {
    pub folder: String,
    pub define: String,
    pub difficulty: String,
}

fn folders_by_define(directory: &Path) -> std::io::Result<HashMap<String, String>> {
    let mut folders = HashMap::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path().join("_main.cfg");
        if !path.is_file() {
            continue;
        }
        // The compiled campaign ID is not necessarily its installation directory
        // (for example LOW is installed under Legend_of_Wesmere).
        let source = fs::read_to_string(path)?;
        let Some((_, body)) = source.split_once("[campaign]") else {
            continue;
        };
        let Some((body, _)) = body.split_once("[/campaign]") else {
            continue;
        };
        let key = body
            .lines()
            .filter_map(|line| line.split_once('='))
            .find(|(key, _)| key.trim() == "define")
            .map(|(_, v)| v.trim().trim_matches('"'));
        if let Some(key) = key {
            folders.insert(
                key.to_owned(),
                entry.file_name().to_string_lossy().into_owned(),
            );
        }
    }
    Ok(folders)
}

fn default_difficulty(cfg: &Node) -> String {
    let mut choices = cfg
        .children
        .iter()
        .filter(|(tag, _)| tag == "difficulty")
        .map(|(_, n)| n);
    let first = choices.clone().next();
    choices
        .find(|n| matches!(n.attribute("default"), Some("yes" | "true" | "1")))
        .or(first)
        .and_then(|n| n.attribute("define"))
        .unwrap_or("NORMAL")
        .to_owned()
}

pub(super) fn catalog(
    config: Node,
    directory: &Path,
) -> Result<(Node, HashMap<String, Campaign>), Box<dyn Error>> {
    let folders = folders_by_define(directory)?;
    let mut descriptors: Vec<_> = config
        .children
        .into_iter()
        .filter(|(tag, cfg)| tag == "campaign" && cfg.attribute("type") != Some("mp"))
        .map(|(_, n)| n)
        .collect();
    descriptors.sort_by_key(|n| {
        n.attribute("rank")
            .and_then(|v| v.parse::<i32>().ok())
            .unwrap_or(1000)
    });
    let mut installed = HashMap::new();
    let mut result = Node::default();
    let mut tutorial = None;
    for cfg in descriptors {
        let (Some(first), Some(define)) =
            (cfg.attribute("first_scenario"), cfg.attribute("define"))
        else {
            continue;
        };
        let Some(folder) = folders.get(define) else {
            continue;
        };
        let entry = Campaign {
            folder: folder.clone(),
            define: define.into(),
            difficulty: default_difficulty(&cfg),
        };
        installed.insert(first.to_owned(), entry);
        if first == crate::tutorial::FIRST_SCENARIO {
            tutorial = Some(directory.join(folder).join("_main.cfg"));
        } else {
            let mut level = Node::default();
            level.attributes.insert(
                "id".into(),
                Value {
                    text: first.into(),
                    ..Default::default()
                },
            );
            level.attributes.insert(
                "name".into(),
                cfg.attributes.get("name").cloned().unwrap_or_default(),
            );
            result.children.push(("campaign".into(), cfg));
            result.children.push(("scenario".into(), level));
        }
    }
    if let Some(file) = tutorial {
        crate::tutorial::insert_entry(&mut result, &fs::read_to_string(file)?)?;
    }
    if installed.is_empty() {
        return Err("No installed single-player campaigns were found".into());
    }
    Ok((result, installed))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn installed_campaigns_follow_rank_and_use_the_actual_directory_and_default_define() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("rustnoth-catalog-{}-{nonce}", std::process::id()));
        let mut source = String::new();
        for (id, rank, folder) in [("LOW", 55, "Legend_of_Wesmere"), ("Liberty", 4, "Liberty")] {
            let cfg = format!(
                "[campaign]\nid={id}\ndefine=CAMPAIGN_{id}\nfirst_scenario={id}_first\nrank={rank}\n[difficulty]\ndefine=EASY\n[/difficulty]\n[difficulty]\ndefine=NORMAL\ndefault=yes\n[/difficulty]\n[/campaign]\n"
            );
            fs::create_dir_all(root.join(folder)).unwrap();
            fs::write(root.join(folder).join("_main.cfg"), &cfg).unwrap();
            source.push_str(&cfg);
        }
        let (list, by_first) = catalog(engine_lite::wml::parse(source).unwrap(), &root).unwrap();
        assert_eq!(list.children[0].1.attribute("id"), Some("Liberty"));
        assert_eq!(by_first["LOW_first"].folder, "Legend_of_Wesmere");
        assert_eq!(by_first["LOW_first"].difficulty, "NORMAL");
        fs::remove_dir_all(root).unwrap();
    }
}
