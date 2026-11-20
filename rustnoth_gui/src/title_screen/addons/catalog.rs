use engine_lite::wml::{self, Node};
use std::{fs, io, path::{Component, Path, PathBuf}};

pub(super) struct Addon {
    pub folder: PathBuf,
    pub title: String,
    pub version: String,
    pub author: String,
    pub description: String,
    pub icon: String,
}

fn read_metadata(path: &Path) -> io::Result<Option<Node>> {
    match fs::read(path) {
        Ok(data) => Ok(wml::parse(data).ok()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

fn translation<'a>(info: &'a Node, language: &str) -> Option<&'a Node> {
    let short = language.split('_').next().unwrap_or(language);
    [language, short].into_iter().find_map(|lang| info.children.iter()
        .find(|(tag, node)| tag == "translation" && node.attribute("language") == Some(lang))
        .map(|(_, node)| node))
}

pub(super) fn installed(directory: &Path, language: &str) -> io::Result<Vec<Addon>> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut result = Vec::new();
    for entry in entries {
        let entry = entry?;
        let folder = entry.path();
        if !folder.join("_main.cfg").is_file() { continue; }
        let id = entry.file_name().to_string_lossy().into_owned();
        let external = directory.join(format!("{id}.pbl"));
        let publishing = read_metadata(&if external.is_file() { external } else { folder.join("_server.pbl") })?;
        let tracking = read_metadata(&folder.join("_info.cfg"))?;
        let tracking = tracking.as_ref().and_then(|root| root.children.iter()
            .find(|(tag, _)| tag == "info").map(|(_, node)| node));
        let localized = publishing.as_ref().and_then(|pbl| translation(pbl, language));
        let field = |key: &str| [localized, publishing.as_ref(), tracking].into_iter()
            .flatten().find_map(|node| node.attribute(key).filter(|s| !s.is_empty()));
        let title = field("title").map_or_else(|| id.replace('_', " "), str::to_owned);
        let mut icon = field("icon").unwrap_or_default().to_owned();
        if icon.contains("units/") && !icon.contains('~') { icon.push_str("~RC(magenta>red)"); }
        result.push(Addon { folder, title, icon, version: field("version").unwrap_or_default().into(),
            author: field("author").unwrap_or_default().into(), description: field("description").unwrap_or_default().into() });
    }
    result.sort_by_cached_key(|a| (a.title.to_lowercase(), a.folder.clone()));
    Ok(result)
}

pub(super) fn icon_file(root: &Path, folder: &Path, name: &Path) -> Option<PathBuf> {
    if name.as_os_str().is_empty() || !name.components().all(|part| matches!(part, Component::Normal(_))) { return None; }
    if let Ok(local) = name.strip_prefix("data/add-ons") {
        return Some(folder.parent()?.join(local));
    }
    [folder.join("images").join(name), root.join("data/core/images").join(name), root.join("images").join(name)]
        .into_iter().find(|path| path.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installed_addons_use_publish_and_tracking_metadata_and_ignore_non_addon_folders() {
        let dir = std::env::temp_dir().join(format!("rustnoth-addons-{}", std::process::id()));
        let addon = dir.join("First_Era");
        let downloaded = dir.join("Downloaded");
        fs::create_dir_all(&addon).unwrap();
        fs::create_dir_all(&downloaded).unwrap();
        fs::create_dir_all(dir.join("Incomplete")).unwrap();
        fs::write(addon.join("_main.cfg"), "").unwrap();
        fs::write(downloaded.join("_main.cfg"), "").unwrap();
        fs::write(downloaded.join("_info.cfg"), "[info]\ntitle=Downloaded era\nversion=1.2\n[/info]").unwrap();
        fs::write(addon.join("_server.pbl"), "title=Interior").unwrap();
        fs::write(dir.join("First_Era.pbl"), "title=Exterior\ndescription=Local description\nicon=units/foo.png\n[translation]\nlanguage=fr\ntitle=Bonjour\n[/translation]").unwrap();
        let rows = installed(&dir, "fr_CA").unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!((&*rows[0].title, &*rows[0].description, &*rows[0].icon), ("Bonjour", "Local description", "units/foo.png~RC(magenta>red)"));
        assert_eq!((&*rows[1].title, &*rows[1].version), ("Downloaded era", "1.2"));
        assert_eq!(icon_file(Path::new("."), &addon, Path::new("data/add-ons/First_Era/images/a.png")), Some(addon.join("images/a.png")));
        fs::remove_dir_all(dir).unwrap();
    }
}
