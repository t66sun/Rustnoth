//! Read-only view of the add-ons installed in the native 1.18 profile.
mod catalog;

use super::{Art, achievements::icons, dialog, image, load_texture, preferences::tiled, profile, wrap};
use catalog::Addon;
use macroquad::prelude::*;
use std::{collections::HashMap, error::Error, fs, path::{Path, PathBuf}};

fn draw_icon(tex: Option<&Texture2D>, cell: Rect) {
    if let Some(tex) = tex {
        let size = tex.size().min(cell.size());
        image(Rect::new(cell.center().x - size.x / 2.0, cell.center().y - size.y / 2.0, size.x, size.y), tex);
    }
}

fn draw_details(art: &Art, item: &Addon, bounds: Rect, icon: Option<&Texture2D>, scroll: &mut usize, step: isize) {
    let font = art.font.as_ref();
    let x = bounds.x + 10.0;
    let width = bounds.w - 20.0;
    let header = Rect::new(x, bounds.y + 8.0, 72.0, 72.0);
    draw_icon(icon, header);
    let title = dialog::fit(font, item.title.clone(), width - 82.0, 24);
    dialog::text(font, &title, x + 82.0, bounds.y + 39.0, 24, dialog::GOLD);
    let mut lines = Vec::new();
    let id = item.folder.file_name().and_then(|id| id.to_str()).unwrap_or("");
    for (key, value) in [("Version:", &*item.version), ("Author", &*item.author), ("Id:", id)] {
        if !value.is_empty() {
            lines.extend(wrap(&format!("{}  {value}", art.locale.ui(key)), font, width));
        }
    }
    lines.push(String::new());
    lines.extend(wrap(if item.description.is_empty() { art.locale.ui("No description available.") } else { &item.description }, font, width));
    let visible = ((bounds.h - 95.0) / 21.0).floor().max(1.0) as usize;
    *scroll = scroll.saturating_add_signed(step).min(lines.len().saturating_sub(visible));
    for (i, line) in lines.iter().skip(*scroll).take(visible).enumerate() {
        dialog::text(font, line, x, bounds.y + 102.0 + i as f32 * 21.0, 16, dialog::PALE);
    }
}

fn icon<'a>(item: &Addon, root: &Path, colors: &[u8], cache: &'a mut HashMap<PathBuf, Option<Texture2D>>) -> Option<&'a Texture2D> {
    cache.entry(item.folder.clone()).or_insert_with(|| {
        let data = icons::pixels(&item.icon, colors, &|name| catalog::icon_file(root, &item.folder, name))?;
        let texture = Texture2D::from_rgba8(data.width().try_into().ok()?, data.height().try_into().ok()?, data.as_raw());
        texture.set_filter(FilterMode::Linear);
        Some(texture)
    }).as_ref()
}

pub(super) async fn show(root: &Path, art: &Art) -> Result<(), Box<dyn Error>> {
    let directory = profile::native_data_directory().ok_or("Cannot locate Wesnoth's user data directory")?.join("data/add-ons");
    let addons = catalog::installed(&directory, &art.locale.effective)?;
    let colors = fs::read(root.join("data/core/team-colors.cfg"))?;
    let background = load_texture(root, "images/dialogs/opaque-background.png")?;
    let highlight = load_texture(root, "images/dialogs/selection-background-fancy.png")?;
    let mut cache = HashMap::new();
    let (mut selected, mut first, mut detail_scroll) = (0usize, 0usize, 0usize);
    let mut detail_view = false;
    let font = art.font.as_ref();
    let installed_label = art.locale.ui("addon_state^Installed").rsplit('^').next().unwrap_or("Installed");
    loop {
        let scale = (screen_width() / 800.0).min(screen_height() / 600.0).clamp(0.01, 1.0);
        let view = vec2(screen_width(), screen_height()) / scale;
        let pointer = Vec2::from(mouse_position()) / scale;
        let click = is_mouse_button_pressed(MouseButton::Left);
        let wide = view.x >= 1281.0 && view.y >= 801.0;
        let list_shown = wide || !detail_view;
        set_camera(&dialog::setup_camera(view.x, view.y));
        tiled(Rect::new(0.0, 0.0, view.x, view.y), &background);
        dialog::text(font, art.locale.ui("Add-ons Manager"), 15.0, 43.0, 31, dialog::GOLD);
        let state = format!("{}  {}", art.locale.ui("State:"), installed_label);
        dialog::text(font, &state, 17.0, 76.0, 16, dialog::PALE);
        let body = Rect::new(15.0, 91.0, view.x - 30.0, view.y - 156.0);
        let list = Rect::new(body.x, body.y, if wide { body.w * 0.50 } else { body.w }, body.h);
        let detail = if wide { Rect::new(list.right() + 12.0, body.y, body.w - list.w - 12.0, body.h) } else { body };
        let step = if is_key_pressed(KeyCode::PageDown) { 5 } else if is_key_pressed(KeyCode::PageUp) { -5 }
            else if is_key_pressed(KeyCode::Down) { 1 } else if is_key_pressed(KeyCode::Up) { -1 } else { 0 };
        let wheel = if mouse_wheel().1 > 0.0 { -1 } else if mouse_wheel().1 < 0.0 { 1 } else { 0 };
        if addons.is_empty() {
            dialog::text(font, art.locale.game("You have no add-ons installed."), body.x + 10.0, body.y + 33.0, 18, dialog::PALE);
        } else if list_shown {
            let previous = selected;
            selected = selected.saturating_add_signed(step + if list.contains(pointer) { wheel } else { 0 }).min(addons.len() - 1);
            if is_key_pressed(KeyCode::Home) { selected = 0; }
            if is_key_pressed(KeyCode::End) { selected = addons.len() - 1; }
            let visible = ((list.h - 30.0) / 82.0).floor().max(1.0) as usize;
            first = first.clamp(selected.saturating_sub(visible - 1), selected).min(addons.len().saturating_sub(visible));
            let name_x = list.x + 82.0;
            let version_x = list.x + list.w * 0.60;
            let author_x = list.x + list.w * 0.75;
            for (label, x) in [("Name", name_x), ("Version", version_x), ("Author", author_x)] {
                dialog::text(font, art.locale.ui(label), x, list.y + 20.0, 16, dialog::GOLD);
            }
            for (index, item) in addons.iter().enumerate().skip(first).take(visible) {
                let row = Rect::new(list.x, list.y + 30.0 + (index - first) as f32 * 82.0, list.w, 82.0);
                if click && row.contains(pointer) { selected = index; }
                if selected == index || row.contains(pointer) { image(row, &highlight); }
                draw_icon(icon(item, root, &colors, &mut cache), Rect::new(row.x + 5.0, row.y + 5.0, 72.0, 72.0));
                let title = dialog::fit(font, item.title.clone(), version_x - name_x - 8.0, 18);
                dialog::text(font, &title, name_x, row.y + 36.0, 18, dialog::PALE);
                dialog::text(font, installed_label, name_x, row.y + 58.0, 14, dialog::MUTED);
                for (value, x, end) in [(&item.version, version_x, author_x), (&item.author, author_x, list.right())] {
                    dialog::text(font, &dialog::fit(font, value.clone(), end - x - 7.0, 16), x, row.center().y + 5.0, 16, dialog::PALE);
                }
            }
            if selected != previous { detail_scroll = 0; }
        }
        if (wide || detail_view) && let Some(item) = addons.get(selected) {
            let delta = if list_shown { 0 } else { step } + if detail.contains(pointer) { wheel } else { 0 };
            draw_details(art, item, detail, icon(item, root, &colors, &mut cache), &mut detail_scroll, delta);
        }
        let exit = Rect::new(view.x - 118.0, view.y - 47.0, 102.0, 32.0);
        art.button(exit, art.locale.ui("Exit"), exit.contains(pointer), false);
        if !wide && !addons.is_empty() {
            let toggle = Rect::new(15.0, exit.y, 172.0, exit.h);
            let label = if detail_view { "Back" } else { "Add-on Details" };
            art.button(toggle, art.locale.ui(label), toggle.contains(pointer), false);
            if is_key_pressed(KeyCode::Enter) || click && toggle.contains(pointer) { detail_view = !detail_view; }
        }
        set_default_camera();
        if click && exit.contains(pointer) { break; }
        if is_key_pressed(KeyCode::Escape) {
            if detail_view && !wide { detail_view = false; } else { break; }
        }
        next_frame().await;
    }
    Ok(())
}
