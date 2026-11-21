use super::{MapParseError, ScenarioMap, map_body, valid_terrain_code};
use crate::hex::Hex;
use std::{collections::BTreeMap, ops::Range};

/// Original terrain tokens and the identifiers immediately preceding them.
/// Unedited fields retain their original spacing when the document is saved.
pub struct EditableMap {
    source: String,
    saved_geometry: Option<(i32, i32, String)>,
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
            saved_geometry: None,
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
        if let Some((w, h, original)) = &self.saved_geometry {
            return *w != self.map.width() || *h != self.map.height() || self.text() != *original;
        }
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

    /// The dialog anchor is a point on a 3×3 grid, numbered left to right.
    /// The stored border is resized along with the playable terrain.
    pub fn resized(&self, size: [usize; 2], anchor: usize, fill: Option<&str>) -> Result<Self, MapParseError> {
        if size.contains(&0) { return Err(MapParseError::Empty); }
        if size.iter().any(|&v| v > super::MAX_STORED_SIDE - 2) { return Err(MapParseError::TooLarge); }
        if let Some(code) = fill && (!valid_terrain_code(code) || code.contains(|c: char| c.is_whitespace() || c == ',')) {
            return Err(MapParseError::InvalidTerrain { row: 1, column: 1 });
        }
        let [w, h] = size.map(|n| n as i32);
        let (old_w, old_h) = (self.map.width(), self.map.height());
        if old_w < 1 || old_h < 1 { return Err(MapParseError::Empty); }
        let offset = |diff, index| match index { 0 => 0, 1 => diff / 2, _ => diff };
        let dx = offset(old_w - w, anchor % 3);
        let dy = offset(old_h - h, anchor / 3);
        let text = self.text();
        let body = map_body(&text);
        let mut output = text[..body.as_ptr() as usize - text.as_ptr() as usize].to_owned();
        let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
        let cells: Vec<_> = body.split(['\r', '\n']).filter(|s| !s.is_empty())
            .flat_map(|row| row.split(',').map(str::trim)).collect();
        for y in -1..=h {
            for x in -1..=w {
                if x != -1 { output.push_str(", "); }
                let (sx, sy) = (x + dx, y + dy);
                if (-1..=old_w).contains(&sx) && (-1..=old_h).contains(&sy) {
                    output.push_str(cells[((sy + 1) * (old_w + 2) + sx + 1) as usize]);
                } else if let Some(terrain) = fill {
                    output.push_str(terrain);
                } else {
                    // Copy the nearest stored edge terrain, without duplicating its named locations.
                    let at = Hex { x: sx.clamp(-1, old_w), y: sy.clamp(-1, old_h) };
                    output.push_str(self.map.terrain_at(at).ok_or(MapParseError::Empty)?);
                }
            }
            if y != h || text.ends_with(['\n', '\r']) { output.push_str(newline); }
        }
        Self::parse(output)
    }

    /// Replace the complete terrain geometry while retaining the last saved baseline across undo and redo.
    pub fn swap_geometry(&mut self, mut next: Self) -> Self {
        let saved = self.saved_geometry.take().unwrap_or_else(||
            (self.map.width(), self.map.height(), self.source.clone()));
        next.saved_geometry = (next.source != saved.2).then_some(saved);
        std::mem::replace(self, next)
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
    fn resizing_translates_start_positions_and_only_duplicates_terrain_at_the_edges() {
        let source = "usage=map\r\nborder_size=1\r\n\r\n1 N Gg, Ww, Mm, Hh\r\nDd, village Vh, Ds, Re\r\nAa, Ss, Wo, Ww\r\n";
        let mut map = EditableMap::parse(source.into()).unwrap();
        let next = map.resized([4, 3], 4, None).unwrap();
        let undo = map.swap_geometry(next);
        assert!(map.dirty() && map.text().starts_with("usage=map\r\nborder_size=1\r\n\r\n"));
        assert_eq!(map.map().terrain_at(Hex {x:-1,y:-1}),Some("Gg"));
        assert_eq!(map.map().special_location("1"),Some(Hex{x:0,y:0}));
        assert_eq!(map.map().special_location("village"),Some(Hex{x:1,y:1}));
        assert_eq!(map.text().matches("1 N ").count(),1);
        let smaller = map.resized([1,1], 8, Some("Ke")).unwrap();
        assert_eq!(smaller.map().special_location("village"),None);
        map.swap_geometry(undo);
        assert_eq!(map.text(),source);
        assert!(!map.dirty());
        let larger = map.resized([5,1],0,Some("Ke")).unwrap();
        assert_eq!(larger.map().special_location("1"), Some(Hex{x:-1,y:-1}));
        assert_eq!(larger.map().terrain_at(Hex{x:5,y:0}),Some("Ke"));
    }
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
