use crate::{dialog, render};
use engine_lite::wml::Node;
use macroquad::prelude::*;
use rustnoth_gui::assets::AssetCatalog;
use std::{collections::HashMap, path::Path};

pub(super) struct Brush {
    pub code: String,
    pub label: String,
    group: String,
    pub image: String,
    base: Option<String>,
}

impl Brush {
    pub fn paint(&self, original: &str, layer_only: bool) -> String {
        let (old_base, old_overlay) = original.split_once('^').unwrap_or((original, ""));
        if self.code.starts_with('^') {
            format!(
                "{}{}",
                if layer_only {
                    old_base
                } else {
                    self.base.as_deref().unwrap_or(old_base)
                },
                self.code
            )
        } else if layer_only && !old_overlay.is_empty() {
            format!(
                "{}^{old_overlay}",
                self.code.split('^').next().unwrap_or(&self.code)
            )
        } else {
            self.code.clone()
        }
    }
}

pub(super) struct Palette {
    pub brushes: Vec<Brush>,
    groups: Vec<String>,
    selected_group: usize,
    pub fg: usize,
    pub bg: usize,
    first: usize,
}

fn choices(cfg: &Node) -> Vec<Brush> {
    cfg.children
        .iter()
        .filter(|(tag, node)| {
            tag == "terrain_type"
                && !matches!(node.attribute("hide_in_editor"), Some("true" | "yes" | "1"))
        })
        .filter_map(|(_, node)| {
            let group = node.attribute("editor_group")?.split(',').next()?.trim();
            let code = node.attribute("string")?;
            let image = node
                .attribute("editor_image")
                .or_else(|| node.attribute("symbol_image"))?;
            Some(Brush {
                code: code.into(),
                group: group.into(),
                label: node
                    .attribute("editor_name")
                    .or_else(|| node.attribute("name"))
                    .unwrap_or(code)
                    .into(),
                base: node
                    .attribute("default_base")
                    .filter(|base| !base.is_empty())
                    .map(str::to_owned),
                image: format!("terrain/{image}.png"),
            })
        })
        .collect()
}

pub(super) struct Art {
    cache: HashMap<String, Option<Texture2D>>,
}

impl Art {
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
        }
    }
    pub fn image(&mut self, assets: &AssetCatalog, key: &str, rect: Rect) {
        self.tinted(assets, key, rect, WHITE);
    }

    pub fn tinted(&mut self, assets: &AssetCatalog, key: &str, rect: Rect, tint: Color) {
        let image = self.cache.entry(key.into()).or_insert_with(|| {
            assets
                .resolve_image(Path::new(key))
                .filter(|p| p.is_file())
                .and_then(|path| render::load_texture(&path, None))
        });
        if let Some(image) = image {
            draw_texture_ex(
                image,
                rect.x,
                rect.y,
                tint,
                DrawTextureParams {
                    dest_size: Some(rect.size()),
                    ..Default::default()
                },
            );
        }
    }
}

impl Palette {
    pub fn new(cfg: &Node) -> Result<Self, &'static str> {
        let brushes = choices(cfg);
        if brushes.is_empty() {
            return Err("No editor terrain definitions were found in the core data.");
        }
        let fg = brushes.iter().position(|b| b.code == "Gg").unwrap_or(0);
        let bg = brushes.iter().position(|b| b.code == "Ww").unwrap_or(fg);
        let mut groups = Vec::new();
        for b in &brushes {
            if !groups.contains(&b.group) {
                groups.push(b.group.clone());
            }
        }
        let selected_group = groups
            .iter()
            .position(|g| g == &brushes[fg].group)
            .unwrap_or(0);
        Ok(Self {
            brushes,
            groups,
            fg,
            bg,
            selected_group,
            first: 0,
        })
    }

    fn tile(&self, i: usize, bounds: Rect, art: &mut Art, assets: &AssetCatalog) {
        let b = &self.brushes[i];
        if let Some(base) = b
            .base
            .as_ref()
            .and_then(|base| self.brushes.iter().find(|x| &x.code == base))
        {
            art.image(assets, &base.image, bounds);
        }
        art.image(assets, &b.image, bounds);
    }

    pub fn sample(&mut self, code: &str, foreground: bool) {
        let mut index = self.brushes.iter().position(|b| b.code == code);
        if index.is_none()
            && let Some(at) = code.find('^')
        {
            index = self.brushes.iter().position(|b| b.code == code[at..]);
        }
        if let Some(i) = index {
            if foreground {
                self.fg = i;
            } else {
                self.bg = i;
            }
            self.selected_group = self
                .groups
                .iter()
                .position(|g| g == &self.brushes[i].group)
                .unwrap_or(0);
            let group = &self.groups[self.selected_group];
            self.first = self.brushes[..i]
                .iter()
                .filter(|b| &b.group == group)
                .count()
                / 3;
        }
    }

    pub fn panel(
        &mut self,
        bounds: Rect,
        point: Vec2,
        assets: &AssetCatalog,
        art: &mut Art,
        font: Option<&Font>,
    ) {
        let library_y = bounds.y + 94.0;
        let controls_y = bounds.y + 58.0;
        let fg = Rect::new(bounds.x + 10.0, bounds.y + 3.0, 52.0, 52.0);
        let bg = Rect::new(bounds.x + 62.0, bounds.y + 15.0, 37.0, 37.0);
        self.tile(self.bg, bg, art, assets);
        self.tile(self.fg, fg, art, assets);
        let label = dialog::fit(
            font,
            self.brushes[self.fg].label.clone(),
            bounds.w - 104.0,
            14,
        );
        dialog::text(
            font,
            &label,
            bg.right() + 3.0,
            fg.center().y + 6.0,
            14,
            WHITE,
        );
        let prev = Rect::new(bounds.x + 4.0, controls_y, 24.0, 26.0);
        let next = Rect::new(bounds.right() - 28.0, controls_y, 24.0, 26.0);
        for (rect, arrow) in [(prev, "left"), (next, "right")] {
            art.image(
                assets,
                &format!("icons/arrows/arrows_ornate_{arrow}_25.png"),
                rect,
            );
        }
        if is_mouse_button_pressed(MouseButton::Left) {
            if prev.contains(point) {
                self.selected_group =
                    (self.selected_group + self.groups.len() - 1) % self.groups.len();
                self.first = 0;
            }
            if next.contains(point) {
                self.selected_group = (self.selected_group + 1) % self.groups.len();
                self.first = 0;
            }
        }
        let group = &self.groups[self.selected_group];
        let caption = dialog::fit(font, group.replace('_', " "), bounds.w - 62.0, 16);
        let w = measure_text(&caption, font, 16, 1.0).width;
        dialog::text(
            font,
            &caption,
            bounds.center().x - w / 2.0,
            controls_y + 20.0,
            16,
            dialog::GOLD,
        );
        let entries: Vec<_> = self
            .brushes
            .iter()
            .enumerate()
            .filter(|(_, b)| &b.group == group)
            .map(|(i, _)| i)
            .collect();
        let cell = ((bounds.w - 14.0) / 3.0).min(54.0);
        let visible = (((bounds.bottom() - library_y) / cell).floor() as usize).max(1);
        let last = entries.len().div_ceil(3).saturating_sub(visible);
        let wheel = mouse_wheel().1;
        if bounds.contains(point) && wheel != 0.0 {
            self.first = self
                .first
                .saturating_add_signed(if wheel > 0.0 { -1 } else { 1 });
        }
        self.first = self.first.min(last);
        for (n, &i) in entries
            .iter()
            .skip(self.first * 3)
            .take(visible * 3)
            .enumerate()
        {
            let r = Rect::new(
                bounds.x + 7.0 + (n % 3) as f32 * cell,
                library_y + (n / 3) as f32 * cell,
                cell - 2.0,
                cell - 2.0,
            );
            self.tile(i, r, art, assets);
            if i == self.fg || i == self.bg || r.contains(point) {
                draw_rectangle_lines(
                    r.x,
                    r.y,
                    r.w,
                    r.h,
                    2.0,
                    if i == self.fg {
                        YELLOW
                    } else if i == self.bg {
                        BLUE
                    } else {
                        WHITE
                    },
                );
            }
            if r.contains(point) {
                if is_mouse_button_pressed(MouseButton::Left) {
                    self.fg = i;
                }
                if is_mouse_button_pressed(MouseButton::Right) {
                    self.bg = i;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_single_layer_brush_keeps_the_other_layer_and_full_overlay_uses_its_default_base() {
        let b = Brush {
            code: "^Wkf".into(),
            base: Some("Ww".into()),
            group: String::new(),
            label: String::new(),
            image: String::new(),
        };
        assert_eq!(b.paint("Wo^Ewl", false), "Ww^Wkf");
        assert_eq!(b.paint("Wo^Ewl", true), "Wo^Wkf");
        let grass = Brush {
            code: "Gg".into(),
            ..b
        };
        assert_eq!(grass.paint("Gs^Fds", true), "Gg^Fds");
        assert_eq!(grass.paint("Gs^Fds", false), "Gg");
    }
}
