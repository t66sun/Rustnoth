//! Display page of the native preferences window.
use super::{Art, centered, dialog, image, load_texture};
use crate::display_preferences;
use macroquad::prelude::*;
use std::{error::Error, path::Path};

pub(super) fn tiled(rect: Rect, texture: &Texture2D) {
    let mut y = rect.y;
    while y < rect.bottom() {
        let mut x = rect.x;
        while x < rect.right() {
            let w = texture.width().min(rect.right() - x);
            let h = texture.height().min(rect.bottom() - y);
            draw_texture_ex(texture, x, y, WHITE, DrawTextureParams {
                source: Some(Rect::new(0.0, 0.0, w, h)), ..Default::default()
            });
            x += w;
        }
        y += texture.height();
    }
}

pub(super) async fn show(
    root: &Path, art: &Art, fullscreen: &mut bool, windowed_size: &mut Vec2,
    draw_behind: impl Fn(f32, f32),
) -> Result<(), Box<dyn Error>> {
    let backdrop = load_texture(root, "images/dialogs/opaque-background.png")?;
    let icon = load_texture(root, "images/icons/icon-display.png")?;
    let checks = [
        load_texture(root, "images/buttons/checkbox.png")?,
        load_texture(root, "images/buttons/checkbox-active.png")?,
        load_texture(root, "images/buttons/checkbox-pressed.png")?,
        load_texture(root, "images/buttons/checkbox-active-pressed.png")?,
    ];
    let font = art.font.as_ref();
    let mut show_grid = display_preferences::grid()?;
    let mut focus = 0;
    let mut down = None;
    let mut last_pointer = mouse_position();
    let mut error = None;
    loop {
        let scale = (screen_width() / 660.0).min(screen_height() / 460.0).clamp(0.01, 1.0);
        let (w, h) = (screen_width() / scale, screen_height() / scale);
        set_camera(&dialog::setup_camera(w, h));
        draw_behind(w, h);
        let bounds = centered(vec2(w, h), vec2(600.0, 230.0));
        let left = bounds.x + 15.0;
        let top = bounds.y + 65.0;
        let gold = Color::from_rgba(186, 172, 125, 255);
        draw_rectangle(bounds.x, bounds.y, bounds.w, bounds.h, Color::from_rgba(16, 22, 35, 255));
        draw_rectangle_lines(bounds.x + 1.0, bounds.y + 1.0, bounds.w - 2.0, bounds.h - 2.0, 1.0,
            Color::from_rgba(114, 79, 46, 255));
        tiled(Rect::new(bounds.x + 2.0, bounds.y + 2.0, bounds.w - 4.0, bounds.h - 4.0), &backdrop);
        dialog::text(font, art.locale.ui("Preferences"), left, bounds.y + 41.0, 28, gold);
        let page = Rect::new(left, top, 150.0, 36.0);
        draw_rectangle(page.x, page.y, page.w, page.h, Color::from_rgba(41, 56, 63, 255));
        draw_rectangle_lines(page.x, page.y, page.w, page.h, 1.0, Color::from_rgba(114, 79, 46, 255));
        image(Rect::new(left + 6.0, top + 3.0, 30.0, 30.0), &icon);
        dialog::text(font, art.locale.ui("Display"), left + 43.0, top + 24.0, 17, gold);

        let checkbox = Rect::new(page.right() + 17.0, top + 3.0, 145.0, 30.0);
        let close = Rect::new(bounds.right() - 127.0, bounds.bottom() - 46.0, 110.0, 30.0);
        let (mx, my) = mouse_position();
        let pointer = vec2(mx / scale, my / scale);
        let grid_box = Rect::new(checkbox.x, checkbox.bottom() + 4.0, checkbox.w, checkbox.h);
        let hover = [checkbox, grid_box, close].iter().position(|r| r.contains(pointer));
        if last_pointer != (mx, my) {
            if let Some(index) = hover { focus = index; }
            last_pointer = (mx, my);
        }
        if is_key_pressed(KeyCode::Tab) || is_key_pressed(KeyCode::Up) || is_key_pressed(KeyCode::Down) {
            let backwards = is_key_pressed(KeyCode::Up) || is_key_down(KeyCode::LeftShift) || is_key_down(KeyCode::RightShift);
            focus = (focus + if backwards { 2 } else { 1 }) % 3;
        }
        for (i, (rect, checked, label)) in [(checkbox, *fullscreen, "Full screen"), (grid_box, show_grid, "Show grid")].into_iter().enumerate() {
            let texture = &checks[2 * usize::from(checked) + usize::from(focus == i)];
            let size = texture.size();
            image(Rect::new(rect.x, rect.y + (rect.h - size.y) * 0.5, size.x, size.y), texture);
            dialog::text(font, art.locale.ui(label), rect.x + size.x + 8.0, rect.y + 21.0, 17, gold);
        }
        if let Some(message) = &error {
            let message = dialog::fit(font, String::clone(message), bounds.w - 35.0, 15);
            dialog::text(font, &message, left, close.y - 10.0, 15, Color::from_rgba(255, 140, 135, 255));
        }
        art.button(close, art.locale.ui("Close"), focus == 2, down == Some(2) && hover == down);
        set_default_camera();

        if is_mouse_button_pressed(MouseButton::Left) { down = hover; }
        let activate = if is_mouse_button_released(MouseButton::Left) {
            down.take().filter(|pressed| Some(*pressed) == hover)
        } else if is_key_pressed(KeyCode::Space) || is_key_pressed(KeyCode::Enter) { Some(focus) } else { None };
        if is_key_pressed(KeyCode::Escape) || activate == Some(2) { return Ok(()); }
        if activate == Some(1) {
            match display_preferences::save_grid(!show_grid) {
                Ok(()) => { show_grid = !show_grid; error = None; }
                Err(cause) => error = Some(format!("Could not save grid preference: {cause}")),
            }
        }
        if activate == Some(0) {
            match display_preferences::save(!*fullscreen) {
                Ok(()) => {
                    if !*fullscreen { *windowed_size = vec2(screen_width(), screen_height()); }
                    *fullscreen = !*fullscreen;
                    set_fullscreen(*fullscreen);
                    if !*fullscreen { request_new_screen_size(windowed_size.x, windowed_size.y); }
                    error = None;
                }
                Err(cause) => error = Some(format!("Could not save window mode: {cause}")),
            }
        }
        next_frame().await;
    }
}
