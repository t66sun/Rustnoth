use super::{EditableMap, Hex, Palette};
use crate::{dialog, render::StatusBox};
use macroquad::prelude::*;
use rustnoth_gui::assets::AssetCatalog;
use std::path::Path;

pub(super) struct Reports {
    frame: StatusBox,
    last: Option<(Hex, String)>,
}

fn bounds(w: f32, h: f32) -> [Rect; 2] {
    let top = if w >= 1024. && h >= 768. { 261. } else { 176. };
    [Rect::new(w - 174., top, 171., 21.), Rect::new(w - 174., top + 19., 171., 21.)]
}

fn coordinate(hex: Hex) -> String {
    format!("{},{}", hex.x + 1, hex.y + 1)
}

impl Reports {
    pub fn new(root: &Path) -> Self {
        Self { frame: StatusBox::load(root), last: None }
    }

    pub fn draw(&mut self, w: f32, h: f32, hovered: Option<Hex>, doc: &EditableMap,
                palette: &Palette, assets: &AssetCatalog, font: Option<&Font>) {
        let [position, terrain] = bounds(w, h);
        self.frame.draw(position, 1.);
        self.frame.draw(terrain, 1.);
        if let Some(hex) = hovered && let Some(code) = doc.map().terrain_at(hex) {
            let label = palette.brushes.iter().find(|brush| brush.code == code)
                .map(|brush| brush.label.as_str()).or_else(|| assets.terrain_name(code)).unwrap_or(code);
            if self.last.as_ref().is_none_or(|(previous, old)| *previous != hex || old != label) {
                self.last = Some((hex, label.to_owned()));
            }
        }
        if let Some((hex, label)) = &self.last {
            dialog::text(font, &coordinate(*hex), position.x + 5., position.y + 15., 15, WHITE);
            let name = dialog::fit(font, label.clone(), terrain.w - 7., 13);
            dialog::text(font, &name, terrain.x + 4., terrain.y + 15., 13, WHITE);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editor_reports_share_the_minimap_edge_and_use_wml_coordinates() {
        assert_eq!(bounds(1200., 820.), [Rect::new(1026., 261., 171., 21.), Rect::new(1026., 280., 171., 21.)]);
        assert_eq!(bounds(800., 600.)[0], Rect::new(626., 176., 171., 21.));
        assert_eq!(coordinate(Hex {x: -1, y: 0}), "0,1");
    }
}
