use crate::dialog::{fit, text};
use macroquad::prelude::*;
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

pub(super) const LABEL: Color = Color::from_rgba(221, 221, 221, 255);
pub(super) const GOLD: Color = Color::from_rgba(186, 172, 125, 255);
pub(super) const EDGE: Color = Color::from_rgba(94, 81, 62, 255);

pub(super) struct Art {
    root: PathBuf,
    pub font: Option<Font>,
    textures: HashMap<PathBuf, Option<Texture2D>>,
}

pub(super) fn fill(r: Rect, color: Color) {
    draw_rectangle(r.x, r.y, r.w, r.h, color);
}
pub(super) fn edge(r: Rect) {
    draw_rectangle_lines(r.x, r.y, r.w, r.h, 2.0, EDGE);
}
pub(super) fn image(tex: &Texture2D, r: Rect) {
    draw_texture_ex(
        tex,
        r.x,
        r.y,
        WHITE,
        DrawTextureParams {
            dest_size: Some(r.size()),
            ..Default::default()
        },
    );
}

impl Art {
    pub fn new(root: &Path) -> Self {
        let font = fs::read(root.join("fonts/Lato-Regular.ttf"))
            .ok()
            .and_then(|bytes| load_ttf_font_from_bytes(&bytes).ok());
        Self {
            root: root.join("images"),
            font,
            textures: HashMap::new(),
        }
    }

    pub fn get(&mut self, id: &str) -> Option<Texture2D> {
        self.file(self.root.join(id))
    }

    pub fn file(&mut self, path: PathBuf) -> Option<Texture2D> {
        self.textures
            .entry(path.clone())
            .or_insert_with(|| crate::render::load_texture(&path, None))
            .clone()
    }

    pub fn backdrop(&mut self, w: f32, h: f32) {
        clear_background(Color::from_rgba(13, 18, 29, 255));
        if let Some(tile) = self.get("dialogs/opaque-background.png") {
            let mut y = 0.0;
            while y < h {
                let mut x = 0.0;
                while x < w {
                    draw_texture(&tile, x, y, WHITE);
                    x += tile.width();
                }
                y += tile.height();
            }
        }
        if let Some(bg) = self.get("dialogs/multi_create/background.png") {
            let width = bg.width() * h / bg.height();
            image(&bg, Rect::new(w - width, 0.0, width, h));
        }
        if let Some(corner) = self.get("dialogs/multi_create/corner.png") {
            let offset = (corner.width() - corner.height()) * 0.5;
            for (x, y, angle) in [
                (-offset, offset, 90.0_f32),
                (
                    w - corner.height() - offset,
                    h - corner.width() + offset,
                    -90.0,
                ),
            ] {
                draw_texture_ex(
                    &corner,
                    x,
                    y,
                    WHITE,
                    DrawTextureParams {
                        rotation: angle.to_radians(),
                        ..Default::default()
                    },
                );
            }
        }
        if let Some(decor) = self.get("dialogs/multi_create/decor.png") {
            draw_texture(&decor, (w - decor.width()) * 0.5, h - decor.height(), WHITE);
        }
    }

    pub fn label(&self, s: &str, x: f32, y: f32, size: u16, c: Color, max_width: f32) {
        text(
            self.font.as_ref(),
            &fit(self.font.as_ref(), s.into(), max_width.max(1.0), size),
            x,
            y,
            size,
            c,
        );
    }

    pub fn button(&mut self, r: Rect, s: &str, mouse: Vec2, on: bool) {
        let state = if r.contains(mouse) && is_mouse_button_down(MouseButton::Left) {
            "-active-pressed"
        } else if r.contains(mouse) || on {
            "-active"
        } else {
            ""
        };
        fill(r, Color::from_rgba(34, 45, 60, 255));
        if let Some(tex) = self.get(&format!("buttons/large-button{state}.png")) {
            image(&tex, r);
        }
        let label = fit(self.font.as_ref(), s.into(), (r.w - 12.0).max(1.0), 17);
        let width = measure_text(&label, self.font.as_ref(), 17, 1.0).width;
        text(
            self.font.as_ref(),
            &label,
            r.center().x - width * 0.5,
            r.center().y + 6.0,
            17,
            GOLD,
        );
    }

    pub fn row(&mut self, r: Rect, on: bool, hover: bool) {
        if on || hover {
            fill(r, Color::from_rgba(37, 52, 68, 240));
            if let Some(bg) = self.get("dialogs/selection-background-fancy.png") {
                image(&bg, r);
            }
            if on {
                edge(r);
            }
        }
    }

    pub fn wrapped(&self, s: &str, r: Rect) {
        if r.h < 17.0 {
            return;
        }
        let mut baseline = r.y + 17.0;
        for paragraph in s.lines() {
            let mut line = String::new();
            for word in paragraph.split_whitespace() {
                let next = if line.is_empty() {
                    word.into()
                } else {
                    format!("{line} {word}")
                };
                if !line.is_empty() && measure_text(&next, self.font.as_ref(), 16, 1.0).width > r.w
                {
                    text(self.font.as_ref(), &line, r.x, baseline, 16, LABEL);
                    baseline += 21.0;
                    line = word.into();
                    if baseline > r.bottom() {
                        return;
                    }
                } else {
                    line = next;
                }
            }
            text(self.font.as_ref(), &line, r.x, baseline, 16, LABEL);
            baseline += 23.0;
            if baseline > r.bottom() {
                return;
            }
        }
    }
}
