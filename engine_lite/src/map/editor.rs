use super::{MapParseError, ScenarioMap, map_body, valid_terrain_code};
use crate::hex::Hex;
use std::{collections::BTreeMap, ops::Range};

/// Editable terrain and the original text. Only the terrain token of a painted
/// hex changes on save; named locations, start positions and map borders survive.
pub struct EditableMap {
    source: String,
    map: ScenarioMap,
    tokens: Vec<Range<usize>>,
    edits: BTreeMap<usize, String>,
}

impl EditableMap {
    pub fn parse(source: String) -> Result<Self, MapParseError> {
        let map = ScenarioMap::parse(&source)?;
        let body = map_body(&source);
        let mut tokens = Vec::new();
        for row in body.split(['\r', '\n']).filter(|line| !line.is_empty()) {
            for cell in row.split(',') {
                let code = cell.trim().rsplit(' ').next().unwrap_or_default();
                let start = code.as_ptr() as usize - source.as_ptr() as usize;
                tokens.push(start..start + code.len());
            }
        }
        Ok(Self {
            source,
            map,
            tokens,
            edits: BTreeMap::new(),
        })
    }

    pub fn map(&self) -> &ScenarioMap {
        &self.map
    }
    pub fn dirty(&self) -> bool {
        !self.edits.is_empty()
    }

    pub fn paint(&mut self, at: Hex, terrain: &str) -> Option<String> {
        if !self.map.is_playable(at)
            || !valid_terrain_code(terrain)
            || terrain.contains(|c: char| c.is_whitespace() || c == ',')
        {
            return None;
        }
        let previous = self.map.terrain_at(at)?;
        if previous == terrain {
            return None;
        }
        let previous = previous.to_owned();
        let index = ((at.y + 1) * self.map.columns + at.x + 1) as usize;
        if self.source[self.tokens[index].clone()] == *terrain {
            self.edits.remove(&index);
        } else {
            self.edits.insert(index, terrain.into());
        }
        self.map.replace_terrain(at, terrain);
        Some(previous)
    }

    pub fn text(&self) -> String {
        let mut output = String::with_capacity(self.source.len());
        let mut cursor = 0;
        for (&index, terrain) in &self.edits {
            let token = &self.tokens[index];
            output.push_str(&self.source[cursor..token.start]);
            output.push_str(terrain);
            cursor = token.end;
        }
        output.push_str(&self.source[cursor..]);
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn painting_preserves_legacy_header_named_starts_border_and_crlf() {
        let original = "usage=map\r\nborder_size=1\r\n\r\nGg,  Ww, Gg\r\nGg,1 castle Gg^Fds ,Mm\r\nSs, Gg, Gg\r\n";
        let mut doc = EditableMap::parse(original.into()).unwrap();
        let tile = Hex { x: 0, y: 0 };
        assert_eq!(doc.paint(tile, "Ke").as_deref(), Some("Gg^Fds"));
        assert!(doc.paint(Hex { x: -1, y: 0 }, "Ke").is_none());
        assert_eq!(doc.map().special_location("castle"), Some(tile));
        assert_eq!(doc.text(), original.replace("castle Gg^Fds", "castle Ke"));
        assert!(doc.dirty());
        doc.paint(tile, "Gg^Fds");
        assert!(!doc.dirty());
        assert_eq!(doc.text(), original);
    }
}
