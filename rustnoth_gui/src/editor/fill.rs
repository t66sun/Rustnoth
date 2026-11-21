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

    pub fn hexes(&self) -> impl Iterator<Item = Hex> + '_ {
        self.0.iter().copied()
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
pub(super) enum Tool { Paint, Fill, Select, Location, Paste }

impl Tool {
    pub fn panel(self, bounds: Rect, compact: bool, point: Vec2, art: &mut Art, assets: &AssetCatalog,
                 tip: &mut Option<&'static str>, can_paste: bool) -> Self {
        let background = if compact { "themes/editor/classic/tools-800.png" } else { "themes/editor/classic/tools.png" };
        art.image(assets, background, bounds);
        let (size, left, stride) = if compact { (25, 13., 26.) } else { (30, 14., 32.) };
        let mut selected = self;
        for (tool, icon, label, col, row) in [
            (Self::Paint, "paint", "Paint Tool (P)", 0., 0.),
            (Self::Select, "select", "Selection Tool (S)", 1., 0.),
            (Self::Location, "starting-position", "Starting Locations Tool", 2., 0.),
            (Self::Fill, "fill", "Fill Tool (F)", 0., 1.),
        ] {
            let button = Rect::new(bounds.x + left + col * stride, bounds.y + 5.0 + row * stride, size as f32, size as f32);
            let hot = button.contains(point);
            let suffix = match (hot, self == tool) {
                (true, true) => "-active-pressed",
                (true, false) => "-active",
                (false, true) => "-pressed",
                (false, false) => "",
            };
            art.image(assets, &format!("buttons/button_square/button_square_{size}{suffix}.png"), button);
            let icon_state = if self == tool { "-pressed" } else if hot { "-active" } else { "" };
            art.image(assets, &format!("icons/action/editor-tool-{icon}_{size}{icon_state}.png"), button);
            if hot {
                *tip = Some(label);
                if is_mouse_button_pressed(MouseButton::Left) {
                    selected = tool;
                }
            }
        }
        // Scenario-only actions have gray artwork on a plain terrain document.
        for (icon, col, row) in [
            ("editor-tool-item", 3., 0.), ("editor-tool-village", 4., 0.),
            ("editor-paste", 1., 1.), ("editor-tool-label", 2., 1.),
            ("editor-tool-unit", 3., 1.), ("", 4., 1.),
        ] {
            let rect = Rect::new(bounds.x + left + col * stride, bounds.y + 5. + row * stride, size as f32, size as f32);
            if icon == "editor-paste" && can_paste {
                let hot = rect.contains(point);
                let suffix = if self == Self::Paste { "-pressed" } else if hot { "-active" } else { "" };
                art.image(assets, &format!("buttons/button_square/button_square_{size}{suffix}.png"), rect);
                art.image(assets, &format!("icons/action/{icon}_{size}{suffix}.png"), rect);
                if hot {
                    *tip = Some("Paste");
                    if is_mouse_button_pressed(MouseButton::Left) { selected = Self::Paste; }
                }
                continue;
            }
            art.image(assets, &format!("buttons/button_square/button_square_{size}.png~GS()"), rect);
            if !icon.is_empty() {
                art.image(assets, &format!("icons/action/{icon}_{size}.png~GS()"), rect);
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
        let brush = &Palette::new(&cfg, &Default::default()).unwrap().brushes[0];
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
