use super::{Art, Entry, fill};
use macroquad::prelude::*;
use std::cmp::Ordering;

/// The native Irdya calendar has four successive epochs and no year zero.
#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum Date {
    Dated(u8, i64),
    Undated,
}

impl Date {
    pub fn read(value: Option<&str>) -> Self {
        let mut parts = value.unwrap_or_default().split_whitespace();
        let Some(year) = parts.next().and_then(|s| s.parse::<u32>().ok()).filter(|&y| y > 0) else {
            return Self::Undated;
        };
        let year = i64::from(year);
        match parts.next() {
            Some("BW") => Self::Dated(0, -year),
            Some("BF") => Self::Dated(2, -year),
            Some("AF") => Self::Dated(3, year),
            _ => Self::Dated(1, year),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Field { Name, Timeline }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Order { Rank, Ascending(Field), Descending(Field) }

impl Order {
    pub fn toggle(self, field: Field) -> Self {
        match self {
            Self::Ascending(active) if active == field => Self::Descending(field),
            Self::Descending(active) if active == field => Self::Rank,
            _ => Self::Ascending(field),
        }
    }

    pub fn rows(self, entries: &[Entry<'_>], locale: &str) -> Vec<usize> {
        let mut rows: Vec<_> = (0..entries.len()).collect();
        let (Self::Ascending(field) | Self::Descending(field)) = self else { return rows };
        rows.sort_by(|&a, &b| {
            let cmp = match field {
                Field::Name => compare_names(&entries[a].name, &entries[b].name, locale),
                Field::Timeline => entries[a].date.cmp(&entries[b].date),
            };
            if matches!(self, Self::Descending(_)) { cmp.reverse() } else { cmp }
        });
        rows
    }
}

fn compare_names(a: &str, b: &str, _locale: &str) -> Ordering {
    #[cfg(windows)]
    {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn CompareStringEx(locale: *const u16, flags: u32, a: *const u16, len_a: i32,
                b: *const u16, len_b: i32, version: *const (), reserved: *const (), user: isize) -> i32;
        }
        let language = _locale.split('@').next().unwrap_or(_locale).replace('_', "-");
        let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
        let (language, left, right) = (wide(&language), wide(a), wide(b));
        // The strings are NUL-terminated and live through the synchronous call.
        let result = unsafe { CompareStringEx(language.as_ptr(), 1, left.as_ptr(), -1,
            right.as_ptr(), -1, std::ptr::null(), std::ptr::null(), 0) };
        if result != 0 { return result.cmp(&2); }
    }
    a.to_lowercase().cmp(&b.to_lowercase())
}

pub(super) fn headers(list: Rect) -> [(Field, Rect); 2] {
    let half = (list.w / 2.).floor();
    [(Field::Name, Rect::new(list.x, list.y, half, 30.)),
     (Field::Timeline, Rect::new(list.x + half, list.y, list.w - half, 30.))]
}

pub(super) fn header(art: &mut Art, field: Field, rect: Rect, order: Order, pointer: Vec2) {
    let active = order == Order::Ascending(field) || order == Order::Descending(field);
    let alpha = if rect.contains(pointer) { 76 } else if active { 25 } else { 0 };
    fill(rect, Color::from_rgba(255,255,255,alpha));
    let label = art.locale.ui(match field { Field::Name => "Name", Field::Timeline => "Timeline" });
    art.typography.draw(label, super::Caption::Header, rect, art.font.as_ref());
    if active && let Some(icon) = art.get("buttons/sliders/slider_arrow_blue.png") {
        let reversed = order == Order::Descending(field);
        draw_texture_ex(&icon, rect.right() - 15., rect.y + 10., WHITE,
            DrawTextureParams { flip_x: reversed, flip_y: reversed, ..Default::default() });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn campaign_headers_cycle_from_ascending_to_descending_to_original_rank() {
        let campaigns = [("Zebra", "3 YW"), ("beta", "4 BW"), ("Alpha", "45 BW"),
            ("Gamma", ""), ("Echo", "8 AF"), ("delta", "60 BF")].map(|(name, year)| Entry {
                name: name.into(), date: Date::read(Some(year)), scenario: 0,
                description: String::new(), icon: None, image: None, background: None, descriptor: None,
            });
        let name = Order::Rank.toggle(Field::Name);
        assert_eq!(name.rows(&campaigns, "en_US"), [2,1,5,4,3,0]);
        let asc = name.toggle(Field::Timeline);
        assert_eq!(asc.rows(&campaigns, "en_US"), [2,1,0,5,4,3]);
        let desc = asc.toggle(Field::Timeline);
        assert_eq!(desc.rows(&campaigns, "en_US"), [3,4,5,0,1,2]);
        assert_eq!(desc.toggle(Field::Timeline).rows(&campaigns, "en_US"), [0,1,2,3,4,5]);
    }
}
