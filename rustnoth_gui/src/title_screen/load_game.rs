//! The local save browser described by GUI2's game_load.cfg.
mod preview;
mod widgets;

use super::{Art, dialog, image, preferences::tiled, version};
use crate::saved_game::{self, Slot};
use macroquad::prelude::*;
use preview::{Date, Preview};
use std::{error::Error, fs, path::Path, time::SystemTime};
use widgets::{Layout, Widgets};

#[derive(Copy, Clone, PartialEq)]
enum Sort { Recent, Oldest, Name, ReverseName }

fn visible(slots: &[Slot], filter: &str, sort: Sort) -> Vec<usize> {
    let query = filter.to_lowercase();
    let words: Vec<_> = query.split_whitespace().collect();
    let mut rows: Vec<_> = slots.iter().enumerate().filter_map(|(index, slot)| {
        let label = slot.label.to_lowercase();
        words.iter().all(|word| label.contains(word)).then_some(index)
    }).collect();
    match sort {
        Sort::Recent => {},
        Sort::Oldest => rows.reverse(),
        Sort::Name | Sort::ReverseName => {
            rows.sort_by_cached_key(|&i| slots[i].label.to_lowercase());
            if sort == Sort::ReverseName { rows.reverse(); }
        }
    }
    rows
}

pub(super) async fn show(root: &Path, art: &Art, behind: impl Fn(f32, f32)) -> Result<(), Box<dyn Error>> {
    let directory = saved_game::directory()?;
    let mut slots = saved_game::slots(&directory)?;
    if slots.is_empty() {
        widgets::empty(root, art, behind).await?;
        return Ok(());
    }
    let widgets = Widgets::new(root)?;
    let mut locale = super::locale::Locale::load(root, art.locale.selected.clone())?;
    let now = SystemTime::now();
    let mut dates: Vec<_> = slots.iter().map(|s| Date::for_slot(s, now)).collect();
    let mut filter = String::new();
    let mut sort = Sort::Recent;
    let mut rows = visible(&slots, &filter, sort);
    let mut cursor = 0usize;
    let mut first = 0usize;
    let mut editing = true;
    let mut select_all = false;
    let mut last_click = None::<(usize, f64)>;
    let mut selected = None;
    let mut preview: Option<Result<Preview, String>> = None;
    let mut watch = false;
    let mut deleting = false;
    let mut error = None::<String>;
    while get_char_pressed().is_some() {}
    loop {
        let scale = (screen_width() / 690.).min(screen_height() / 475.).clamp(0.01, 1.);
        let view = vec2(screen_width(), screen_height()) / scale;
        set_camera(&dialog::setup_camera(view.x, view.y));
        behind(view.x, view.y);
        let l = Layout::new(view);
        let mouse = Vec2::from(mouse_position()) / scale;
        let down = is_mouse_button_pressed(MouseButton::Left);
        let released = is_mouse_button_released(MouseButton::Left);
        let modal = deleting || error.is_some();
        let click = |r: Rect| !modal && released && r.contains(mouse);
        if !modal && down {
            editing = l.filter().contains(mouse);
            select_all = false;
        }
        let mut changed = false;
        if !modal && editing {
            let ctrl = is_key_down(KeyCode::LeftControl) || is_key_down(KeyCode::RightControl)
                || is_key_down(KeyCode::LeftSuper) || is_key_down(KeyCode::RightSuper);
            if ctrl && is_key_pressed(KeyCode::A) { select_all = true; }
            if is_key_pressed(KeyCode::Backspace) || (is_key_pressed(KeyCode::Delete) && select_all) {
                if select_all { filter.clear(); } else { filter.pop(); }
                select_all = false;
                changed = true;
            }
            while let Some(ch) = get_char_pressed() {
                if !ch.is_control() && !ctrl {
                    if select_all { filter.clear(); select_all = false; }
                    filter.push(ch);
                    changed = true;
                }
            }
        }
        if click(l.name_header()) {
            sort = if sort == Sort::Name { Sort::ReverseName } else { Sort::Name };
            changed = true;
        }
        if click(l.date_header()) {
            sort = if sort == Sort::Recent { Sort::Oldest } else { Sort::Recent };
            changed = true;
        }
        if changed {
            rows = visible(&slots, &filter, sort);
            cursor = selected.and_then(|i| rows.iter().position(|&n| n == i)).unwrap_or(0);
            first = 0;
            last_click = None;
        }
        let n = l.rows();
        let mut double_click = false;
        if !rows.is_empty() && !modal {
            let wheel = if l.list().contains(mouse) { mouse_wheel().1 } else { 0. };
            let step = if is_key_pressed(KeyCode::Down) || wheel < 0. { 1 }
                else if is_key_pressed(KeyCode::Up) || wheel > 0. { -1 }
                else if is_key_pressed(KeyCode::PageDown) { n as isize }
                else if is_key_pressed(KeyCode::PageUp) { -(n as isize) } else { 0 };
            cursor = cursor.saturating_add_signed(step).min(rows.len()-1);
            if !editing && is_key_pressed(KeyCode::Home) { cursor = 0; }
            if !editing && is_key_pressed(KeyCode::End) { cursor = rows.len()-1; }
            first = first.clamp(cursor.saturating_sub(n-1), cursor).min(rows.len().saturating_sub(n));
            if released && l.list().contains(mouse) {
                let pos = first + ((mouse.y - l.list().y) / 41.) as usize;
                if let Some(&slot) = rows.get(pos) {
                    cursor = pos;
                    double_click = last_click.is_some_and(|(i,t)| i == slot && get_time()-t < 0.4);
                    last_click = Some((slot, get_time()));
                }
            }
        }
        let new = rows.get(cursor).copied();
        if new != selected {
            selected = new;
            watch = false;
            preview = selected.map(|i| {
                crate::frame_trace::input("preview_begin");
                let result = Preview::read(root, &slots[i], &mut locale).map_err(|e| e.to_string());
                crate::frame_trace::ready("preview_end");
                result
            });
        }
        let valid = preview.as_ref().and_then(|p| p.as_ref().ok());
        let archive = valid.is_some_and(|p| p.replay);
        if click(l.watch()) && valid.is_some() && !archive { watch = !watch; }
        widgets.frame(&l, art, mouse, &filter, editing, select_all);
        for (line, &i) in rows.iter().skip(first).take(n).enumerate() {
            widgets.row(&l, art, line, &slots[i].label, &dates[i].short, selected == Some(i));
        }
        if let (Some(i), Some(p)) = (selected, valid) { widgets.preview(&l, art, p, &dates[i].long); }
        if let Some(Err(e)) = &preview { widgets.invalid(&l, art, e); }
        widgets.actions(&l, art, mouse, selected.is_some(), valid.is_some(), archive, watch);
        if modal {
            let text = error.as_deref().map(str::to_owned).unwrap_or_else(|| {
                art.locale.ui("Delete savegame?").to_owned() + " " + selected.map(|i| slots[i].label.as_str()).unwrap_or("")
            });
            let (yes, no) = widgets.prompt(&l, art, mouse, &text, deleting && error.is_none());
            if error.is_some() {
                if released && yes.contains(mouse) || is_key_pressed(KeyCode::Enter) || is_key_pressed(KeyCode::Escape) { error = None; }
            } else if released && no.contains(mouse) || is_key_pressed(KeyCode::Escape) { deleting = false; }
            else if released && yes.contains(mouse) {
                deleting = false;
                if let Some(i) = selected {
                    match fs::remove_dir_all(&slots[i].directory) {
                        Ok(()) => {
                            slots.remove(i); dates.remove(i);
                            rows = visible(&slots, &filter, sort);
                            cursor = cursor.min(rows.len().saturating_sub(1)); first = 0;
                            selected = None; preview = None; last_click = None;
                        }
                        Err(e) => error = Some(e.to_string()),
                    }
                }
            }
        } else if click(l.cancel()) || is_key_pressed(KeyCode::Escape) {
            set_default_camera();
            return Ok(());
        } else if valid.is_some() && (click(l.load()) || double_click || is_key_pressed(KeyCode::Enter)) {
            crate::frame_trace::input("load_input");
            let Some(Ok(mut p)) = preview.take() else { unreachable!() };
            if let Some(restore) = &mut p.data.restore { restore.watch_replay = watch; }
            set_default_camera();
            next_frame().await;
            return crate::show_board(p.data, p.players).await;
        } else if selected.is_some() && (click(l.delete()) || !editing && is_key_pressed(KeyCode::Delete)) {
            deleting = true;
        } else if click(l.folder()) {
            if let Err(e) = fs::create_dir_all(&directory).and_then(|()| version::path_open(&directory)) { error = Some(e.to_string()); }
        }
        next_frame().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn filter_matches_every_word_and_keeps_original_slot_indices_after_sorting() {
        let entries = ["West Turn 7", "West Replay", "East Turn 2"].map(|label| Slot {
            label: label.into(), directory: label.into(),
        });
        assert_eq!(visible(&entries, "TURN wEsT", Sort::Recent), vec![0]);
        assert_eq!(visible(&entries, "turn", Sort::Name), vec![2, 0]);
        assert!(visible(&entries, "east replay", Sort::Oldest).is_empty());
    }
}
