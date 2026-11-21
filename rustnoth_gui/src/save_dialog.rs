use crate::dialog;
use macroquad::prelude::*;
use std::path::Path;

pub fn save_pressed() -> bool {
    is_key_pressed(KeyCode::S)
        && (is_key_down(KeyCode::LeftControl) || is_key_down(KeyCode::RightControl))
}

use crate::dialog::Skin;

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
