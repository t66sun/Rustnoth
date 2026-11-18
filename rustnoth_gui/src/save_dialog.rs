use crate::{
    dialog,
    saved_game::{self, Slot},
    show_board,
};
use macroquad::prelude::*;
use std::{error::Error, path::Path};

pub fn save_pressed() -> bool {
    is_key_pressed(KeyCode::S)
        && (is_key_down(KeyCode::LeftControl) || is_key_down(KeyCode::RightControl))
}

struct Skin {
    font: Option<Font>,
    panel: Option<Texture2D>,
    selected: Option<Texture2D>,
    buttons: [Option<Texture2D>; 2],
}

impl Skin {
    fn load(root: &Path) -> Self {
        let image = |name: &str| crate::render::load_texture(&root.join("images").join(name), None);
        Self {
            font: std::fs::read(root.join("fonts/Lato-Regular.ttf"))
                .ok()
                .and_then(|bytes| load_ttf_font_from_bytes(&bytes).ok()),
            panel: image("dialogs/opaque-background.png"),
            selected: image("dialogs/selection-background-fancy.png"),
            buttons: [
                image("buttons/large-button.png"),
                image("buttons/large-button-active.png"),
            ],
        }
    }
    fn paint(tex: &Option<Texture2D>, r: Rect) {
        if let Some(tex) = tex {
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
    }
    fn frame(&self, title: &str, w: f32, h: f32, backdrop: &Texture2D) -> (Rect, Vec2) {
        set_default_camera();
        draw_texture_ex(
            backdrop,
            0.0,
            0.0,
            WHITE,
            DrawTextureParams {
                dest_size: Some(vec2(screen_width(), screen_height())),
                flip_y: true,
                ..Default::default()
            },
        );
        draw_rectangle(
            0.0,
            0.0,
            screen_width(),
            screen_height(),
            Color::from_rgba(0, 0, 0, 125),
        );
        let scale = (screen_width() / (w + 16.0))
            .min(screen_height() / (h + 16.0))
            .clamp(0.01, 1.0);
        let size = vec2(screen_width(), screen_height()) / scale;
        set_camera(&dialog::setup_camera(size.x, size.y));
        let r = Rect::new((size.x - w) * 0.5, (size.y - h) * 0.5, w, h);
        draw_rectangle(r.x, r.y, w, h, Color::from_rgba(31, 34, 40, 255));
        if let Some(tex) = &self.panel {
            let tile = tex.size();
            let mut y = 0.0;
            while y < h {
                let mut x = 0.0;
                while x < w {
                    let cut = Rect::new(0.0, 0.0, tile.x.min(w - x), tile.y.min(h - y));
                    draw_texture_ex(
                        tex,
                        r.x + x,
                        r.y + y,
                        WHITE,
                        DrawTextureParams {
                            source: Some(cut),
                            ..Default::default()
                        },
                    );
                    x += tile.x;
                }
                y += tile.y;
            }
        }
        draw_rectangle_lines(r.x, r.y, w, h, 2.0, dialog::GOLD);
        dialog::text(
            self.font.as_ref(),
            title,
            r.x + 18.0,
            r.y + 38.0,
            28,
            dialog::GOLD,
        );
        (r, Vec2::from(mouse_position()) / scale)
    }
    fn button(&self, r: Rect, p: Vec2, label: &str, enabled: bool) -> bool {
        let hover = enabled && r.contains(p);
        Self::paint(&self.buttons[usize::from(hover)], r);
        let m = measure_text(label, self.font.as_ref(), 17, 1.0);
        dialog::text(
            self.font.as_ref(),
            label,
            r.center().x - m.width / 2.0,
            r.center().y + 6.0,
            17,
            if enabled { dialog::GOLD } else { GRAY },
        );
        hover && is_mouse_button_pressed(MouseButton::Left)
    }
}

pub(super) async fn name(root: &Path, default_name: &str) -> Option<String> {
    let background = Texture2D::from_image(&get_screen_data());
    let skin = Skin::load(root);
    let mut value: String = default_name.chars().take(127).collect();
    let mut replace = true;
    while get_char_pressed().is_some() {}
    let answer = loop {
        let (r, p) = skin.frame("Save Game", 500.0, 188.0, &background);
        let input = Rect::new(r.x + 92.0, r.y + 63.0, r.w - 110.0, 33.0);
        dialog::text(
            skin.font.as_ref(),
            "Name:",
            r.x + 18.0,
            input.y + 23.0,
            18,
            WHITE,
        );
        draw_rectangle(
            input.x,
            input.y,
            input.w,
            input.h,
            Color::from_rgba(16, 23, 29, 255),
        );
        draw_rectangle_lines(input.x, input.y, input.w, input.h, 1.0, dialog::GOLD);
        if replace {
            draw_rectangle(
                input.x + 3.0,
                input.y + 3.0,
                input.w - 6.0,
                input.h - 6.0,
                Color::from_rgba(63, 79, 100, 255),
            );
        }
        dialog::text(
            skin.font.as_ref(),
            &dialog::fit(skin.font.as_ref(), value.clone(), input.w - 16.0, 17),
            input.x + 7.0,
            input.y + 22.0,
            17,
            WHITE,
        );
        if is_mouse_button_pressed(MouseButton::Left) && input.contains(p) {
            replace = false;
        }
        if is_key_pressed(KeyCode::A)
            && (is_key_down(KeyCode::LeftControl) || is_key_down(KeyCode::RightControl))
        {
            replace = true;
        }
        if is_key_pressed(KeyCode::Backspace) {
            if replace {
                value.clear();
                replace = false;
            } else {
                value.pop();
            }
        }
        while let Some(ch) = get_char_pressed() {
            if !ch.is_control() {
                if replace {
                    value.clear();
                    replace = false;
                }
                if value.chars().count() < 127 {
                    value.push(ch);
                }
            }
        }
        let accept = skin.button(
            Rect::new(r.right() - 224.0, r.bottom() - 57.0, 98.0, 40.0),
            p,
            "Save",
            !value.trim().is_empty(),
        );
        let cancel = skin.button(
            Rect::new(r.right() - 115.0, r.bottom() - 57.0, 98.0, 40.0),
            p,
            "Cancel",
            true,
        );
        if cancel || is_key_pressed(KeyCode::Escape) {
            break None;
        }
        if !value.trim().is_empty() && (accept || is_key_pressed(KeyCode::Enter)) {
            break Some(value.trim().into());
        }
        next_frame().await;
    };
    set_default_camera();
    answer
}

async fn select(root: &Path, slots: &[Slot]) -> Option<usize> {
    let background = Texture2D::from_image(&get_screen_data());
    let skin = Skin::load(root);
    let mut cursor = 0usize;
    let rows = 8usize;
    let mut first = 0usize;
    let mut last_click: Option<(usize, f64)> = None;
    let answer = loop {
        let (r, p) = skin.frame("Load Game", 710.0, 460.0, &background);
        let heading = r.y + 75.0;
        dialog::text(
            skin.font.as_ref(),
            "Name",
            r.x + 26.0,
            heading,
            17,
            dialog::GOLD,
        );
        if slots.is_empty() {
            dialog::text(
                skin.font.as_ref(),
                "No saved games",
                r.x + 26.0,
                heading + 40.0,
                18,
                WHITE,
            );
        }
        let mut double = false;
        if !slots.is_empty() {
            let wheel = mouse_wheel().1;
            let step = if is_key_pressed(KeyCode::Down) || wheel < 0.0 {
                1
            } else if is_key_pressed(KeyCode::Up) || wheel > 0.0 {
                -1
            } else if is_key_pressed(KeyCode::PageDown) {
                rows as isize
            } else if is_key_pressed(KeyCode::PageUp) {
                -(rows as isize)
            } else {
                0
            };
            cursor = cursor.saturating_add_signed(step).min(slots.len() - 1);
            if is_key_pressed(KeyCode::Home) {
                cursor = 0
            }
            if is_key_pressed(KeyCode::End) {
                cursor = slots.len() - 1
            }
            first = first.clamp(cursor.saturating_sub(rows - 1), cursor);
            for (index, slot) in slots.iter().enumerate().skip(first).take(rows) {
                let row = Rect::new(
                    r.x + 18.0,
                    heading + 8.0 + (index - first) as f32 * 36.0,
                    r.w - 36.0,
                    35.0,
                );
                if cursor == index {
                    Skin::paint(&skin.selected, row);
                }
                dialog::text(
                    skin.font.as_ref(),
                    &dialog::fit(skin.font.as_ref(), slot.label.clone(), row.w - 20.0, 17),
                    row.x + 9.0,
                    row.y + 24.0,
                    17,
                    WHITE,
                );
                if row.contains(p) && is_mouse_button_pressed(MouseButton::Left) {
                    let now = get_time();
                    double = last_click.is_some_and(|(i, t)| i == index && now - t < 0.4);
                    last_click = Some((index, now));
                    cursor = index;
                }
            }
        }
        let load = skin.button(
            Rect::new(r.right() - 240.0, r.bottom() - 58.0, 106.0, 40.0),
            p,
            "Load",
            !slots.is_empty(),
        );
        let cancel = skin.button(
            Rect::new(r.right() - 122.0, r.bottom() - 58.0, 106.0, 40.0),
            p,
            "Cancel",
            true,
        );
        if cancel || is_key_pressed(KeyCode::Escape) {
            break None;
        }
        if !slots.is_empty() && (load || double || is_key_pressed(KeyCode::Enter)) {
            break Some(cursor);
        }
        next_frame().await;
    };
    set_default_camera();
    answer
}

pub(super) async fn open(root: &Path) -> Result<(), Box<dyn Error>> {
    let slots = saved_game::slots(&saved_game::directory()?)?;
    let Some(i) = select(root, &slots).await else {
        return Ok(());
    };
    let (data, players) = saved_game::load(&slots[i], root)?;
    next_frame().await;
    show_board(data, players).await
}
