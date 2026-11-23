use super::{
    Field, Setup,
    art::{Art, GOLD, LABEL, edge, fill, image},
    metadata,
};
use crate::{dialog::setup_camera, render::minimap};
use engine_lite::map::ScenarioMap;
use macroquad::prelude::*;
use rustnoth_gui::{assets::AssetCatalog, session::Players};
use std::{collections::HashMap, error::Error, path::Path};

pub(super) enum Input {
    Choose(Field, usize),
    Start(Players),
    Exit,
    Idle,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Create,
    Lobby,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    General,
    Settings,
}
struct Picker {
    field: Field,
    cursor: usize,
    first: usize,
}
struct MapPreview {
    index: usize,
    size: f32,
    dimensions: Option<(i32, i32)>,
    texture: Option<Texture2D>,
}

pub(super) struct SetupView {
    art: Art,
    assets: AssetCatalog,
    descriptions: HashMap<String, metadata::Description>,
    page: Page,
    tab: Tab,
    players: Players,
    focus: usize,
    first: usize,
    search: String,
    editing: bool,
    picker: Option<Picker>,
    preview_art: minimap::Art,
    preview: Option<MapPreview>,
    lobby_first: usize,
}

struct Controls {
    w: f32,
    h: f32,
    mouse: Vec2,
    click: bool,
    confirm: bool,
}
impl Controls {
    fn begin() -> Self {
        let scale = (screen_width() / 800.0)
            .min(screen_height() / 600.0)
            .clamp(0.01, 1.0);
        let (w, h) = (screen_width() / scale, screen_height() / scale);
        set_camera(&setup_camera(w, h));
        let (x, y) = mouse_position();
        Self {
            w,
            h,
            mouse: vec2(x / scale, y / scale),
            click: is_mouse_button_pressed(MouseButton::Left),
            confirm: is_key_pressed(KeyCode::Enter) || is_key_pressed(KeyCode::Space),
        }
    }
    fn clicked(&self, r: Rect) -> bool {
        self.click && r.contains(self.mouse)
    }
}

fn scroll_to(index: usize, first: usize, rows: usize, count: usize) -> usize {
    first
        .clamp(index.saturating_sub(rows - 1), index)
        .min(count.saturating_sub(rows))
}

impl SetupView {
    pub(super) fn new(root: &Path, core: &[u8]) -> Result<Self, Box<dyn Error>> {
        Ok(Self {
            art: Art::new(root),
            assets: AssetCatalog::for_setup(root, core)?,
            descriptions: metadata::read_all(root),
            page: Page::Create,
            tab: Tab::General,
            players: Players::HotSeat,
            focus: 0,
            first: 0,
            search: String::new(),
            editing: false,
            picker: None,
            preview_art: minimap::Art::new(root),
            preview: None,
            lobby_first: 0,
        })
    }

    fn map_name<'a>(&'a self, setup: &'a Setup, index: usize) -> &'a str {
        let key = &setup.maps[index].name;
        self.descriptions
            .get(key)
            .map_or(key.as_str(), |m| m.title.as_str())
    }

    pub(super) fn frame(&mut self, setup: &Setup, error: Option<&str>) -> Input {
        let c = Controls::begin();
        self.art.backdrop(c.w, c.h);
        let modal = self.picker.is_some();
        let mut result = match self.page {
            Page::Create => self.create(setup, &c, error),
            Page::Lobby => self.lobby(setup, &c, error),
        };
        if modal {
            result = self.pick(setup, &c);
        }
        set_default_camera();
        result
    }

    fn tab_focus(&mut self, count: usize) {
        if is_key_pressed(KeyCode::Tab) {
            self.editing = false;
            let reverse = is_key_down(KeyCode::LeftShift) || is_key_down(KeyCode::RightShift);
            self.focus = (self.focus + if reverse { count - 1 } else { 1 }) % count;
        }
    }

    fn create(&mut self, setup: &Setup, c: &Controls, error: Option<&str>) -> Input {
        let mut result = Input::Idle;
        let active = self.picker.is_none();
        let compact = c.w < 1200.0 || c.h < 700.0;
        let pad = 16.0;
        let top = if compact { 99.0 } else { 63.0 };
        let bottom = c.h - 70.0;
        let left = Rect::new(
            pad,
            top,
            if compact {
                (c.w - 40.0) * 0.43
            } else {
                (c.w - 64.0) * 0.27
            },
            bottom - top,
        );
        let list = Rect::new(
            left.right() + 12.0,
            top,
            if compact {
                c.w - left.right() - 28.0
            } else {
                (c.w - 64.0) * 0.30
            },
            bottom - top,
        );
        let options = if compact {
            Rect::new(pad, top, c.w - pad * 2.0, bottom - top)
        } else {
            Rect::new(
                list.right() + 18.0,
                top,
                c.w - list.right() - 34.0,
                bottom - top,
            )
        };
        let show_maps = !compact || self.tab == Tab::General;
        self.art
            .label("Create Game", pad + 2.0, 40.0, 32, GOLD, c.w - 40.0);
        if active {
            self.tab_focus(6);
        }
        let tabs_x = if compact { pad } else { options.x };
        let tabs_y = if compact { 52.0 } else { options.y };
        for (idx, (tab, name)) in [(Tab::General, "General"), (Tab::Settings, "Game Settings")]
            .into_iter()
            .enumerate()
        {
            let r = Rect::new(tabs_x + idx as f32 * 136.0, tabs_y, 134.0, 34.0);
            self.art.button(r, name, c.mouse, self.tab == tab);
            if active && (c.clicked(r) || self.focus == 1 && c.confirm && self.tab != tab) {
                self.tab = tab;
                self.editing = false;
                if c.clicked(r) {
                    self.focus = 1;
                    break;
                }
                break;
            }
        }
        if show_maps {
            if let Some(change) = self.map_list(setup, c, list, active) {
                result = change;
            }
            self.map_details(setup, left, if c.w <= 1280.0 { 200.0 } else { 275.0 });
        }
        // On the native small layout, game settings replace the map-selection page.
        if !compact || self.tab == Tab::Settings {
            let y = if compact {
                options.y + 8.0
            } else {
                tabs_y + 66.0
            };
            if let Some(change) = self.settings(setup, c, options.x, y, options.w, active) {
                result = change;
            }
        }
        if compact && self.tab == Tab::General {
            self.art.label(
                "Default era",
                left.x,
                left.bottom() - 6.0,
                16,
                LABEL,
                left.w,
            );
        }
        let cancel = Rect::new(c.w - 124.0, c.h - 54.0, 106.0, 36.0);
        let create = Rect::new(cancel.x - 150.0, cancel.y, 140.0, 36.0);
        self.art
            .button(create, "Create Game", c.mouse, self.focus == 4);
        self.art.button(cancel, "Cancel", c.mouse, self.focus == 5);
        if let Some(message) = error.or(setup.maps[setup.map]
            .board
            .as_ref()
            .err()
            .map(String::as_str))
        {
            self.art
                .label(message, pad, c.h - 28.0, 16, ORANGE, create.x - pad * 2.0);
        }
        if active {
            if is_key_pressed(KeyCode::Escape) {
                if self.editing {
                    self.editing = false;
                    self.search.clear();
                } else {
                    return Input::Exit;
                }
            }
            if c.clicked(cancel) || (!self.editing && self.focus == 5 && c.confirm) {
                return Input::Exit;
            }
            if matches!(result, Input::Idle)
                && !setup.sides.is_empty()
                && (c.clicked(create)
                    || (!self.editing && c.confirm && matches!(self.focus, 0 | 4)))
            {
                self.page = Page::Lobby;
                self.focus = 0;
                self.editing = false;
            }
        }
        result
    }

    fn map_details(&mut self, setup: &Setup, area: Rect, target: f32) {
        let title = self.map_name(setup, setup.map).to_owned();
        self.art
            .label(&title, area.x + 3.0, area.y + 21.0, 21, GOLD, area.w - 6.0);
        let size = target.min(area.w - 10.0);
        if self
            .preview
            .as_ref()
            .is_none_or(|p| p.index != setup.map || p.size != size)
        {
            let map = setup.maps[setup.map]
                .board
                .as_ref()
                .ok()
                .and_then(|(text, _)| ScenarioMap::parse(text).ok());
            self.preview = Some(MapPreview {
                index: setup.map,
                size,
                dimensions: map.as_ref().map(|map| (map.width(), map.height())),
                texture: map.as_ref().and_then(|map| self.preview_art.skirmish_preview(&self.assets, map, size)),
            });
        }
        let r = Rect::new(area.x + 3.0, area.y + 37.0, size + 4.0, size + 4.0);
        fill(r, Color::from_rgba(12, 15, 20, 230));
        edge(r);
        if let Some(preview) = &self.preview {
            if let Some(tex) = &preview.texture {
                let xy = (r.center() - tex.size() * 0.5).round();
                draw_texture(tex, xy.x, xy.y, WHITE);
            }
            if let Some((width, height)) = preview.dimensions {
                self.art.label(
                    &format!("Map size:   {width} × {height}"),
                    area.x + 4.0,
                    r.bottom() + 59.0,
                    17,
                    LABEL,
                    area.w - 8.0,
                );
            }
        }
        self.art.label(
            &format!("Players:     {}", setup.sides.len()),
            area.x + 4.0,
            r.bottom() + 32.0,
            17,
            LABEL,
            area.w - 8.0,
        );
        let body = self
            .descriptions
            .get(&setup.maps[setup.map].name)
            .map(|m| m.body.as_str())
            .unwrap_or("");
        self.art.wrapped(
            body,
            Rect::new(
                area.x + 4.0,
                r.bottom() + 79.0,
                area.w - 12.0,
                (area.bottom() - r.bottom() - 110.0).max(0.0),
            ),
        );
    }

    fn map_list(&mut self, setup: &Setup, c: &Controls, area: Rect, active: bool) -> Option<Input> {
        let search = Rect::new(area.x, area.y, area.w, 32.0);
        if active && c.click {
            self.editing = search.contains(c.mouse);
            if self.editing {
                self.focus = 0;
            }
        }
        if active
            && (is_key_down(KeyCode::LeftControl) || is_key_down(KeyCode::RightControl))
            && is_key_pressed(KeyCode::F)
        {
            self.editing = true;
            self.focus = 0;
        }
        if active && self.editing {
            while let Some(ch) = get_char_pressed() {
                if !ch.is_control() {
                    self.search.push(ch);
                }
            }
            if is_key_pressed(KeyCode::Backspace) {
                self.search.pop();
            }
        }
        fill(search, Color::from_rgba(12, 17, 24, 231));
        edge(search);
        let caption = if self.search.is_empty() {
            if self.editing { "|" } else { "Filter" }
        } else {
            self.search.as_str()
        };
        self.art.label(
            caption,
            search.x + 8.0,
            search.y + 22.0,
            16,
            LABEL,
            area.w - 16.0,
        );
        self.art.label(
            "Multiplayer Scenarios",
            area.x + 5.0,
            area.y + 61.0,
            18,
            GOLD,
            area.w - 10.0,
        );
        let list = Rect::new(area.x, area.y + 72.0, area.w, area.h - 72.0);
        fill(list, Color::from_rgba(10, 16, 25, 206));
        edge(list);
        let query = self.search.to_lowercase();
        let maps: Vec<_> = (0..setup.maps.len())
            .filter(|&n| self.map_name(setup, n).to_lowercase().contains(&query))
            .collect();
        let rows = ((list.h - 4.0) / 32.0).floor().max(1.0) as usize;
        let selected = maps.iter().position(|n| *n == setup.map);
        self.first = self.first.min(maps.len().saturating_sub(rows));
        if active && list.contains(c.mouse) && mouse_wheel().1 != 0.0 {
            self.first = self
                .first
                .saturating_add_signed(if mouse_wheel().1 > 0.0 { -3 } else { 3 })
                .min(maps.len().saturating_sub(rows));
        }
        if active && self.focus == 0 && !maps.is_empty() {
            let delta = if is_key_pressed(KeyCode::Down) {
                1
            } else if is_key_pressed(KeyCode::Up) {
                -1
            } else if is_key_pressed(KeyCode::PageDown) {
                rows as isize
            } else if is_key_pressed(KeyCode::PageUp) {
                -(rows as isize)
            } else {
                0
            };
            if delta != 0 {
                let next =
                    selected.map_or(0, |i| i.saturating_add_signed(delta).min(maps.len() - 1));
                self.first = scroll_to(next, self.first, rows, maps.len());
                return Some(Input::Choose(Field::Map, maps[next]));
            }
            if self.editing && is_key_pressed(KeyCode::Enter) {
                self.editing = false;
                self.focus = 4;
                return Some(Input::Choose(Field::Map, maps[0]));
            }
        }
        for (row, &idx) in maps.iter().skip(self.first).take(rows).enumerate() {
            let r = Rect::new(
                list.x + 2.0,
                list.y + 2.0 + row as f32 * 32.0,
                list.w - 4.0,
                31.0,
            );
            self.art.row(r, idx == setup.map, r.contains(c.mouse));
            let name = self.map_name(setup, idx).to_owned();
            self.art
                .label(&name, r.x + 9.0, r.y + 22.0, 16, LABEL, r.w - 14.0);
            if active && c.clicked(r) {
                self.focus = 0;
                return Some(Input::Choose(Field::Map, idx));
            }
        }
        if maps.is_empty() {
            self.art.label(
                "No matches",
                list.x + 8.0,
                list.y + 25.0,
                16,
                LABEL,
                list.w - 16.0,
            );
        }
        None
    }

    fn settings(
        &mut self,
        setup: &Setup,
        c: &Controls,
        x: f32,
        y: f32,
        w: f32,
        active: bool,
    ) -> Option<Input> {
        if self.tab == Tab::General {
            self.art
                .label("Eras:", x + 4.0, y + 18.0, 18, LABEL, w - 8.0);
            self.art
                .label("Default", x + 4.0, y + 48.0, 18, GOLD, w - 8.0);
            self.art
                .label("Players", x + 4.0, y + 108.0, 18, LABEL, w - 8.0);
            self.mode_buttons(c, x, y + 121.0, w, active, 2);
            return None;
        }
        self.art.label(
            &format!(
                "Turns:   {}",
                setup.label(Field::Rounds, setup.rounds_index)
            ),
            x + 4.0,
            y + 20.0,
            18,
            LABEL,
            w - 8.0,
        );
        let slider = Rect::new(x + 7.0, y + 34.0, w - 14.0, 31.0);
        let left = slider.x + 9.0;
        let length = slider.w - 18.0;
        draw_line(
            left,
            slider.center().y,
            left + length,
            slider.center().y,
            3.0,
            Color::from_rgba(125, 117, 101, 255),
        );
        let knob_x =
            left + length * setup.rounds_index as f32 / (setup.count(Field::Rounds) - 1) as f32;
        if let Some(knob) = self.art.get("buttons/sliders/slider.png") {
            draw_texture(
                &knob,
                knob_x - knob.width() * 0.5,
                slider.center().y - knob.height() * 0.5,
                WHITE,
            );
        }
        if self.focus == 2 {
            edge(slider);
        }
        if active && slider.contains(c.mouse) && is_mouse_button_down(MouseButton::Left) {
            self.focus = 2;
            let at = ((c.mouse.x - left) / length).clamp(0.0, 1.0);
            let index = (at * (setup.count(Field::Rounds) - 1) as f32).round() as usize;
            if index != setup.rounds_index {
                return Some(Input::Choose(Field::Rounds, index));
            }
        }
        if active && self.focus == 2 {
            if is_key_pressed(KeyCode::Left) {
                return Some(Input::Choose(
                    Field::Rounds,
                    setup.rounds_index.saturating_sub(1),
                ));
            }
            if is_key_pressed(KeyCode::Right) {
                return Some(Input::Choose(
                    Field::Rounds,
                    (setup.rounds_index + 1).min(setup.count(Field::Rounds) - 1),
                ));
            }
        }
        for (i, (bit, label)) in [(1, "Fog of War"), (2, "Shroud")].into_iter().enumerate() {
            let r = Rect::new(x + 4.0, y + 91.0 + 35.0 * i as f32, w - 8.0, 30.0);
            let icon = if setup.visibility & bit != 0 {
                "buttons/checkbox-pressed.png"
            } else {
                "buttons/checkbox.png"
            };
            if let Some(tex) = self.art.get(icon) {
                image(&tex, Rect::new(r.x, r.y + 4.0, 22.0, 22.0));
            }
            self.art
                .label(label, r.x + 30.0, r.y + 22.0, 17, LABEL, r.w - 34.0);
            if active && c.clicked(r) {
                self.focus = 3;
                return Some(Input::Choose(Field::Visibility, setup.visibility ^ bit));
            }
        }
        if active && self.focus == 3 && c.confirm {
            self.picker = Some(Picker {
                field: Field::Visibility,
                cursor: setup.visibility,
                first: 0,
            });
        }
        None
    }

    fn mode_buttons(&mut self, c: &Controls, x: f32, y: f32, w: f32, active: bool, focus: usize) {
        let switched = active
            && self.focus == focus
            && (is_key_pressed(KeyCode::Right) || is_key_pressed(KeyCode::Left) || c.confirm);
        if switched {
            self.players = if self.players == Players::HotSeat {
                Players::VersusComputer
            } else {
                Players::HotSeat
            };
        }
        for (idx, (players, label, id)) in [
            (Players::HotSeat, "Hot seat", "icons/icon-hotseat.png"),
            (Players::VersusComputer, "Computer", "icons/icon-ai.png"),
        ]
        .into_iter()
        .enumerate()
        {
            let rect = Rect::new(x + idx as f32 * (w * 0.5), y, w * 0.5 - 3.0, 38.0);
            self.art
                .row(rect, self.players == players, rect.contains(c.mouse));
            if let Some(tex) = self.art.get(id) {
                image(&tex, Rect::new(rect.x + 5.0, rect.y + 7.0, 24.0, 24.0));
            }
            self.art.label(
                label,
                rect.x + 34.0,
                rect.y + 25.0,
                16,
                LABEL,
                rect.w - 36.0,
            );
            if active && c.clicked(rect) {
                self.players = players;
                self.focus = focus;
            }
        }
    }

    fn lobby(&mut self, setup: &Setup, c: &Controls, error: Option<&str>) -> Input {
        let active = self.picker.is_none();
        if active {
            self.tab_focus(7);
        }
        let title = format!("Game Lobby  —  {}", self.map_name(setup, setup.map));
        self.art.label(&title, 20.0, 43.0, 32, GOLD, c.w - 40.0);
        self.mode_buttons(c, 19.0, 69.0, 370.0, active, 1);
        let list = Rect::new(17.0, 126.0, c.w - 34.0, c.h - 206.0);
        fill(list, Color::from_rgba(13, 19, 29, 204));
        edge(list);
        let xs = [0.015, 0.16, 0.36, 0.56, 0.80].map(|v| list.x + list.w * v);
        for (col, name) in ["Side", "Controller", "Faction", "Leader", "Team"]
            .iter()
            .enumerate()
        {
            self.art
                .label(name, xs[col] + 5.0, list.y + 29.0, 18, GOLD, list.w * 0.18);
        }
        let rows = ((list.h - 43.0) / 73.0).floor().max(1.0) as usize;
        self.lobby_first = self.lobby_first.min(setup.sides.len().saturating_sub(rows));
        let mut result = Input::Idle;
        if active && list.contains(c.mouse) && mouse_wheel().1 != 0.0 {
            self.lobby_first = self
                .lobby_first
                .saturating_add_signed(if mouse_wheel().1 > 0.0 { -2 } else { 2 })
                .min(setup.sides.len().saturating_sub(rows));
        }
        if active && self.focus == 0 && !setup.sides.is_empty() {
            let delta = if is_key_pressed(KeyCode::Up) {
                -1
            } else if is_key_pressed(KeyCode::Down) {
                1
            } else {
                0
            };
            if delta != 0 {
                let side = setup
                    .side
                    .saturating_add_signed(delta)
                    .min(setup.sides.len() - 1);
                self.lobby_first = scroll_to(side, self.lobby_first, rows, setup.sides.len());
                result = Input::Choose(Field::Side, side);
            }
        }
        for (row, (index, side)) in setup
            .sides
            .iter()
            .enumerate()
            .skip(self.lobby_first)
            .take(rows)
            .enumerate()
        {
            let r = Rect::new(
                list.x + 3.0,
                list.y + 40.0 + row as f32 * 73.0,
                list.w - 6.0,
                70.0,
            );
            self.art.row(r, index == setup.side, r.contains(c.mouse));
            let faction = &setup.factions[side.faction];
            let leader = &faction.leaders[side.leader];
            self.art.label(
                &format!("Side {}", index + 1),
                xs[0] + 4.0,
                r.center().y + 6.0,
                18,
                LABEL,
                xs[1] - xs[0] - 8.0,
            );
            let controller = if self.players == Players::VersusComputer && index != 0 {
                "Computer"
            } else {
                "Local Player"
            };
            self.art.label(
                controller,
                xs[1] + 4.0,
                r.center().y + 6.0,
                17,
                LABEL,
                xs[2] - xs[1] - 8.0,
            );
            for (col, (field, label, current)) in [
                (Field::Faction, faction.id.to_owned(), side.faction),
                (Field::Leader, leader.clone(), side.leader),
                (Field::Team, format!("Team {}", side.team + 1), side.team),
            ]
            .into_iter()
            .enumerate()
            {
                let a = xs[col + 2];
                let b = xs.get(col + 3).copied().unwrap_or(list.right() - 6.0);
                let control = Rect::new(a + 2.0, r.y + 12.0, b - a - 6.0, 46.0);
                if field == Field::Leader {
                    if let Some(tex) = self
                        .assets
                        .unit_image(leader)
                        .and_then(|p| self.art.file(p))
                    {
                        image(&tex, Rect::new(a + 1.0, r.center().y - 22.0, 44.0, 44.0));
                    }
                }
                if self.focus == 2 + col && index == setup.side {
                    edge(control);
                }
                let offset = if field == Field::Leader { 49.0 } else { 6.0 };
                self.art.label(
                    &label,
                    a + offset,
                    r.center().y + 6.0,
                    17,
                    LABEL,
                    b - a - offset - 14.0,
                );
                if let Some(arrow) = self.art.get("buttons/fold-arrow.png") {
                    image(&arrow, Rect::new(b - 17.0, r.center().y - 6.0, 12.0, 12.0));
                }
                if active
                    && (c.clicked(control)
                        || (c.confirm && self.focus == 2 + col && index == setup.side))
                {
                    self.focus = 2 + col;
                    self.picker = Some(Picker {
                        field,
                        cursor: current,
                        first: current.saturating_sub(3),
                    });
                    result = Input::Choose(Field::Side, index);
                }
            }
            if active && self.picker.is_none() && c.clicked(r) {
                self.focus = 0;
                result = Input::Choose(Field::Side, index);
            }
        }
        let cancel = Rect::new(c.w - 124.0, c.h - 54.0, 106.0, 36.0);
        let start = Rect::new(cancel.x - 150.0, cancel.y, 140.0, 36.0);
        self.art
            .button(start, "Start Game", c.mouse, self.focus == 5);
        self.art.button(cancel, "Cancel", c.mouse, self.focus == 6);
        if let Some(msg) = error {
            self.art
                .label(msg, list.x, c.h - 29.0, 16, ORANGE, start.x - list.x - 8.0);
        }
        if active
            && (is_key_pressed(KeyCode::Escape)
                || c.clicked(cancel)
                || (self.focus == 6 && c.confirm))
        {
            self.page = Page::Create;
            self.focus = 4;
        } else if active && (c.clicked(start) || (self.focus == 5 && c.confirm)) {
            return Input::Start(self.players);
        }
        result
    }

    fn pick(&mut self, setup: &Setup, c: &Controls) -> Input {
        let Some(mut picker) = self.picker.take() else {
            return Input::Idle;
        };
        let total = setup.count(picker.field);
        if total == 0 {
            return Input::Idle;
        }
        let visible = total.min(((c.h - 172.0) / 39.0).floor().max(1.0) as usize);
        let h = 104.0 + visible as f32 * 39.0;
        let r = Rect::new((c.w - 416.0) * 0.5, (c.h - h) * 0.5, 416.0, h);
        fill(
            Rect::new(0.0, 0.0, c.w, c.h),
            Color::from_rgba(0, 0, 0, 110),
        );
        fill(r, Color::from_rgba(19, 26, 36, 255));
        edge(r);
        self.art.label(
            picker.field.title(),
            r.x + 15.0,
            r.y + 32.0,
            24,
            GOLD,
            r.w - 30.0,
        );
        let delta = if is_key_pressed(KeyCode::Up) || mouse_wheel().1 > 0.0 {
            -1
        } else if is_key_pressed(KeyCode::Down) || mouse_wheel().1 < 0.0 {
            1
        } else {
            0
        };
        picker.cursor = picker.cursor.saturating_add_signed(delta).min(total - 1);
        picker.first = scroll_to(picker.cursor, picker.first, visible, total);
        for (row, i) in (picker.first..total).take(visible).enumerate() {
            let rect = Rect::new(r.x + 10.0, r.y + 44.0 + row as f32 * 39.0, r.w - 20.0, 38.0);
            self.art
                .row(rect, i == picker.cursor, rect.contains(c.mouse));
            self.art.label(
                &setup.label(picker.field, i),
                rect.x + 10.0,
                rect.y + 25.0,
                17,
                LABEL,
                rect.w - 20.0,
            );
            if c.clicked(rect) {
                return Input::Choose(picker.field, i);
            }
        }
        let cancel = Rect::new(r.right() - 116.0, r.bottom() - 44.0, 104.0, 32.0);
        self.art.button(cancel, "Cancel", c.mouse, false);
        if is_key_pressed(KeyCode::Escape) || c.clicked(cancel) {
            return Input::Idle;
        }
        if c.confirm {
            return Input::Choose(picker.field, picker.cursor);
        }
        self.picker = Some(picker);
        Input::Idle
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn map_list_follows_keyboard_selection_through_partial_last_pages() {
        assert_eq!(scroll_to(14, 0, 5, 18), 10);
        assert_eq!(scroll_to(17, 15, 5, 18), 13);
        assert_eq!(scroll_to(1, 13, 5, 18), 1);
        assert_eq!(scroll_to(1, 0, 9, 3), 0);
    }
}
