//! Recruit Unit window, following the stock unit_recruit.cfg grid.
use std::{collections::HashMap, fs};

use engine_lite::{fixed_match::Action, unit_type::UnitType};
use macroquad::prelude::*;
use rustnoth_gui::{choices::ActionOption, team_colors::TeamColors};

use crate::{BoardData, dialog, render::load_texture};

pub(super) enum Result {
    Recruit(Action),
    Recall,
    Cancel,
}

struct Entry<'a> {
    action: &'a Action,
    name: &'a str,
    id: &'a str,
    kind: &'a UnitType,
    sprite: Option<Texture2D>,
}

fn filtered(rows: &[Entry<'_>], query: &str) -> Vec<usize> {
    let query = query.to_lowercase();
    rows.iter()
        .enumerate()
        .filter(|(_, row)| {
            row.name.to_lowercase().contains(&query) || row.id.to_lowercase().contains(&query)
        })
        .map(|(index, _)| index)
        .collect()
}

fn image(texture: &Texture2D, bounds: Rect, fit: bool) {
    let mut size = vec2(bounds.w, bounds.h);
    if fit {
        size = texture.size()
            * (bounds.w / texture.width())
                .min(bounds.h / texture.height())
                .min(1.0);
    }
    let origin = bounds.center() - size * 0.5;
    draw_texture_ex(
        texture,
        origin.x,
        origin.y,
        WHITE,
        DrawTextureParams {
            dest_size: Some(size),
            ..Default::default()
        },
    );
}

const GOLD: Color = Color::from_rgba(186, 172, 125, 255);
const LIGHT: Color = Color::from_rgba(221, 221, 221, 255);
const EDGE: Color = Color::from_rgba(114, 79, 46, 255);
const DARK: Color = Color::from_rgba(16, 22, 35, 255);

struct Art {
    panel: Option<Texture2D>,
    selected: Option<Texture2D>,
    hover: Option<Texture2D>,
    buttons: [Option<Texture2D>; 3],
    gold: Option<Texture2D>,
    font: Option<Font>,
}

impl Art {
    fn load(data: &BoardData) -> Self {
        let texture = |name: &str| load_texture(&data.wesnoth_root.join(name), None);
        Self {
            panel: texture("images/dialogs/opaque-background.png"),
            selected: texture("images/dialogs/selection2-background.png"),
            hover: texture("images/dialogs/selection-background.png"),
            buttons: [
                "background.png",
                "background-active.png",
                "background-pressed.png",
            ]
            .map(|name| texture(&format!("images/buttons/button_normal/{name}"))),
            gold: texture("data/core/images/themes/gold.png"),
            font: fs::read(data.wesnoth_root.join("fonts/Lato-Regular.ttf"))
                .ok()
                .and_then(|bytes| load_ttf_font_from_bytes(&bytes).ok()),
        }
    }

    fn label(&self, text: &str, x: f32, baseline: f32, size: u16, color: Color, width: f32) {
        let text = dialog::fit(self.font.as_ref(), text.to_owned(), width, size);
        dialog::text(self.font.as_ref(), &text, x, baseline, size, color);
    }

    fn button(&self, bounds: Rect, label: &str, mouse: Vec2, enabled: bool) -> bool {
        let hover = bounds.contains(mouse) && enabled;
        let index = if hover && is_mouse_button_down(MouseButton::Left) {
            2
        } else {
            usize::from(hover)
        };
        draw_rectangle(bounds.x, bounds.y, bounds.w, bounds.h, DARK);
        if let Some(texture) = &self.buttons[index] {
            image(texture, bounds, false);
        }
        draw_rectangle_lines(bounds.x, bounds.y, bounds.w, bounds.h, 2.0, EDGE);
        let metrics = measure_text(label, self.font.as_ref(), 16, 1.0);
        dialog::text(
            self.font.as_ref(),
            label,
            bounds.center().x - metrics.width * 0.5,
            bounds.center().y + metrics.offset_y - metrics.height * 0.5,
            16,
            if enabled { GOLD } else { GRAY },
        );
        hover && is_mouse_button_pressed(MouseButton::Left)
    }
}

pub(super) async fn show(data: &BoardData, options: &[ActionOption], side: usize) -> Result {
    let palette = fs::read(data.wesnoth_root.join("data/core/team-colors.cfg"))
        .ok()
        .and_then(|bytes| TeamColors::parse(&bytes).ok());
    let mut rows: Vec<_> = options
        .iter()
        .filter_map(|option| {
            let Action::Recruit { unit_type, .. } = &option.action else {
                return None;
            };
            let kind = data.scenario.unit_type(unit_type)?;
            let sprite = data
                .assets
                .unit_image(unit_type)
                .and_then(|path| load_texture(&path, palette.as_ref().map(|p| (p, side))));
            Some(Entry {
                action: &option.action,
                name: &option.label,
                id: unit_type,
                kind,
                sprite,
            })
        })
        .collect();
    rows.sort_by(|a, b| a.name.cmp(b.name).then(a.id.cmp(b.id)));
    if rows.is_empty() {
        return Result::Cancel;
    }
    let recall = options
        .iter()
        .any(|option| matches!(option.action, Action::Recall { .. }));
    let art = Art::load(data);
    let mut portraits = HashMap::new();
    let mut query = String::new();
    let mut matches = filtered(&rows, "");
    let mut cursor = 0usize;
    let mut first = 0usize;
    let mut editing = false;
    let mut last_click = None;
    let backdrop = dialog::Backdrop::capture(FilterMode::Linear);
    next_frame().await;
    loop {
        let scale = (screen_width() / 680.0)
            .min(screen_height() / 480.0)
            .clamp(0.01, 1.0);
        let (w, h) = (screen_width() / scale, screen_height() / scale);
        set_camera(&dialog::setup_camera(w, h));
        draw_texture_ex(
            backdrop.texture(),
            0.0,
            0.0,
            WHITE,
            DrawTextureParams {
                dest_size: Some(vec2(w, h)),
                flip_y: true,
                ..Default::default()
            },
        );
        let mouse = Vec2::from(mouse_position()) / scale;
        let clicked = is_mouse_button_pressed(MouseButton::Left);
        let width = (w - 16.0).min(840.0);
        let height = (h - 16.0).min(688.0);
        let panel = Rect::new((w - width) * 0.5, (h - height) * 0.5, width, height);
        let left = panel.x + 15.0;
        let split = panel.x + width * 0.52;
        let top = panel.y + 57.0;
        let filter = Rect::new(split, panel.y + 14.0, panel.right() - 15.0 - split, 30.0);
        let row_height = 67.0;
        let visible = ((height - 110.0) / row_height).floor().max(1.0) as usize;

        if clicked {
            editing = filter.contains(mouse);
        }
        if is_key_pressed(KeyCode::F)
            && (is_key_down(KeyCode::LeftControl) || is_key_down(KeyCode::RightControl))
        {
            editing = true;
        }
        let previous_query = query.clone();
        if editing && is_key_pressed(KeyCode::Backspace) {
            query.pop();
        }
        while let Some(c) = get_char_pressed() {
            if editing
                && !c.is_control()
                && !is_key_down(KeyCode::LeftControl)
                && !is_key_down(KeyCode::RightControl)
            {
                query.push(c);
            }
        }
        if query != previous_query {
            matches = filtered(&rows, &query);
            cursor = 0;
            first = 0;
            last_click = None;
        }
        let delta = if is_key_pressed(KeyCode::Down) {
            1
        } else if is_key_pressed(KeyCode::Up) {
            -1
        } else if is_key_pressed(KeyCode::PageDown) {
            visible as isize
        } else if is_key_pressed(KeyCode::PageUp) {
            -(visible as isize)
        } else {
            0
        };
        cursor = cursor
            .saturating_add_signed(delta)
            .min(matches.len().saturating_sub(1));
        if delta != 0 {
            editing = false;
        }
        first = first.clamp((cursor + 1).saturating_sub(visible), cursor);
        if mouse.x >= split && mouse.y >= top && mouse.y < panel.bottom() - 50.0 {
            let wheel = mouse_wheel().1;
            if wheel != 0.0 {
                first = first
                    .saturating_add_signed(-(wheel.signum() as isize))
                    .min(matches.len().saturating_sub(visible));
                cursor = cursor.clamp(
                    first,
                    (first + visible - 1).min(matches.len().saturating_sub(1)),
                );
            }
        }
        draw_rectangle(panel.x, panel.y, width, height, DARK);
        let inner = Rect::new(panel.x + 2.0, panel.y + 2.0, width - 4.0, height - 4.0);
        if let Some(bg) = &art.panel {
            let mut y = inner.y;
            while y < inner.bottom() {
                let mut x = inner.x;
                while x < inner.right() {
                    let iw = bg.width().min(inner.right() - x);
                    let ih = bg.height().min(inner.bottom() - y);
                    draw_texture_ex(
                        bg,
                        x,
                        y,
                        WHITE,
                        DrawTextureParams {
                            source: Some(Rect::new(0.0, 0.0, iw, ih)),
                            ..Default::default()
                        },
                    );
                    x += iw;
                }
                y += bg.height();
            }
        }
        draw_rectangle_lines(
            panel.x + 1.0,
            panel.y + 1.0,
            width - 2.0,
            height - 2.0,
            2.0,
            EDGE,
        );
        art.label(
            "Recruit Unit",
            left,
            panel.y + 38.0,
            27,
            GOLD,
            split - left - 6.0,
        );
        draw_rectangle(
            filter.x,
            filter.y,
            filter.w,
            filter.h,
            Color::from_rgba(5, 9, 14, 210),
        );
        draw_rectangle_lines(
            filter.x,
            filter.y,
            filter.w,
            filter.h,
            2.0,
            if editing { GOLD } else { EDGE },
        );
        art.label(
            if query.is_empty() { "Filter" } else { &query },
            filter.x + 7.0,
            filter.y + 21.0,
            16,
            LIGHT,
            filter.w - 14.0,
        );
        let mut confirm = is_key_pressed(KeyCode::Enter);
        for (i, &index) in matches.iter().enumerate().skip(first).take(visible) {
            let row = &rows[index];
            let rect = Rect::new(
                split,
                top + (i - first) as f32 * row_height,
                filter.w,
                row_height - 1.0,
            );
            let hovered = rect.contains(mouse);
            if clicked && hovered {
                confirm =
                    last_click.is_some_and(|(key, time)| key == index && get_time() - time < 0.35);
                last_click = Some((index, get_time()));
                cursor = i;
            }
            let glow = if i == cursor {
                art.selected.as_ref()
            } else if hovered {
                art.hover.as_ref()
            } else {
                None
            };
            if let Some(tex) = glow {
                image(tex, rect, false);
            }
            if i == cursor {
                draw_rectangle_lines(rect.x, rect.y, rect.w, rect.h, 1.0, GOLD);
            }
            if let Some(sprite) = &row.sprite {
                image(sprite, Rect::new(rect.x + 2.0, rect.y, 66.0, 66.0), true);
            }
            art.label(
                row.name,
                rect.x + 73.0,
                rect.y + 25.0,
                18,
                LIGHT,
                rect.w - 79.0,
            );
            if let Some(gold) = &art.gold {
                image(
                    gold,
                    Rect::new(rect.x + 73.0, rect.y + 34.0, 19.0, 19.0),
                    true,
                );
            }
            art.label(
                &row.kind.cost.to_string(),
                rect.x + 98.0,
                rect.y + 50.0,
                16,
                GOLD,
                rect.w - 105.0,
            );
        }
        if matches.is_empty() {
            art.label(
                "No matching units",
                split + 6.0,
                top + 25.0,
                17,
                LIGHT,
                filter.w - 12.0,
            );
        }
        if let Some(row) = matches.get(cursor).map(|&index| &rows[index]) {
            let portrait = portraits.entry(row.id).or_insert_with(|| {
                data.assets
                    .dialogue_portrait(row.id)
                    .or_else(|| data.assets.unit_portrait(row.id))
                    .and_then(|path| load_texture(&path, None))
            });
            let preview = Rect::new(left, top, split - left - 12.0, (height * 0.36).min(244.0));
            if let Some(tex) = portrait.as_ref().or(row.sprite.as_ref()) {
                image(tex, preview, true);
            }
            let print = |value: &str, y: f32, size, color| {
                art.label(value, left, y, size, color, preview.w)
            };
            let y = preview.bottom() + 22.0;
            print(row.name, y, 23, GOLD);
            print(
                &format!("Level {}   ·   {:?}", row.kind.level, row.kind.alignment),
                y + 23.0,
                17,
                LIGHT,
            );
            print(
                &format!(
                    "HP {}     XP {}     MP {}",
                    row.kind.hitpoints, row.kind.experience, row.kind.movement
                ),
                y + 49.0,
                17,
                LIGHT,
            );
            for (i, weapon) in row.kind.attacks.iter().enumerate() {
                let baseline = y + 83.0 + i as f32 * 22.0;
                if baseline > panel.bottom() - 56.0 {
                    break;
                }
                let name = data.assets.weapon_name(row.id, i).unwrap_or(&weapon.id);
                print(
                    &format!(
                        "{name}   {} × {}   {}  {}",
                        weapon.damage, weapon.strikes, weapon.range, weapon.damage_type
                    ),
                    baseline,
                    16,
                    LIGHT,
                );
            }
        }
        let footer = panel.bottom() - 42.0;
        confirm |= art.button(
            Rect::new(panel.right() - 213.0, footer, 95.0, 31.0),
            "Recruit",
            mouse,
            !matches.is_empty(),
        );
        let cancel = art.button(
            Rect::new(panel.right() - 108.0, footer, 95.0, 31.0),
            "Cancel",
            mouse,
            true,
        );
        let choose_recall =
            recall && art.button(Rect::new(left, footer, 100.0, 31.0), "Recall…", mouse, true);
        set_default_camera();
        if cancel || is_key_pressed(KeyCode::Escape) || is_mouse_button_pressed(MouseButton::Right)
        {
            return Result::Cancel;
        }
        if choose_recall || recall && is_key_pressed(KeyCode::R) && !editing {
            return Result::Recall;
        }
        if confirm && let Some(&index) = matches.get(cursor) {
            return Result::Recruit(rows[index].action.clone());
        }
        next_frame().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_lite::{hex::Hex, wml};

    #[test]
    fn filtered_recruit_uses_the_exact_referee_action_and_catalog_price() {
        let cfg = wml::parse(b"[unit_type]\nid=Spearman\ncost=14\n[/unit_type]").unwrap();
        let unit = UnitType::from_wml(cfg.children.into_iter().next().unwrap().1);
        let action = Action::Recruit {
            unit_type: unit.id.clone(),
            to: Hex { x: 7, y: 4 },
        };
        let row = Entry {
            action: &action,
            name: "City Guard",
            id: &unit.id,
            kind: &unit,
            sprite: None,
        };
        let rows = [row];
        let result = filtered(&rows, "GUARD");
        assert_eq!(result, [0]);
        assert_eq!(rows[result[0]].action, &action);
        assert_eq!(rows[result[0]].kind.cost, 14);
        assert_eq!(filtered(&rows, "spear"), [0]);
        assert!(filtered(&rows, "archer").is_empty());
    }
}
