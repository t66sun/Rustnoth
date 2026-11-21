use super::{Art, AssetCatalog, BoardCamera, EditableMap, Hex, History, dialog};
use macroquad::prelude::*;
use std::cmp::Ordering;

pub(super) enum Action { Add, Delete, GoTo }

pub(super) struct Locations {
    ids: Vec<String>,
    selected: String,
    first: usize,
    visible: usize,
    click: bool,
}

fn numeric(id: &str) -> bool {
    id != "0" && !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit())
}

fn order(left: &str, right: &str) -> Ordering {
    match (numeric(left), numeric(right)) {
        (true, true) => {
            let a = left.trim_start_matches('0');
            let b = right.trim_start_matches('0');
            (a.len(), a, left).cmp(&(b.len(), b, right))
        }
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        _ => left.cmp(right),
    }
}

pub(super) fn label(id: &str) -> String {
    if numeric(id) { format!("Player {id}") } else { id.to_owned() }
}

impl Locations {
    pub fn new(doc: &EditableMap) -> Self {
        let mut ids: Vec<_> = (1..=9).map(|n| n.to_string()).collect();
        ids.extend(doc.locations().map(|(id, _)| id.to_owned()));
        ids.sort_by(|a, b| order(a, b));
        ids.dedup();
        Self { ids, selected: "1".into(), first: 0, visible: 1, click: false }
    }

    pub fn selected(&self) -> &str { &self.selected }

    pub fn add(&mut self, id: &str) -> bool {
        if id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') { return false; }
        self.select(id);
        true
    }

    fn select(&mut self, id: &str) {
        let index = match self.ids.binary_search_by(|item| order(item, id)) {
            Ok(index) => index,
            Err(index) => { self.ids.insert(index, id.into()); index }
        };
        self.selected = id.to_owned();
        if !(self.first..self.first + self.visible).contains(&index) {
            self.first = index.saturating_sub(self.visible / 2).min(self.ids.len().saturating_sub(self.visible));
        }
    }

    fn left(&mut self, at: Hex, ctrl: bool, doc: &mut EditableMap, history: &mut History) {
        if !doc.map().is_playable(at) { return; }
        let previous = doc.location_at(at);
        if ctrl {
            if let Some(id) = previous { self.select(id); }
        } else {
            let target = (previous != Some(self.selected())).then_some(at);
            history.place(doc, self.selected(), target);
        }
    }

    pub fn go_to(&self, doc: &EditableMap, camera: &mut BoardCamera) {
        if let Some(at) = doc.map().special_location(&self.selected) { center(camera, at); }
    }

    pub fn input(&mut self, at: Option<Hex>, ctrl: bool, doc: &mut EditableMap, history: &mut History, camera: &mut BoardCamera) {
        if is_mouse_button_pressed(MouseButton::Left) { self.click = at.is_some(); }
        if is_mouse_button_released(MouseButton::Left) && std::mem::take(&mut self.click) && let Some(at) = at {
            self.left(at, ctrl, doc, history);
        }
        if (is_mouse_button_released(MouseButton::Right) || is_key_released(KeyCode::Delete))
            && let Some(id) = at.and_then(|p| doc.location_at(p)).map(str::to_owned) {
            history.place(doc, &id, None);
        }
        let alt = is_key_down(KeyCode::LeftAlt) || is_key_down(KeyCode::RightAlt);
        let digits = [KeyCode::Key1, KeyCode::Key2, KeyCode::Key3, KeyCode::Key4, KeyCode::Key5,
            KeyCode::Key6, KeyCode::Key7, KeyCode::Key8, KeyCode::Key9];
        for (i, key) in digits.into_iter().enumerate() {
            if is_key_released(key) {
                let id = (i + 1).to_string();
                if alt {
                    if let Some(hex) = at
                        && doc.location_at(hex).is_none_or(|occupant| occupant == id) {
                        history.place(doc, &id, Some(hex));
                    }
                } else if let Some(hex) = doc.map().special_location(&id) {
                    center(camera, hex);
                }
            }
        }
    }

    pub fn panel(&mut self, panel: Rect, compact: bool, point: Vec2, assets: &AssetCatalog, art: &mut Art, font: Option<&Font>) -> Option<Action> {
        let target = Rect::new(panel.x + 15., panel.y + 40., 155., (panel.h - 40.).max(0.));
        let panel_top = Rect::new(panel.x, panel.y + 35., panel.w, 45.);
        art.image(assets, "themes/editor/classic/palette_top.png", panel_top);
        art.image(assets, "themes/editor/classic/palette.png", Rect::new(panel.x, panel_top.bottom(), panel.w, (panel.bottom() - panel_top.bottom()).max(0.)));
        self.visible = ((target.h - 90.) / 23.).floor().max(0.) as usize;
        let max_first = self.ids.len().saturating_sub(self.visible);
        let wheel = mouse_wheel().1;
        if panel.contains(point) && wheel != 0. {
            self.first = self.first.saturating_add_signed(if wheel > 0. { -1 } else { 1 });
        }
        self.first = self.first.min(max_first);
        let released = is_mouse_button_released(MouseButton::Left);
        let (groups, swap, side) = if compact { (12.,81.,25.) } else { (14.,78.,30.) };
        for (x, size, icon, active, direction) in [
            (groups, 30., String::new(), false, 0),
            (swap, side, format!("icons/action/editor-terrain-palette-swap_{side:.0}"), false, 0),
            (swap + side + 7., 25., "icons/arrows/arrows_ornate_up_25".into(), self.first > 0, -1),
            (swap + side + 39., 25., "icons/arrows/arrows_ornate_down_25".into(), self.first < max_first, 1),
        ] {
            let rect = Rect::new(panel.x + x, panel.y, size, size);
            let effect = if active { "" } else { "~GS()" };
            let state = if active && rect.contains(point) { "-active" } else { "" };
            art.image(assets, &format!("buttons/button_square/button_square_{size:.0}{state}.png{effect}"), rect);
            if !icon.is_empty() { art.image(assets, &format!("{icon}.png{effect}"), rect); }
            if active && released && rect.contains(point) {
                self.first = self.first.saturating_add_signed(direction).min(max_first);
            }
        }
        for (i, id) in self.ids.iter().skip(self.first).take(self.visible).enumerate() {
            let row = Rect::new(target.x, target.y + (i * 23) as f32, target.w, 20.);
            if row.contains(point) {
                draw_rectangle(row.x, row.y, row.w, row.h, Color::from_rgba(200,200,200,26));
                if released { self.selected.clone_from(id); }
            }
            if id == &self.selected { draw_rectangle_lines(row.x, row.y, row.w, row.h, 1., WHITE); }
            dialog::text(font, &label(id), row.x + 2., row.y + 16., 16, dialog::PALE);
        }
        for (i, (text, action)) in [("Delete", Action::Delete), ("Add", Action::Add), ("Go To", Action::GoTo)].into_iter().enumerate() {
            let r = Rect::new(target.x, panel.bottom() - 90. + i as f32 * 30., target.w - 10., 22.);
            let hot = r.contains(point);
            let suffix = if hot { "-active" } else { "" };
            art.image(assets, &format!("buttons/large-button{suffix}.png"), r);
            let w = measure_text(text, font, 16, 1.).width;
            dialog::text(font, text, r.center().x - w / 2., r.y + 16., 16, dialog::GOLD);
            if released && hot { return Some(action); }
        }
        None
    }
}

fn center(camera: &mut BoardCamera, at: Hex) {
    camera.center_world([at.x as f32 * 0.75 + 0.5, at.y as f32 + at.x.rem_euclid(2) as f32 * 0.5 + 0.5]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selecting_placing_and_displacing_named_starts_round_trips_map_and_history() {
        let source = "Gg, Gg, Gg, Gg\r\nGg, 2 Hh,  west_gate Gg ,Gg\r\nGg, Gg, Gg, Gg\r\n";
        let mut doc = EditableMap::parse(source.into()).unwrap();
        let mut p = Locations::new(&doc);
        let mut history = History::default();
        let left = Hex { x: 0, y: 0 };
        let right = Hex { x: 1, y: 0 };
        assert_eq!(p.ids[9], "west_gate");
        assert!(!p.add("invalid id"));
        p.left(right, true, &mut doc, &mut history);
        assert_eq!(p.selected(), "west_gate");
        p.left(left, false, &mut doc, &mut history);
        let saved = doc.text();
        let reopened = EditableMap::parse(saved.clone()).unwrap();
        assert_eq!(reopened.map().special_location("west_gate"), Some(left));
        assert_eq!(reopened.map().special_location("2"), None);
        history.apply(&mut doc, true);
        assert_eq!(doc.text(), source);
        assert!(!doc.dirty());
        history.apply(&mut doc, false);
        assert_eq!(doc.text(), saved);
        p.left(left, false, &mut doc, &mut history);
        assert_eq!(doc.location_at(left), None);
        assert!(p.add("10"));
        assert_eq!(&p.ids[9..], &["10", "west_gate"]);
    }
}
