use super::{Art, History, palette::Brush};
use engine_lite::{hex::Hex, map::{ScenarioMap, editor::EditableMap}};
use macroquad::prelude::*;
use rustnoth_gui::assets::AssetCatalog;
use std::collections::HashSet;

#[derive(Default)]
pub(super) struct Region(HashSet<Hex>);

impl Region {
    pub fn clear(&mut self) {
        self.0.clear();
    }

    pub fn contains(&self, hex: Hex) -> bool {
        self.0.contains(&hex)
    }

    pub fn follow(&mut self, map: &ScenarioMap, hover: Option<Hex>) {
        if hover.is_some_and(|hex| self.contains(hex)) {
            return;
        }
        self.clear();
        let Some(start) = hover else { return };
        let Some(code) = map.terrain_at(start) else { return };
        let mut pending = vec![start];
        self.0.insert(start);
        while let Some(next) = pending.pop() {
            for hex in next.neighbors() {
                if map.terrain_at(hex) == Some(code) && self.0.insert(hex) {
                    pending.push(hex);
                }
            }
        }
    }

    pub fn paint(&self, doc: &mut EditableMap, history: &mut History, at: Hex,
                 brush: &Brush, layer_only: bool) -> bool {
        let Some(old) = doc.map().terrain_at(at) else { return false };
        let code = brush.paint(old, layer_only);
        if old == code {
            return false;
        }
        for &hex in &self.0 {
            history.paint(doc, hex, &code);
        }
        history.finish()
    }
}

#[derive(Copy, Clone, Eq, PartialEq)]
pub(super) enum Tool { Paint, Fill }

impl Tool {
    pub fn panel(self, bounds: Rect, point: Vec2, art: &mut Art, assets: &AssetCatalog,
                 tip: &mut Option<&'static str>) -> Self {
        art.image(assets, "themes/editor/classic/tools.png", bounds);
        let mut selected = self;
        for (row, (tool, icon, label)) in [
            (Self::Paint, "paint", "Paint Tool (P)"),
            (Self::Fill, "fill", "Fill Tool (F)"),
        ].into_iter().enumerate() {
            let button = Rect::new(bounds.x + 14.0, bounds.y + 5.0 + row as f32 * 32.0, 30.0, 30.0);
            let hot = button.contains(point);
            let suffix = match (hot, self == tool) {
                (true, true) => "-active-pressed",
                (true, false) => "-active",
                (false, true) => "-pressed",
                (false, false) => "",
            };
            art.image(assets, &format!("buttons/button_square/button_square_30{suffix}.png"), button);
            let icon_state = if self == tool { "-pressed" } else if hot { "-active" } else { "" };
            art.image(assets, &format!("icons/action/editor-tool-{icon}_30{icon_state}.png"), button);
            if hot {
                *tip = Some(label);
                if is_mouse_button_pressed(MouseButton::Left) {
                    selected = tool;
                }
            }
        }
        selected
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::Palette;

    #[test]
    fn region_fill_follows_layered_hexes_into_the_border_and_undoes_once() {
        let original = "Gg^Fds, Mm, Mm, Mm, Mm, Mm\n\
                        Mm, Gg^Fds, Mm, Gg, Gg^Fds, Mm\n\
                        Mm, Gg^Fds, Gg^Fds, Mm, Mm, Mm\n\
                        Mm, Mm, Mm, Mm, Mm, Mm\n";
        let mut doc = EditableMap::parse(original.into()).unwrap();
        let seed = Hex { x: 0, y: 0 };
        let mut region = Region::default();
        region.follow(doc.map(), Some(seed));
        assert_eq!(region.0.len(), 4);
        assert!(region.contains(Hex { x: -1, y: -1 }));
        assert!(!region.contains(Hex { x: 2, y: 0 }));
        assert!(!region.contains(Hex { x: 3, y: 0 }));
        let cfg = engine_lite::wml::parse("[terrain_type]\nstring=Hh\neditor_group=rough\nsymbol_image=hills\n[/terrain_type]").unwrap();
        let brush = &Palette::new(&cfg).unwrap().brushes[0];
        let mut history = History::default();
        assert!(region.paint(&mut doc, &mut history, seed, brush, true));
        assert_eq!(doc.text().matches("Hh^Fds").count(), 4);
        assert_eq!(doc.map().terrain_at(Hex { x: -1, y: -1 }), Some("Hh^Fds"));
        assert!(history.apply(&mut doc, true));
        assert_eq!(doc.text(), original);
        assert!(!history.apply(&mut doc, true));
        assert!(history.apply(&mut doc, false));
        assert_eq!(doc.text().matches("Hh^Fds").count(), 4);
    }
}
