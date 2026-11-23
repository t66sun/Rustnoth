//! The native Unit List shows only units belonging to the viewing side.
use crate::{
    BoardData,
    dialog::{self, Skin},
    render::load_texture,
};
use engine_lite::{hex::Hex, observation::Observation};
use macroquad::prelude::*;
use rustnoth_gui::session::Session;
use std::cmp::Ordering;

struct Row {
    at: Hex,
    kind: String,
    name: String,
    moves: i32,
    max_moves: i32,
    hp: i32,
    max_hp: i32,
    level: i32,
    xp: i32,
    max_xp: i32,
    traits: String,
    states: [bool; 3],
    picture: Option<Texture2D>,
}

impl Row {
    fn order(&self, other: &Self, col: usize) -> Ordering {
        match col {
            0 => self.kind.cmp(&other.kind),
            1 => self.name.cmp(&other.name),
            2 => self.moves.cmp(&other.moves),
            4 => self.hp.cmp(&other.hp),
            5 => (self.level, self.xp - self.max_xp).cmp(&(other.level, other.xp - other.max_xp)),
            6 => self.xp.cmp(&other.xp),
            _ => self.traits.cmp(&other.traits),
        }
    }

    fn fields(&self) -> [String; 8] {
        [
            self.kind.clone(),
            self.name.clone(),
            format!("{}/{}", self.moves, self.max_moves),
            String::new(),
            format!("{}/{}", self.hp, self.max_hp),
            self.level.to_string(),
            format!("{}/{}", self.xp, self.max_xp),
            self.traits.clone(),
        ]
    }
}

pub(super) async fn show(
    data: &BoardData,
    session: &Session<'_>,
    view: &Observation,
    selected: Option<Hex>,
) -> Option<Hex> {
    let mut rows: Vec<_> = view
        .units
        .iter()
        .filter(|u| u.side == view.side)
        .filter_map(|unit| {
            let report = session.unit_report(unit.position, unit.position)?;
            let picture = data
                .assets
                .unit_portrait(&unit.type_id)
                .or_else(|| data.assets.unit_image(&unit.type_id))
                .and_then(|p| load_texture(&p, None));
            Some(Row {
                at: unit.position,
                kind: data
                    .assets
                    .unit_name(&unit.type_id)
                    .unwrap_or(&unit.type_id)
                    .into(),
                name: report.name.unwrap_or_default(),
                moves: unit.moves_left.unwrap_or(0),
                max_moves: report.max_movement,
                hp: unit.hitpoints,
                max_hp: unit.max_hitpoints,
                level: report.level,
                xp: unit.experience,
                max_xp: unit.max_experience,
                traits: report.traits.join(", "),
                states: [unit.slowed, unit.poisoned, unit.petrified],
                picture,
            })
        })
        .collect();
    if rows.is_empty() {
        return None;
    }
    let skin = Skin::load(&data.wesnoth_root);
    let states = ["slowed", "poisoned", "petrified"].map(|name| {
        data.assets
            .resolve_image(std::path::Path::new(&format!("misc/{name}.png")))
            .and_then(|path| load_texture(&path, None))
    });
    let back = dialog::Backdrop::capture(FilterMode::Linear);
    let mut cursor = rows
        .iter()
        .position(|r| Some(r.at) == selected)
        .unwrap_or(0);
    let mut first = 0usize;
    let mut sort = None;
    let mut previous = None;
    next_frame().await;
    loop {
        let (r, point) = skin.frame("Unit List", 1090.0, 538.0, back.texture());
        let click = is_mouse_button_pressed(MouseButton::Left);
        let table = Rect::new(r.x + 262.0, r.y + 65.0, r.w - 278.0, r.h - 126.0);
        let preview = Rect::new(r.x + 12.0, table.y, 240.0, table.h);
        let widths = [159., 112., 73., 61., 72., 44., 72., table.w - 593.];
        let mut x = table.x;
        let mut sorted_by = None;
        let labels = [
            "Type", "Name", "Moves", "Status", "HP", "Lvl", "XP", "Traits",
        ];
        let label = |s: &str, x: f32, y: f32, width: f32, size: u16, color: Color| {
            let text = dialog::fit(skin.font.as_ref(), s.into(), width.max(0.0), size);
            dialog::text(skin.font.as_ref(), &text, x, y, size, color);
        };
        for (i, (title, w)) in labels.iter().zip(widths).enumerate() {
            let head = Rect::new(x, table.y, w, 29.0);
            let caption = if let Some((col, desc)) = sort
                && col == i
            {
                format!("{} {}", title, if desc { "▼" } else { "▲" })
            } else {
                title.to_string()
            };
            label(&caption, x + 3.0, head.y + 19.0, w - 6.0, 15, dialog::GOLD);
            if i != 3 && click && head.contains(point) {
                sorted_by = Some(i);
            }
            x += w;
        }
        if let Some(col) = sorted_by {
            let desc = sort == Some((col, false));
            let preserve = rows[cursor].at;
            rows.sort_by(|a, b| {
                let cmp = a.order(b, col);
                if desc { cmp.reverse() } else { cmp }
            });
            cursor = rows.iter().position(|r| r.at == preserve).unwrap();
            sort = Some((col, desc));
        }
        let row_h = 31.0;
        let count = ((table.h - 29.0) / row_h) as usize;
        let wheel = if table.contains(point) {
            mouse_wheel().1
        } else {
            0.0
        };
        let step = if is_key_pressed(KeyCode::Down) || wheel < 0.0 {
            1
        } else if is_key_pressed(KeyCode::Up) || wheel > 0.0 {
            -1
        } else if is_key_pressed(KeyCode::PageDown) {
            count as isize
        } else if is_key_pressed(KeyCode::PageUp) {
            -(count as isize)
        } else {
            0
        };
        cursor = cursor.saturating_add_signed(step).min(rows.len() - 1);
        first = first
            .clamp(cursor.saturating_sub(count - 1), cursor)
            .min(rows.len().saturating_sub(count));
        let mut confirmed = is_key_pressed(KeyCode::Enter);
        for (j, row) in rows.iter().enumerate().skip(first).take(count) {
            let rect = Rect::new(
                table.x,
                table.y + 29.0 + (j - first) as f32 * row_h,
                table.w,
                row_h,
            );
            if click && rect.contains(point) {
                let now = get_time();
                confirmed |= previous.is_some_and(|(i, t)| i == j && now - t < 0.4);
                previous = Some((j, now));
                cursor = j;
            }
            if j == cursor {
                Skin::paint(&skin.selected, rect);
            }
            let mut left = rect.x + 3.0;
            for (i, (text, width)) in row.fields().iter().zip(widths).enumerate() {
                if i == 3 {
                    let mut dx = left;
                    for (flag, tex) in row.states.iter().zip(&states) {
                        if *flag {
                            Skin::paint(tex, Rect::new(dx, rect.y + 6.0, 18.0, 18.0));
                            dx += 16.0;
                        }
                    }
                } else {
                    label(text, left, rect.y + 21.0, width - 6.0, 15, WHITE);
                }
                left += width;
            }
        }
        let unit = &rows[cursor];
        if let Some(tex) = &unit.picture {
            let size = tex.size()
                * (preview.w / tex.width())
                    .min((preview.h - 104.0) / tex.height())
                    .min(1.0);
            draw_texture_ex(
                tex,
                preview.center().x - size.x * 0.5,
                preview.y + 4.0,
                WHITE,
                DrawTextureParams {
                    dest_size: Some(size),
                    ..Default::default()
                },
            );
        }
        label(
            &unit.name,
            preview.x + 6.0,
            preview.bottom() - 62.0,
            preview.w - 12.0,
            20,
            dialog::GOLD,
        );
        label(
            &unit.kind,
            preview.x + 6.0,
            preview.bottom() - 36.0,
            preview.w - 12.0,
            17,
            WHITE,
        );
        label(
            &format!(
                "Level {}   ({}, {})",
                unit.level,
                unit.at.x + 1,
                unit.at.y + 1
            ),
            preview.x + 6.0,
            preview.bottom() - 12.0,
            preview.w - 12.0,
            15,
            GRAY,
        );
        let close = Rect::new(r.right() - 119.0, r.bottom() - 43.0, 103.0, 31.0);
        let target = Rect::new(close.x - 125.0, close.y, 118.0, close.h);
        let go = skin.button(target, point, "Scroll To", true);
        let quit = skin.button(close, point, "Close", true);
        set_default_camera();
        if quit || is_key_pressed(KeyCode::Escape) {
            return None;
        }
        if go || confirmed {
            return Some(unit.at);
        }
        next_frame().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roster_sorts_numeric_hp_and_orders_equal_levels_by_experience_left() {
        let row = |hp, xp| Row {
            at: Hex { x: 0, y: 0 },
            kind: String::new(),
            name: String::new(),
            moves: 0,
            max_moves: 0,
            hp,
            max_hp: 40,
            level: 2,
            xp,
            max_xp: 60,
            traits: String::new(),
            states: [false; 3],
            picture: None,
        };
        let (a, b) = (row(9, 53), row(20, 12));
        assert_eq!(a.order(&b, 4), Ordering::Less);
        assert_eq!(a.order(&b, 5), Ordering::Greater);
        assert_eq!(a.fields()[6], "53/60");
    }
}
