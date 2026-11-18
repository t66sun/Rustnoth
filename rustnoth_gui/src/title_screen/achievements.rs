//! Title-screen browser for the installed Wesnoth 1.18.8 achievement groups.
mod catalog;
mod icons;

use super::{Art, centered, dialog, image, load_texture, preferences::tiled, wrap};
use catalog::{Group, Row};
use icons::Icons;
use macroquad::prelude::*;
use std::{error::Error, fs, io, path::Path};

struct Layout {
    text: Vec<String>,
    height: f32,
}

fn layout(row: &Row, width: f32, font: Option<&Font>) -> Layout {
    let text = wrap(&row.description, font, width);
    let cols = (width / 40.0).floor().max(1.0) as usize;
    let height = (34.0 + text.len() as f32 * 20.0 + if row.progress.is_some() { 20.0 } else { 0.0 }
        + row.subs.len().div_ceil(cols) as f32 * 40.0).max(82.0);
    Layout { text, height }
}

fn show_icon(icons: &mut Icons, icon: &str, complete: bool, cell: Rect) {
    if let Some(tex) = icons.get(icon, complete) {
        let size = vec2(cell.w.min(tex.width()), cell.h.min(tex.height()));
        image(Rect::new(cell.center().x - size.x * 0.5, cell.center().y - size.y * 0.5, size.x, size.y), tex);
    }
}

fn draw_row(row: &Row, layout: &Layout, cell: Rect, font: Option<&Font>, icons: &mut Icons, pointer: Vec2) -> Option<String> {
    let icon = Rect::new(cell.x + 5.0, cell.center().y - 36.0, 72.0, 72.0);
    show_icon(icons, &row.icon, row.completed, icon);
    let x = cell.x + 85.0;
    let width = cell.right() - x - 8.0;
    let title = if let Some((current, max)) = row.progress { format!("{} ({current}/{max})", row.name) } else { row.name.clone() };
    dialog::text(font, &dialog::fit(font, title, width, 18), x, cell.y + 23.0, 18, if row.completed { dialog::GOLD } else { WHITE });
    let mut y = cell.y + 43.0;
    for line in &layout.text {
        dialog::text(font, line, x, y, 16, if row.completed { Color::from_rgba(90,197,93,255) } else { dialog::PALE });
        y += 20.0;
    }
    if let Some((current, max)) = row.progress {
        draw_rectangle(x, y - 8.0, width, 10.0, Color::from_rgba(30, 38, 43, 255));
        draw_rectangle_lines(x, y - 8.0, width, 10.0, 1.0, Color::from_rgba(117, 98, 68, 255));
        draw_rectangle(x + 2.0, y - 6.0, (width - 4.0) * (current as f32 / max as f32).min(1.0), 6.0,
            Color::from_rgba(0, 55, 82, 255));
        y += 16.0;
    }
    let cols = (width / 40.0).floor().max(1.0) as usize;
    let mut hovered = None;
    for (i, (path, desc, done)) in row.subs.iter().enumerate() {
        let bounds = Rect::new(x + (i % cols) as f32 * 40.0, y - 9.0 + (i / cols) as f32 * 40.0, 36.0, 36.0);
        show_icon(icons, path, *done, bounds);
        if bounds.contains(pointer) { hovered = Some(desc.clone()); }
    }
    hovered
}

pub(super) async fn show(root: &Path, art: &Art, behind: impl Fn(f32, f32)) -> Result<(), Box<dyn Error>> {
    let installed = root.to_owned();
    let language = art.locale.effective.clone();
    let worker = std::thread::spawn(move || -> io::Result<Vec<Group>> {
        let bytes = crate::desktop::achievements(&installed)?;
        let config = engine_lite::wml::parse(bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        catalog::groups(&config, &catalog::read_profile()?, &installed, &language)
    });
    while !worker.is_finished() {
        behind(screen_width(), screen_height());
        dialog::text(art.font.as_ref(), art.locale.ui("Loading…"), 24.0, screen_height() - 28.0, 18, WHITE);
        next_frame().await;
    }
    let groups = worker.join().map_err(|_| "Achievement data preparation stopped")??;
    let config = crate::display_preferences::config_directory()?.join("achievement-group");
    let previous = match fs::read_to_string(&config) {
        Ok(id) => id,
        Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e.into()),
    };
    let mut selected = groups.iter().position(|g| g.id == previous).unwrap_or(0);
    let texture = load_texture(root, "images/dialogs/opaque-background.png")?;
    let highlight = load_texture(root, "images/dialogs/selection-background-fancy.png")?;
    let dropdown = load_texture(root, "images/buttons/button_dropdown/button_dropdown.png")?;
    let arrow = load_texture(root, "images/icons/arrows/short_arrow_ornate_left_25.png")?;
    let thumb = load_texture(root, "images/buttons/scrollbars/scrollmid.png")?;
    let font = art.font.as_ref();
    let mut icons = Icons::new(root);
    let mut offset = 0f32;
    let mut popup = None;
    let mut drag = None;
    let mut layouts = Vec::new();
    let mut cached = (usize::MAX, 0u32);
    let mut chosen_row = None;

    loop {
        let scale = (screen_width() / 660.0).min(screen_height() / 460.0).clamp(0.01, 1.0);
        let view = vec2(screen_width(), screen_height()) / scale;
        let pointer = Vec2::from(mouse_position()) / scale;
        let click = is_mouse_button_pressed(MouseButton::Left);
        set_camera(&dialog::setup_camera(view.x, view.y));
        behind(view.x, view.y);
        let panel = centered(view, vec2((view.x - 20.0).min(800.0), (view.y - 20.0).min(615.0)));
        tiled(panel, &texture);
        draw_rectangle_lines(panel.x, panel.y, panel.w, panel.h, 2.0, Color::from_rgba(114,79,46,255));
        dialog::text(font, art.locale.ui("Achievements"), panel.x + 15.0, panel.y + 37.0, 28, dialog::GOLD);
        let selector = Rect::new(panel.right() - 270.0, panel.y + 9.0, 258.0, 34.0);
        let list = Rect::new(panel.x + 8.0, selector.bottom() + 10.0, panel.w - 16.0, panel.h - 106.0);
        draw_rectangle(list.x, list.y, list.w, list.h, Color::from_rgba(15, 20, 25, 255));
        let close = Rect::new(panel.right() - 104.0, panel.bottom() - 43.0, 92.0, 32.0);
        let mut tip = None;
        if let Some(group) = groups.get(selected) {
            image(selector, &dropdown);
            draw_texture_ex(&arrow, selector.right() - 29.0, selector.center().y - 12.5, WHITE,
                DrawTextureParams { rotation: -std::f32::consts::FRAC_PI_2, ..Default::default() });
            dialog::text(font, &dialog::fit(font, group.name.clone(), selector.w - 42.0, 17),
                selector.x + 8.0, selector.center().y + 6.0, 17, dialog::PALE);
            let key = (selected, list.w.to_bits());
            let row_width = list.w - 16.0;
            if cached != key {
                layouts = group.rows.iter().map(|row| layout(row, row_width - 93.0, font)).collect();
                cached = key;
            }
            let total: f32 = layouts.iter().map(|l| l.height).sum();
            let max = (total - list.h).max(0.0);
            let rail = Rect::new(list.right() - 14.0, list.y, 14.0, list.h);
            let slider_height = if max > 0.0 { (list.h * list.h / total).max(26.0) } else { list.h };
            if popup.is_none() {
                if list.contains(pointer) { offset -= mouse_wheel().1 * 55.0; }
                for (key, amount) in [(KeyCode::Down, 42.0), (KeyCode::Up, -42.0), (KeyCode::PageDown, list.h), (KeyCode::PageUp, -list.h)] {
                    if is_key_pressed(key) { offset += amount; }
                }
                if is_key_pressed(KeyCode::Home) { offset = 0.0; }
                if is_key_pressed(KeyCode::End) { offset = max; }
                if max > 0.0 && click && rail.contains(pointer) {
                    let slider_y = rail.y + offset.clamp(0.0,max) / max * (rail.h - slider_height);
                    drag = Some(if (slider_y..slider_y+slider_height).contains(&pointer.y) {
                        pointer.y - slider_y
                    } else { slider_height / 2.0 });
                }
                if !is_mouse_button_down(MouseButton::Left) { drag = None; }
                if let Some(grab) = drag && max > 0.0 {
                    offset = (pointer.y - rail.y - grab) * max / (rail.h - slider_height);
                }
            }
            offset = offset.clamp(0.0, max);
            let dpi = screen_dpi_scale() * scale;
            unsafe { get_internal_gl().quad_gl.scissor(Some(((list.x*dpi) as i32, (list.y*dpi) as i32, (list.w*dpi) as i32, (list.h*dpi) as i32))); }
            let mut y = list.y - offset;
            for (i, (row, layout)) in group.rows.iter().zip(&layouts).enumerate() {
                let rect = Rect::new(list.x + 1.0, y, row_width, layout.height);
                y += layout.height;
                if rect.bottom() < list.y { continue; }
                if rect.y >= list.bottom() { break; }
                let hover = popup.is_none() && list.contains(pointer) && rect.contains(pointer);
                if hover && click { chosen_row = Some(i); }
                if chosen_row == Some(i) || hover { image(rect, &highlight); }
                if let Some(description) = draw_row(row, layout, rect, font, &mut icons, pointer) && hover {
                    tip = Some(description);
                }
            }
            unsafe { get_internal_gl().quad_gl.scissor(None); }
            if max > 0.0 {
                let slider = Rect::new(rail.x, rail.y + (rail.h - slider_height) * offset/max, rail.w, slider_height);
                image(slider, &thumb);
            }
            let count = format!("{} {}/{}", art.locale.ui("Completed"), group.completed, group.total);
            dialog::text(font, &count, panel.x + 13.0, close.center().y + 5.0, 16, dialog::PALE);
        }

        art.button(close, art.locale.ui("Close"), close.contains(pointer), close.contains(pointer) && is_mouse_button_down(MouseButton::Left));
        if let Some(mut cursor) = popup {
            if is_key_pressed(KeyCode::Up) { cursor = (cursor + groups.len() - 1) % groups.len(); }
            if is_key_pressed(KeyCode::Down) { cursor = (cursor + 1) % groups.len(); }
            let mut new_group = is_key_pressed(KeyCode::Enter).then_some(cursor);
            for (i, group) in groups.iter().enumerate() {
                let cell = Rect::new(selector.x, selector.bottom() + 30.0 * i as f32, selector.w, 30.0);
                tiled(cell, &texture);
                if cursor == i || cell.contains(pointer) { image(cell, &highlight); }
                dialog::text(font, &group.name, cell.x + 7.0, cell.y + 21.0, 16, dialog::PALE);
                if cell.contains(pointer) && click { new_group = Some(i); }
            }
            popup = Some(cursor);
            if let Some(i) = new_group { selected = i; offset = 0.0; chosen_row = None; popup = None; }
            else if click || is_key_pressed(KeyCode::Escape) { popup = None; }
        } else if (click && selector.contains(pointer) || is_key_pressed(KeyCode::Space)) && !groups.is_empty() {
            popup = Some(selected);
        } else if is_key_pressed(KeyCode::Escape) || is_key_pressed(KeyCode::Enter) || click && close.contains(pointer) {
            if let Some(group) = groups.get(selected) && group.id != previous {
                if let Some(dir) = config.parent() { fs::create_dir_all(dir)?; }
                fs::write(&config, &group.id)?;
            }
            set_default_camera();
            return Ok(());
        } else if let Some(body) = tip {
            let width = (measure_text(&body, font, 16, 1.0).width + 16.0).min(panel.w - 16.0);
            let lines = wrap(&body, font, width - 12.0);
            let height = 8.0 + 20.0 * lines.len() as f32;
            let r = Rect::new(pointer.x.min(view.x-width), (pointer.y+16.0).min(view.y-height), width, height);
            tiled(r, &texture);
            for (i, line) in lines.iter().enumerate() { dialog::text(font, line, r.x+6.0, r.y+18.0+i as f32*20.0, 16, dialog::PALE); }
        }
        set_default_camera();
        next_frame().await;
    }
}
