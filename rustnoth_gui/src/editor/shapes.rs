use super::{Art, History, palette::Brush};
use engine_lite::{hex::Hex, map::editor::EditableMap, wml::{self, Node}};
use macroquad::prelude::*;
use rustnoth_gui::assets::AssetCatalog;
use std::{collections::BTreeSet, error::Error, fs, io, path::Path};

const ORIGIN: Hex = Hex { x: 0, y: 0 };
const CONTROLS: [(&str, &str, KeyCode); 5] = [
    ("brush-1", "Single Hex (1)", KeyCode::Key1),
    ("brush-2", "Radius 1 Hex (2)", KeyCode::Key2),
    ("brush-3", "Radius 2 Hex (3)", KeyCode::Key3),
    ("brush-nw-se", "Hex Line NW-SE (4)", KeyCode::Key4),
    ("brush-sw-ne", "Hex Line SW-NE (5)", KeyCode::Key5),
];

struct Shape {
    id: String,
    relative: Vec<Hex>,
}

pub(super) struct Shapes {
    definitions: Vec<Shape>,
    selected: usize,
    footprint_key: Option<(usize, Hex)>,
    footprint: Vec<Hex>,
}

impl Shapes {
    pub fn load(root: &Path, core: &Node) -> Result<Self, Box<dyn Error>> {
        if core.children.iter().any(|(tag, _)| tag == "brush") {
            return Ok(Self::from_config(core));
        }
        match fs::read(root.join("data/core/editor/brushes.cfg")) {
            Ok(bytes) => Ok(Self::from_config(&wml::parse(bytes)?)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Self::from_config(core)),
            Err(e) => Err(e.into()),
        }
    }

    fn from_config(core: &Node) -> Self {
        let mut definitions = Vec::new();
        for (_, cfg) in core.children.iter().filter(|(tag, _)| tag == "brush") {
            let Some(id) = cfg.attribute("id") else { continue };
            let number = |node: &Node, key| node.attribute(key).and_then(|v| v.parse::<i32>().ok()).unwrap_or(0);
            let mut offsets = BTreeSet::new();
            let radius = number(cfg, "radius");
            if radius > 0 {
                for x in -radius..=radius {
                    for y in -radius..=radius {
                        let at = Hex { x, y };
                        if ORIGIN.distance(at) <= radius as u64 { offsets.insert(at); }
                    }
                }
            }
            for (_, offset) in cfg.children.iter().filter(|(tag, _)| tag == "relative") {
                offsets.insert(Hex { x: number(offset, "x"), y: number(offset, "y") });
            }
            if !offsets.is_empty() {
                definitions.push(Shape { id: id.into(), relative: offsets.into_iter().collect() });
            }
        }
        if definitions.is_empty() {
            definitions.push(Shape { id: "brush-1".into(), relative: vec![ORIGIN] });
        }
        Self { definitions, selected: 0, footprint_key: None, footprint: Vec::new() }
    }

    pub fn clear_footprint(&mut self) {
        self.footprint_key = None;
    }

    /// Reuse the clipped brush while its center, shape, and map geometry agree.
    pub fn footprint(&mut self, center: Hex, mut is_stored: impl FnMut(Hex) -> bool) -> &[Hex] {
        let key = (self.selected, center);
        if self.footprint_key != Some(key) {
            self.footprint.clear();
            let offsets = &self.definitions[self.selected].relative;
            let cells = offsets.iter().map(|delta| Hex {
                x: center.x + delta.x,
                // Wesnoth map_location::vector_sum carries one row for two odd columns.
                y: center.y + delta.y + (center.x & delta.x & 1),
            });
            self.footprint.extend(cells.filter(|&at| is_stored(at)));
            self.footprint_key = Some(key);
        }
        &self.footprint
    }

    pub fn keys(&mut self) {
        if is_key_pressed(KeyCode::B) {
            self.selected = (self.selected + 1) % self.definitions.len();
        }
        for (id, _, key) in CONTROLS {
            if is_key_pressed(key) && let Some(i) = self.definitions.iter().position(|s| s.id == id) {
                self.selected = i;
            }
        }
    }

    pub fn panel(&mut self, bounds: Rect, compact: bool, enabled: bool, point: Vec2,
                 art: &mut Art, assets: &AssetCatalog, tip: &mut Option<&'static str>) {
        let (side, stride, left, top) = if compact { (25, 26., 13., 59.) } else { (30, 32., 14., 117.) };
        for (col, (id, label, _)) in CONTROLS.iter().enumerate() {
            let button = Rect::new(bounds.x + left + col as f32 * stride, bounds.y + top, side as f32, side as f32);
            let index = self.definitions.iter().position(|s| &s.id == id);
            let available = enabled && index.is_some();
            let hot = available && button.contains(point);
            let selected = available && index == Some(self.selected);
            let state = match (hot, selected) {
                (true, true) => "-active-pressed", (false, true) => "-pressed",
                (true, false) => "-active", _ => "",
            };
            let overlay = if selected { "-pressed" } else if hot { "-active" } else { "" };
            let tint = if available { WHITE } else { GRAY };
            art.tinted(assets, &format!("buttons/button_square/button_square_{side}{state}.png"), button, tint);
            art.tinted(assets, &format!("icons/action/editor-{id}_{side}{overlay}.png"), button, tint);
            if hot {
                *tip = Some(label);
                if is_mouse_button_pressed(MouseButton::Left) {
                    self.selected = index.unwrap();
                }
            }
        }
    }
}

pub(super) fn paint(doc: &mut EditableMap, history: &mut History, footprint: &[Hex],
                    brush: &Brush, layer_only: bool) {
    for &at in footprint {
        if let Some(old) = doc.map().terrain_at(at) {
            let code = brush.paint(old, layer_only);
            history.paint(doc, at, &code);
        }
    }
    history.finish_brush_step();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::{Palette, new_map};

    #[test]
    fn paint_uses_the_preview_footprint_on_odd_columns_and_undoes_one_drag() {
        let cfg = wml::parse("[brush]\nid=brush-nw-se\n[relative]\nx=-1\ny=-1\n[/relative]\n\
            [relative]\nx=0\n[/relative]\n[relative]\nx=1\ny=0\n[/relative]\n[/brush]\n\
            [brush]\nid=brush-3\nradius=2\n[relative]\nx=0\n[/relative]\n[/brush]\n\
            [terrain_type]\nstring=Ww\neditor_group=water\nsymbol_image=water\n[/terrain_type]").unwrap();
        let mut shapes = Shapes::from_config(&cfg);
        let palette = Palette::new(&cfg, &Default::default()).unwrap();
        let mut doc = new_map();
        let mut history = History::default();
        let present = |at| doc.map().terrain_at(at).is_some();
        let first = shapes.footprint(Hex { x: 1, y: 2 }, present).to_vec();
        assert_eq!(first, [Hex { x: 0, y: 2 }, Hex { x: 1, y: 2 }, Hex { x: 2, y: 3 }]);
        let second = shapes.footprint(Hex { x: 2, y: 3 }, present);
        assert_eq!(second[0], first[1]);
        for cells in [first.as_slice(), second] {
            paint(&mut doc, &mut history, cells, &palette.brushes[0], false);
        }
        assert!(history.finish());
        assert!(first.iter().chain(second).all(|at| doc.map().terrain_at(*at) == Some("Ww")));
        assert_eq!(doc.map().terrain_at(Hex { x: 2, y: 2 }), Some("Gg"));
        assert!(history.apply(&mut doc, true));
        assert!(!doc.dirty());
        assert!(!history.apply(&mut doc, true));
        shapes.selected = 1;
        let present = |at| doc.map().terrain_at(at).is_some();
        assert_eq!(shapes.footprint(Hex { x: 3, y: 3 }, present).len(), 19);
        let edge = shapes.footprint(Hex { x: -1, y: -1 }, present);
        assert!(edge.len() < 19 && edge.contains(&Hex { x: -1, y: -1 }));
    }

    #[test]
    fn stationary_brush_reuses_footprint_until_shape_or_map_changes() {
        let cfg = wml::parse("[brush]\nid=brush-1\n[relative]\nx=0\n[/relative]\n[/brush]\n\
            [brush]\nid=brush-2\nradius=1\n[/brush]").unwrap();
        let mut shapes = Shapes::from_config(&cfg);
        let center = Hex { x: 3, y: 3 };

        assert_eq!(shapes.footprint(center, |_| true), [center]);
        assert_eq!(shapes.footprint(center, |_| panic!("unchanged footprint was recomputed")), [center]);
        shapes.selected = 1;
        assert_eq!(shapes.footprint(center, |_| true).len(), 7);

        shapes.clear_footprint();
        assert_eq!(shapes.footprint(center, |at| at == center), [center]);
    }
}
