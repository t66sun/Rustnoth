use super::{MapParseError, ScenarioMap, map_body, valid_terrain_code};
use crate::hex::Hex;
use std::{collections::BTreeMap, ops::Range};

/// Original terrain tokens and the identifiers immediately preceding them.
/// Unedited fields retain their original spacing when the document is saved.
pub struct EditableMap {
    source: String,
    map: ScenarioMap,
    tokens: Vec<Range<usize>>,
    edits: BTreeMap<usize, String>,
    original_locations: BTreeMap<String, Option<Hex>>,
}

/// Previous values of the moved identifier and any displaced identifier.
pub struct LocationEdit(Vec<(String, Option<Hex>)>);

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
            original_locations: BTreeMap::new(),
        })
    }

    pub fn map(&self) -> &ScenarioMap {
        &self.map
    }
    pub fn dirty(&self) -> bool {
        !self.edits.is_empty() || !self.original_locations.is_empty()
    }

    pub fn locations(&self) -> impl Iterator<Item = (&str, Hex)> {
        self.map.special_locations.iter().map(|(id, &at)| (id.as_str(), at))
    }

    pub fn location_at(&self, at: Hex) -> Option<&str> {
        self.locations().filter_map(|(id, pos)| (pos == at).then_some(id)).min()
    }

    fn replace_location(&mut self, id: &str, at: Option<Hex>) -> Option<Hex> {
        let old = match at {
            Some(hex) => self.map.special_locations.insert(id.to_owned(), hex),
            None => self.map.special_locations.remove(id),
        };
        if *self.original_locations.entry(id.to_owned()).or_insert(old) == at {
            self.original_locations.remove(id);
        }
        old
    }

    pub fn set_location(&mut self, id: &str, at: Option<Hex>) -> Option<LocationEdit> {
        if id.is_empty() || id.contains(|c: char| c.is_whitespace() || c == ',')
            || at.is_some_and(|p| !self.map.is_playable(p)) || self.map.special_location(id) == at {
            return None;
        }
        let mut undo = Vec::new();
        if let Some(displaced) = at.and_then(|p| self.location_at(p)).map(str::to_owned) {
            let previous = self.replace_location(&displaced, None);
            undo.push((displaced, previous));
        }
        let previous = self.replace_location(id, at);
        undo.push((id.to_owned(), previous));
        Some(LocationEdit(undo))
    }

    pub fn restore_locations(&mut self, edit: LocationEdit) -> LocationEdit {
        LocationEdit(edit.0.into_iter().map(|(id, previous)| {
            let inverse = self.replace_location(&id, previous);
            (id, inverse)
        }).collect())
    }

    pub fn paint(&mut self, at: Hex, terrain: &str) -> Option<String> {
        if !self.map.is_playable(at) {
            return None;
        }
        self.paint_stored(at, terrain)
    }

    /// Editor region operations include the authored one-hex border.
    pub fn paint_stored(&mut self, at: Hex, terrain: &str) -> Option<String> {
        if !valid_terrain_code(terrain)
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
        self.map.terrain[index] = terrain.into();
        Some(previous)
    }

    pub fn text(&self) -> String {
        let mut output = String::with_capacity(self.source.len());
        let mut cursor = 0;
        let mut added = BTreeMap::<usize, Vec<&str>>::new();
        for id in self.original_locations.keys() {
            if let Some(at) = self.map.special_location(id) {
                let index = ((at.y + 1) * self.map.columns + at.x + 1) as usize;
                added.entry(index).or_default().push(id);
            }
        }
        for (index, token) in self.tokens.iter().enumerate() {
            let prefix = &self.source[cursor..token.start];
            let start = prefix.rfind([',', '\r', '\n']).map_or(0, |i| i + 1);
            let fields = prefix[start..].trim_start();
            if fields.split_whitespace().any(|s| self.original_locations.contains_key(s)) {
                output.push_str(&prefix[..prefix.len() - fields.len()]);
                for part in fields.split_inclusive(' ') {
                    if !self.original_locations.contains_key(part.trim_end()) {
                        output.push_str(part);
                    }
                }
            } else {
                output.push_str(prefix);
            }
            if let Some(ids) = added.get(&index) {
                for id in ids { output.push_str(id); output.push(' '); }
            }
            output.push_str(self.edits.get(&index).map_or(&self.source[token.clone()], String::as_str));
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

    #[test]
    fn moving_a_location_preserves_other_aliases_paint_and_header() {
        let original = "usage=map\r\n\r\nGg,Gg,Gg,Gg\r\nGg, start   altar Hh, Gg ,Gg\r\nGg,Gg,Gg,Gg\r\n";
        let mut doc = EditableMap::parse(original.into()).unwrap();
        let a = Hex { x: 0, y: 0 };
        let b = Hex { x: 1, y: 0 };
        doc.paint(a, "Ww");
        assert!(doc.set_location("start", Some(Hex { x: -1, y: 0 })).is_none());
        let undo = doc.set_location("start", Some(b)).unwrap();
        let serialized = doc.text();
        assert!(serialized.starts_with("usage=map\r\n\r\n"));
        let next = EditableMap::parse(serialized).unwrap();
        assert_eq!(next.map().special_location("altar"), Some(a));
        assert_eq!(next.map().special_location("start"), Some(b));
        assert_eq!(next.map().terrain_at(a), Some("Ww"));
        doc.restore_locations(undo);
        assert_eq!(doc.text(), original.replace("Hh", "Ww"));
    }
}
