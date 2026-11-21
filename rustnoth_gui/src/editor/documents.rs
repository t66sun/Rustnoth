use super::{BoardCamera, EditableMap, History, new_map};
use std::{fs, io, path::{Path, PathBuf}};

pub(super) struct Document {
    pub map: EditableMap,
    pub locations: super::locations::Locations,
    pub history: History,
    pub camera: BoardCamera,
    pub path: Option<PathBuf>,
}

impl Document {
    fn blank() -> Self {
        Self::new(new_map(), None)
    }

    fn new(map: EditableMap, path: Option<PathBuf>) -> Self {
        Self { locations: super::locations::Locations::new(&map), map,
            history: History::default(), camera: BoardCamera::default(), path }
    }

    pub fn name(&self) -> &str {
        self.path.as_deref().and_then(Path::file_stem).and_then(|s| s.to_str()).unwrap_or("Untitled")
    }
}

pub(super) struct Documents {
    pub list: Vec<Document>,
    pub current: usize,
}

impl Default for Documents {
    fn default() -> Self { Self { list: vec![Document::blank()], current: 0 } }
}

impl Documents {
    pub fn new_map(&mut self, map: EditableMap) {
        self.list.push(Document::new(map, None));
        self.current = self.list.len() - 1;
    }

    pub fn open(&mut self, path: &Path) -> io::Result<()> {
        let path = fs::canonicalize(path)?;
        if let Some(index) = self.list.iter().position(|doc| doc.path.as_ref() == Some(&path)) {
            self.current = index;
            return Ok(());
        }
        let map = EditableMap::parse(fs::read_to_string(&path)?).map_err(io::Error::other)?;
        if map.map().width() < 1 || map.map().height() < 1 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Map contains no playable hexes"));
        }
        self.list.push(Document::new(map, Some(path)));
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
    fn open_maps_keep_unsaved_cells_and_undo_history_when_switching_and_closing() {
        let mut docs = Documents::default();
        let hex = Hex { x: 2, y: 2 };
        let original = &mut docs.list[0];
        original.history.paint(&mut original.map, hex, "Ww");
        original.history.finish();
        docs.new_map(new_map());
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
