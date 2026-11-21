use super::{Art, EDGE, LABEL, difficulty, image, outline};
use crate::{campaign_browser::modifications, dialog::fit, title_screen::{button_canvas::State, locale::Locale}};
use engine_lite::wml::Node;
use macroquad::prelude::*;
use std::{collections::HashSet, fs, io, path::Path};

struct Row<'a> {
    node: &'a Node,
    label: String,
    unavailable: Option<&'static str>,
    selected: bool,
}

pub(super) struct Menu<'a> {
    rows: Vec<Row<'a>>,
    cursor: Option<usize>,
    first: usize,
    held: bool,
    changed: bool,
}

impl<'a> Menu<'a> {
    pub fn new(catalog: &'a Node, locale: &mut Locale, root: &Path) -> Self {
        let mut ids = HashSet::new();
        let rows = catalog.children.iter().filter(|(tag, node)| tag == "modification" && modifications::eligible(node))
            .filter_map(|(_, node)| {
                let id = node.attribute("id")?;
                ids.insert(id).then(|| Row {
                    node, label: locale.attribute(root, node, "name", id), unavailable: modifications::unsupported(node),
                    selected: false,
                })
            }).collect();
        Self { rows, cursor: None, first: 0, held: false, changed: false }
    }

    pub fn restore(&mut self) -> io::Result<()> {
        if self.rows.is_empty() { return Ok(()); }
        let path = crate::display_preferences::config_directory()?.join("campaign-modifications");
        let saved = match fs::read_to_string(path) {
            Ok(ids) => ids,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e),
        };
        let selected: HashSet<_> = saved.lines().collect();
        for row in &mut self.rows {
            row.selected = row.unavailable.is_none() && row.node.attribute("id").is_some_and(|id| selected.contains(id));
        }
        Ok(())
    }

    pub fn remember(&mut self) -> io::Result<()> {
        if !self.changed { return Ok(()); }
        let dir = crate::display_preferences::config_directory()?;
        fs::create_dir_all(&dir)?;
        let ids: Vec<_> = self.rows.iter().filter(|r| r.selected).filter_map(|r| r.node.attribute("id")).collect();
        let temp = dir.join(format!("campaign-modifications.{}.tmp", std::process::id()));
        fs::write(&temp, ids.join("\n"))?;
        fs::rename(&temp, dir.join("campaign-modifications"))?;
        self.changed = false;
        Ok(())
    }

    pub fn is_open(&self) -> bool { self.cursor.is_some() }

    pub fn selected(&self) -> Vec<Node> {
        self.rows.iter().filter(|row| row.selected).map(|row| row.node.clone()).collect()
    }

    fn caption(&self, locale: &Locale) -> String {
        let selected: Vec<_> = self.rows.iter().filter(|r| r.selected).map(|r| r.label.as_str()).collect();
        let key = if self.rows.is_empty() { "active_modifications^None" }
            else if selected.is_empty() { "multimenu^None Selected" }
            else if selected.len() == self.rows.len() { "multimenu^All Selected" }
            else { return selected.join(", "); };
        locale.ui(key).rsplit('^').next().unwrap_or(key).to_owned()
    }

    pub fn draw(&mut self, art: &mut Art, left: f32, y: f32, width: f32, pointer: Vec2, view: Vec2, available: bool) {
        let label = art.locale.ui("Modifications:");
        let label_w = measure_text(label, art.font.as_ref(), 15, 1.).width.ceil();
        art.label(label, left, y + 20., 15, LABEL);
        let control = Rect::new(left + label_w + 10., y, width, 30.);
        let hot = available && control.contains(pointer);
        let clicked = is_mouse_button_released(MouseButton::Left);
        if is_mouse_button_pressed(MouseButton::Left) { self.held = hot; }
        let state = if self.rows.is_empty() { State::Disabled }
            else if self.is_open() || hot && self.held { State::Pressed }
            else if hot { State::Focused } else { State::Enabled };
        let caption = self.caption(&art.locale);
        difficulty::draw_control(art, control, state, &caption);
        if !available || self.rows.is_empty() { return; }
        let Some(mut cursor) = self.cursor else {
            if clicked && self.held && hot { self.cursor = Some(0); self.first = 0; }
            if clicked { self.held = false; }
            return;
        };

        let row_h = 28.;
        let below = view.y - control.bottom();
        let visible = (((control.y.max(below) - 2.) / row_h).floor() as usize).clamp(1, self.rows.len());
        let height = row_h * visible as f32 + 2.;
        let y = if below >= height { control.bottom() } else { control.y - height };
        let names: Vec<_> = self.rows.iter().map(|r| match r.unavailable {
            Some(reason) => format!("{} — {reason}", r.label),
            None => r.label.clone(),
        }).collect();
        let width = names.iter().map(|name| measure_text(name, art.font.as_ref(), 15, 1.).width + 40.)
            .fold(control.w, f32::max).ceil().min(view.x);
        let popup = Rect::new(control.x.min(view.x - width), y, width, height);
        art.tile("dialogs/menu-background.png", popup);
        outline(popup, EDGE);
        let wheel = if popup.contains(pointer) { mouse_wheel().1 } else { 0. };
        if is_key_pressed(KeyCode::Up) || wheel > 0. { cursor = cursor.saturating_sub(1); }
        if is_key_pressed(KeyCode::Down) || wheel < 0. { cursor = (cursor + 1).min(self.rows.len() - 1); }
        if is_key_pressed(KeyCode::Home) { cursor = 0; }
        if is_key_pressed(KeyCode::End) { cursor = self.rows.len() - 1; }
        self.first = self.first.clamp(cursor.saturating_sub(visible - 1), cursor);
        for index in self.first..(self.first + visible).min(self.rows.len()) {
            let rect = Rect::new(popup.x + 1., popup.y + 1. + (index-self.first) as f32 * row_h, popup.w - 2., row_h);
            let row = &mut self.rows[index];
            if cursor == index || rect.contains(pointer) {
                if let Some(tex) = art.get("dialogs/selection-background.png") { image(&tex, rect); }
                if cursor == index { outline(rect, EDGE); }
            }
            if clicked && rect.contains(pointer) {
                cursor = index;
                if row.unavailable.is_none() { row.selected = !row.selected; self.changed = true; }
            }
            let checkbox = if row.selected { "buttons/checkbox-pressed.png" } else { "buttons/checkbox.png" };
            if let Some(tex) = art.get(checkbox) {
                image(&tex, Rect::new(rect.x + 5., rect.center().y - tex.height()/2., tex.width(), tex.height()));
            }
            let name = fit(art.font.as_ref(), names[index].clone(), rect.w-33., 15);
            art.label(&name, rect.x + 30., rect.y + 19., 15, if row.unavailable.is_none() { LABEL } else { GRAY });
        }
        if is_key_pressed(KeyCode::Space) && self.rows[cursor].unavailable.is_none() {
            self.rows[cursor].selected = !self.rows[cursor].selected;
            self.changed = true;
        }
        self.cursor = if clicked && !popup.contains(pointer) || is_key_pressed(KeyCode::Escape) || is_key_pressed(KeyCode::Enter) {
            None
        } else { Some(cursor) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_single_player_modifications_produces_the_vanilla_caption() {
        let root = Path::new(".");
        let cfg = engine_lite::wml::parse("[modification]\nid=only_mp\ntype=mp\n[/modification]").unwrap();
        let mut locale = Locale::default();
        let menu = Menu::new(&cfg, &mut locale, root);
        assert!(menu.rows.is_empty());
        assert_eq!(menu.caption(&locale), "None");
        let cfg = engine_lite::wml::parse("[modification]\nid=bonus\n[/modification]\n[modification]\nid=bonus\n[/modification]").unwrap();
        let mut menu = Menu::new(&cfg, &mut locale, root);
        assert_eq!(menu.rows.len(), 1);
        assert_eq!(menu.caption(&locale), "None Selected");
        menu.rows[0].selected = true;
        assert_eq!(menu.caption(&locale), "All Selected");
        assert_eq!(menu.selected()[0].attribute("id"), Some("bonus"));
    }
}
