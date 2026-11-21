use super::{BoardCamera, EditableMap, History, new_map};
use std::{fs, io, path::{Path, PathBuf}};

pub(super) struct Document {
    pub map: EditableMap,
    pub history: History,
    pub camera: BoardCamera,
    pub path: Option<PathBuf>,
}

impl Document {
    pub fn can_revert(&self) -> bool { self.path.is_some() && self.map.dirty() }

    pub fn revert(&mut self) -> io::Result<()> {
        let path = self.path.as_ref().ok_or(io::ErrorKind::NotFound)?;
        let map = read_map(path)?;
        self.map = map;
        self.history = History::default();
        self.camera = BoardCamera::default();
        Ok(())
    }

    fn blank() -> Self {
        Self { map: new_map(), history: History::default(), camera: BoardCamera::default(), path: None }
    }

    pub fn name(&self) -> &str {
        self.path.as_deref().and_then(Path::file_stem).and_then(|s| s.to_str()).unwrap_or("Untitled")
    }
}

pub(super) struct Documents {
    pub list: Vec<Document>,
    pub current: usize,
}

fn read_map(path: &Path) -> io::Result<EditableMap> {
    let map = EditableMap::parse(fs::read_to_string(path)?).map_err(io::Error::other)?;
    if map.map().width() < 1 || map.map().height() < 1 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "Map contains no playable hexes"));
    }
    Ok(map)
}

impl Default for Documents {
    fn default() -> Self { Self { list: vec![Document::blank()], current: 0 } }
}

impl Documents {
    pub fn new_map(&mut self) {
        self.list.push(Document::blank());
        self.current = self.list.len() - 1;
    }

    pub fn open(&mut self, path: &Path) -> io::Result<()> {
        let path = fs::canonicalize(path)?;
        if let Some(index) = self.list.iter().position(|doc| doc.path.as_ref() == Some(&path)) {
            self.current = index;
            return Ok(());
        }
        let map = read_map(&path)?;
        self.list.push(Document { map, path: Some(path), history: History::default(), camera: BoardCamera::default() });
        self.current = self.list.len() - 1;
        Ok(())
    }

    pub fn close(&mut self) {
        self.list.remove(self.current);
        if self.list.is_empty() { self.list.push(Document::blank()); }
        self.current = self.current.min(self.list.len() - 1);
    }

    pub fn titles(&self) -> Vec<String> {
        self.list.iter().enumerate().map(|(i, doc)| format!("{}{}{}",
            if i == self.current { "✓ " } else { "" }, doc.name(), if doc.map.dirty() { " *" } else { "" })).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_lite::hex::Hex;

    #[test]
    fn revert_reads_the_named_map_again_and_preserves_changes_on_read_failure() {
        let path = std::env::temp_dir().join(format!("rustnoth-revert-{}.map", std::process::id()));
        let mut doc = Document::blank();
        let original = doc.map.text();
        assert!(!doc.can_revert());
        fs::write(&path, &original).unwrap();
        doc.path = Some(path.clone());
        doc.history.paint(&mut doc.map, Hex{x:0,y:0}, "Ww");
        doc.history.finish();
        assert!(doc.can_revert());
        fs::write(&path, "invalid,\n").unwrap();
        assert!(doc.revert().is_err());
        assert_eq!(doc.map.map().terrain_at(Hex{x:0,y:0}), Some("Ww"));
        fs::write(&path,original).unwrap();
        doc.revert().unwrap();
        assert_eq!(doc.map.map().terrain_at(Hex{x:0,y:0}), Some("Gg"));
        assert!(!doc.can_revert() && doc.history.undo.is_empty());
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn open_maps_keep_unsaved_cells_and_undo_history_when_switching_and_closing() {
        let mut docs = Documents::default();
        let hex = Hex { x: 2, y: 2 };
        let original = &mut docs.list[0];
        original.history.paint(&mut original.map, hex, "Ww");
        original.history.finish();
        docs.new_map();
        assert_eq!(docs.titles(), ["Untitled *", "✓ Untitled"]);
        assert_eq!(docs.list[1].map.map().terrain_at(hex), Some("Gg"));
        docs.close();
        let original = &mut docs.list[docs.current];
        assert_eq!(original.map.map().terrain_at(hex), Some("Ww"));
        assert!(original.history.apply(&mut original.map, true));
        assert_eq!(original.map.map().terrain_at(hex), Some("Gg"));
        docs.close();
        assert_eq!(docs.titles(), ["✓ Untitled"]);
    }
}
