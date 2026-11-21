//! Apply the stock tutorial's [victory] progress to the Rustnoth user profile.
use engine_lite::wml::{self, Node};
use std::{fs::{self, File, OpenOptions}, io::{self, Read, Seek, Write}, path::Path};

const PROGRESS_FILE: &str = "tutorial-completed-progress";

fn children<'a>(node: &'a Node, tag: &'a str) -> impl Iterator<Item = &'a Node> {
    node.children.iter().filter_map(move |(name, child)| (name == tag).then_some(child))
}

#[derive(Clone, Copy)]
pub(crate) struct Award {
    amount: u32,
    limit: u32,
}

impl Award {
    pub(crate) fn from_victory(scenario: &Node) -> Option<Self> {
        let cmd = children(scenario, "event").filter(|e| e.attribute("name") == Some("victory"))
            .flat_map(|e| children(e, "progress_achievement"))
            .find(|a| a.attribute("content_for") == Some("tutorial") && a.attribute("id") == Some("completed"))?;
        Some(Self {
            amount: cmd.attribute("amount")?.parse().ok()?,
            limit: cmd.attribute("limit").and_then(|s| s.parse().ok()).unwrap_or(u32::MAX),
        })
    }

    pub(crate) fn record(self, root: &Path, directory: &Path) -> io::Result<()> {
        let path = root.join(crate::desktop::TUTORIAL).join("achievements.cfg");
        let config = wml::parse(fs::read(path)?).map_err(io::Error::other)?;
        let maximum = children(&config, "achievement_group").filter(|g| g.attribute("content_for") == Some("tutorial"))
            .flat_map(|g| children(g, "achievement")).find(|a| a.attribute("id") == Some("completed"))
            .and_then(|a| a.attribute("max_progress")).and_then(|value| value.parse().ok())
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Missing tutorial achievement maximum"))?;
        let native = super::catalog::read_profile()?;
        self.update(&native, maximum, directory)
    }

    fn update(self, native: &Node, maximum: u32, directory: &Path) -> io::Result<()> {
        let group = children(native, "achievements").find(|g| g.attribute("content_for") == Some("tutorial"));
        if group.and_then(|g| g.attribute("ids")).unwrap_or("").split(',').any(|id| id.trim() == "completed") {
            return Ok(());
        }
        let imported = group.and_then(|g| children(g, "in_progress").find(|a| a.attribute("id") == Some("completed")))
            .and_then(|a| a.attribute("progress_at")).and_then(|v| v.parse().ok()).unwrap_or(0);
        fs::create_dir_all(directory)?;
        let mut file = OpenOptions::new().read(true).write(true).create(true).truncate(false)
            .open(directory.join(PROGRESS_FILE))?;
        file.lock()?;
        let current = read_count(&mut file)?.max(imported);
        let next = current.saturating_add(self.amount).min(maximum).min(self.limit);
        if next > current {
            file.rewind()?;
            write!(file, "{next}")?;
        }
        Ok(())
    }
}

fn read_count(file: &mut File) -> io::Result<u32> {
    let mut value = String::new();
    file.read_to_string(&mut value)?;
    if value.is_empty() { return Ok(0); }
    value.parse().map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

pub(crate) fn read(directory: &Path) -> io::Result<u32> {
    let mut file = match File::open(directory.join(PROGRESS_FILE)) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(e),
    };
    file.lock_shared()?;
    read_count(&mut file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn tutorial_wins_update_the_local_catalog_with_the_authored_cap() {
        let directory = std::env::temp_dir().join(format!("rustnoth-tutorial-award-{}-{}", std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
        let native = Node::default();
        let config = wml::parse("[achievement_group]\ncontent_for=tutorial\n[achievement]\nid=completed\nmax_progress=2\n[/achievement]\n[/achievement_group]").unwrap();
        let row = || super::super::catalog::groups(&config, &native, read(&directory).unwrap(), &directory, "C")
            .unwrap().remove(0).rows.remove(0);
        let event = |name: &str, limit: &str| wml::parse(format!("[event]\nname={name}\n[progress_achievement]\ncontent_for=tutorial\nid=completed\namount=1\n{limit}\n[/progress_achievement]\n[/event]")).unwrap();
        assert!(Award::from_victory(&event("defeat", "")).is_none());
        let first = Award::from_victory(&event("victory", "limit=1")).unwrap();
        let second = Award::from_victory(&event("victory", "")).unwrap();
        assert_eq!(row().progress, Some((0, 2)));
        first.update(&native, 2, &directory).unwrap();
        first.update(&native, 2, &directory).unwrap();
        assert_eq!(row().progress, Some((1, 2)));
        second.update(&native, 2, &directory).unwrap();
        first.update(&native, 2, &directory).unwrap();
        second.update(&native, 2, &directory).unwrap();
        assert!(row().completed);
        assert_eq!(read(&directory).unwrap(), 2);

        let imported = wml::parse("[achievements]\ncontent_for=tutorial\n[in_progress]\nid=completed\nprogress_at=1\n[/in_progress]\n[/achievements]").unwrap();
        let other = directory.join("imported");
        second.update(&imported, 2, &other).unwrap();
        assert_eq!(read(&other).unwrap(), 2);
        fs::remove_dir_all(directory).unwrap();
    }
}
