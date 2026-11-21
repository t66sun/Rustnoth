//! Standalone terrain editor. Only the map opened by the editor is visible here.
mod files;
mod fill;
mod palette;

use crate::{
    camera::BoardCamera,
    dialog,
    render::{self, EditorMinimap, EditorTerrain},
};
use engine_lite::{hex::Hex, map::editor::EditableMap, observation::Observation, wml};
use macroquad::prelude::*;
use palette::{Art, Palette};
use fill::{Region, Tool};
use rustnoth_gui::{
    assets::AssetCatalog,
    layout::{BoardLayout, Viewport},
};
use std::{collections::{BTreeMap, HashSet}, error::Error, fs, path::Path};

type Changes = BTreeMap<Hex, String>;

enum Edit {
    Terrain(Changes),
    Selection(HashSet<Hex>),
}

#[derive(Default)]
struct History {
    stroke: Changes,
    selected: HashSet<Hex>,
    selection_stroke: HashSet<Hex>,
    undo: Vec<Edit>,
    redo: Vec<Edit>,
}

impl History {
    fn paint(&mut self, doc: &mut EditableMap, at: Hex, code: &str) {
        if let Some(old) = doc.paint_stored(at, code) {
            self.stroke.entry(at).or_insert(old);
        }
    }

    fn finish(&mut self) -> bool {
        if self.stroke.is_empty() {
            return false;
        }
        self.undo.push(Edit::Terrain(std::mem::take(&mut self.stroke)));
        self.redo.clear();
        true
    }

    fn select(&mut self, at: Hex, add: bool) {
        let changed = if add { self.selected.insert(at) } else { self.selected.remove(&at) };
        if changed && !self.selection_stroke.insert(at) {
            self.selection_stroke.remove(&at);
        }
    }

    fn finish_selection(&mut self) {
        if !self.selection_stroke.is_empty() {
            self.undo.push(Edit::Selection(std::mem::take(&mut self.selection_stroke)));
            self.redo.clear();
        }
    }

    fn fill_selected(&mut self, doc: &mut EditableMap, brush: &palette::Brush) -> bool {
        for &at in &self.selected {
            if let Some(old) = doc.map().terrain_at(at) {
                let code = brush.paint(old, false);
                if let Some(old) = doc.paint_stored(at, &code) {
                    self.stroke.entry(at).or_insert(old);
                }
            }
        }
        self.finish()
    }

    // Return whether the terrain renderer needs rebuilding.
    fn apply(&mut self, doc: &mut EditableMap, undo: bool) -> bool {
        let (from, to) = if undo {
            (&mut self.undo, &mut self.redo)
        } else {
            (&mut self.redo, &mut self.undo)
        };
        let Some(edit) = from.pop() else {
            return false;
        };
        match edit {
            Edit::Selection(delta) => {
                for &hex in &delta {
                    if !self.selected.remove(&hex) { self.selected.insert(hex); }
                }
                to.push(Edit::Selection(delta));
                false
            }
            Edit::Terrain(changes) => {
                let mut inverse = Changes::new();
                for (hex, code) in changes {
                    if let Some(old) = doc.paint_stored(hex, &code) {
                        inverse.insert(hex, old);
                    }
                }
                to.push(Edit::Terrain(inverse));
                true
            }
        }
    }
}

#[derive(Copy, Clone, PartialEq)]
enum Command {
    New,
    Open,
    Save,
    Exit,
    Undo,
    Redo,
    FillSelection,
}

impl Command {
    fn label(self) -> &'static str {
        match self {
            Self::New => "New Map",
            Self::Open => "Open Map…",
            Self::Save => "Save Map As…",
            Self::Exit => "Quit to Title Screen",
            Self::Undo => "Undo",
            Self::Redo => "Redo",
            Self::FillSelection => "Fill Selection",
        }
    }
}

fn new_map() -> EditableMap {
    // Wesnoth's default context is 44 by 33 playable hexes with one stored border.
    let row = ["Gg"; 44 + 2].join(", ");
    EditableMap::parse(vec![row; 33 + 2].join("\n") + "\n").expect("valid blank grass map")
}

const SIDEBAR_WIDTH: f32 = 181.0;

fn map_viewport(width: f32, height: f32) -> Viewport {
    // The classic editor's main_map starts below the two toolbars. Tool hints overlay the map.
    Viewport { x: 0.0, y: 68.0, width: width - SIDEBAR_WIDTH, height: height - 68.0 }
}

fn update_editor_camera(camera: &mut BoardCamera, area: Viewport, view: &Observation, fit: bool) {
    let opening = camera.layout().is_none();
    camera.update(area, view, fit);
    if opening && !fit {
        // The editor opens at zero scroll, exposing its top and left half-hex borders.
        camera.center_world([0.0, 0.0]);
    }
}

fn draw_terrain(
    rules: &EditorTerrain,
    view: &Observation,
    layout: &BoardLayout,
    area: Viewport,
    history: &History,
    region: &Region,
    doc: &EditableMap,
    palette: &Palette,
    art: &mut Art,
    assets: &AssetCatalog,
) {
    let ms = (get_time() * 1000.0) as u64;
    let size = layout.tile_size();
    let [origin_x, origin_y] = layout.tile_origin(Hex { x: 0, y: 0 });
    let lo_x = (((area.x - origin_x - size) / (size * 0.75)).floor() as i32).max(-1);
    let hi_x = (((area.x + area.width - origin_x) / (size * 0.75)).ceil() as i32).min(view.width);
    let lo_y = (((area.y - origin_y) / size).floor() as i32 - 2).max(-1);
    let hi_y = (((area.y + area.height - origin_y) / size).ceil() as i32).min(view.height);
    for front in [false, true] {
        for y in lo_y..=hi_y {
            for odd in [0, 1] {
                for x in (lo_x..=hi_x).filter(|x| x.rem_euclid(2) == odd) {
                    let hex = Hex { x, y };
                    let [left, top] = layout.tile_origin(hex);
                    rules.draw(
                        hex,
                        vec2(left + size * 0.5, top + size * 0.5),
                        size,
                        front,
                        "",
                        ms,
                    );
                    if front && history.selected.contains(&hex) {
                        art.image(assets, "editor/selection-overlay.png", Rect::new(left, top, size, size));
                    }
                    if front && region.contains(hex) {
                        art.image(assets, "editor/brush.png", Rect::new(left, top, size, size));
                    }
                }
            }
        }
    }
    // Show brush feedback while the pointer is held. Rebuild connected terrain once per stroke.
    for &at in history.stroke.keys() {
        let [x, y] = layout.tile_origin(at);
        if let Some(code) = doc.map().terrain_at(at) {
            let (base, overlay) = code.split_once('^').unwrap_or((code, ""));
            let r = Rect::new(x, y, size, size);
            for brush in palette
                .brushes
                .iter()
                .filter(|b| b.code == base || b.code.strip_prefix('^') == Some(overlay))
            {
                art.image(assets, &brush.image, r);
            }
        }
    }
    for (id, at) in doc.map().numbered_starts() {
        let [x, y] = layout.tile_origin(at);
        let center = vec2(x + size * 0.5, y + size * 0.5);
        draw_circle(center.x, center.y, 11.0, Color::from_rgba(22, 18, 13, 202));
        let text = id.to_string();
        let width = measure_text(&text, None, 17, 1.0).width;
        draw_text(&text, center.x - width * 0.5, center.y + 5.0, 17.0, WHITE);
    }
}

fn text_button(
    area: Rect,
    label: &str,
    point: Vec2,
    active: bool,
    art: &mut Art,
    assets: &AssetCatalog,
    font: Option<&Font>,
) -> bool {
    let hot = area.contains(point);
    let suffix = if active || hot { "-active" } else { "" };
    art.image(
        assets,
        &format!("buttons/button_menu/menu_button_copper_H20{suffix}.png"),
        area,
    );
    let w = measure_text(label, font, 15, 1.0).width;
    dialog::text(
        font,
        label,
        area.center().x - w / 2.0,
        area.center().y + 5.0,
        15,
        WHITE,
    );
    hot && is_mouse_button_pressed(MouseButton::Left)
}

struct Chrome {
    menu: Option<bool>,
    tip: Option<&'static str>,
}

impl Chrome {
    fn draw(
        &mut self,
        w: f32,
        h: f32,
        side: f32,
        point: Vec2,
        assets: &AssetCatalog,
        art: &mut Art,
        font: Option<&Font>,
        name: &str,
    ) -> Option<Command> {
        self.tip = None;
        let click = is_mouse_button_pressed(MouseButton::Left);
        let sidebar = Rect::new(w - side, 0.0, side, h);
        art.image(assets, "themes/classic/sidebar.png", sidebar);
        art.image(
            assets,
            "themes/classic/menubar.png",
            Rect::new(0.0, 0.0, w - side, 27.0),
        );
        art.image(
            assets,
            "themes/editor/classic/toolbar.png",
            Rect::new(0.0, 27.0, w - side, 40.0),
        );
        art.image(
            assets,
            "themes/editor/classic/menubar_rightside.png",
            Rect::new(w - side, 0.0, side, 26.0),
        );
        let mut triggered = false;
        for (index, label) in ["File", "Edit"].iter().enumerate() {
            let file = index == 0;
            if text_button(
                Rect::new(index as f32 * 76.0, 1.0, 75.0, 23.0),
                label,
                point,
                self.menu == Some(file),
                art,
                assets,
                font,
            ) {
                self.menu = if self.menu == Some(file) {
                    None
                } else {
                    Some(file)
                };
                triggered = true;
            }
        }
        let mut result = None;
        for (i, (key, cmd)) in [
            ("editor-map-load_30", Command::Open),
            ("editor-map-save_30", Command::Save),
            ("undo_30", Command::Undo),
            ("redo_30", Command::Redo),
        ]
        .into_iter()
        .enumerate()
        {
            let r = Rect::new(7.0 + i as f32 * 34.0, 31.0, 30.0, 30.0);
            art.image(assets, "buttons/button_square/button_square_30.png", r);
            art.image(assets, &format!("icons/action/{key}.png"), r);
            if r.contains(point) {
                self.tip = Some(cmd.label());
                if click && self.menu.is_none() {
                    result = Some(cmd);
                }
            }
        }
        let title = dialog::fit(font, name.into(), (w - side - 173.0).max(12.0), 16);
        dialog::text(font, &title, 165.0, 19.0, 16, dialog::PALE);
        if let Some(file) = self.menu {
            let commands: &[Command] = if file {
                &[Command::New, Command::Open, Command::Save, Command::Exit]
            } else {
                &[Command::Undo, Command::Redo, Command::FillSelection]
            };
            for (i, &cmd) in commands.iter().enumerate() {
                let r = Rect::new(
                    if file { 0.0 } else { 76.0 },
                    26.0 + i as f32 * 28.0,
                    175.0,
                    28.0,
                );
                draw_rectangle(
                    r.x,
                    r.y,
                    r.w,
                    r.h,
                    if r.contains(point) {
                        Color::from_rgba(44, 65, 71, 255)
                    } else {
                        Color::from_rgba(31, 32, 35, 255)
                    },
                );
                dialog::text(font, cmd.label(), r.x + 9.0, r.y + 19.0, 16, WHITE);
                if !triggered && click && r.contains(point) {
                    result = Some(cmd);
                }
            }
            if click && !triggered {
                self.menu = None;
            }
        }
        result
    }
}

pub(super) async fn run(root: &Path, core: &[u8]) -> Result<(), Box<dyn Error>> {
    let form = dialog::Dialog::new(root);
    let assets = AssetCatalog::parse(root, core, b"")?;
    let mut palette = Palette::new(&wml::parse(core)?)?;
    let mut art = Art::new();
    let mut doc = new_map();
    let mut view = render::map_observation(doc.map());
    let mut rules = EditorTerrain::default();
    let mut mini = EditorMinimap::new(root);
    let mut rebuild = true;
    let mut camera = BoardCamera::default();
    let mut history = History::default();
    let mut chrome = Chrome {
        menu: None,
        tip: None,
    };
    let mut name = "Untitled".to_string();
    let folder = crate::display_preferences::config_directory()?.join("editor/maps");
    let mut status = String::new();
    let mut drawing = None;
    let mut selecting = false;
    let mut last_selection = None;
    let mut tool = Tool::Paint;
    let mut region = Region::default();
    next_frame().await;
    loop {
        if rebuild {
            region.clear();
            view = render::map_observation(doc.map());
            rules.load_editor(&assets, &view, doc.map());
            mini.load(&view, &assets);
            rebuild = false;
        }
        let scale = (screen_width() / 660.0)
            .min(screen_height() / 440.0)
            .clamp(0.01, 1.0);
        let (w, h) = (screen_width() / scale, screen_height() / scale);
        let sidebar = SIDEBAR_WIDTH;
        let area = map_viewport(w, h);
        let (mx, my) = mouse_position();
        let point = vec2(mx / scale, my / scale);
        let p = [point.x, point.y];
        let ctrl = is_key_down(KeyCode::LeftControl)
            || is_key_down(KeyCode::RightControl)
            || is_key_down(KeyCode::LeftSuper)
            || is_key_down(KeyCode::RightSuper);
        let shift = is_key_down(KeyCode::LeftShift) || is_key_down(KeyCode::RightShift);
        update_editor_camera(&mut camera, area, &view, is_key_pressed(KeyCode::Home));
        camera.scroll(p, mouse_wheel().1);
        camera.drag(
            p,
            is_mouse_button_pressed(MouseButton::Middle),
            is_mouse_button_down(MouseButton::Middle),
        );
        let modal = chrome.menu.is_some();
        if !modal && !ctrl {
            if is_key_pressed(KeyCode::P) {
                tool = Tool::Paint;
            } else if is_key_pressed(KeyCode::F) && !shift {
                tool = Tool::Fill;
            } else if is_key_pressed(KeyCode::S) {
                tool = Tool::Select;
            }
        }
        if tool != Tool::Paint && drawing.take().is_some() {
            rebuild |= history.finish();
        }
        if selecting && (tool != Tool::Select || !is_mouse_button_down(MouseButton::Left)) {
            history.finish_selection();
            selecting = false;
            last_selection = None;
        }
        let at = if tool != Tool::Paint && area.contains(p) {
            camera.layout().and_then(|layout| layout.editor_hex_at(p))
        } else {
            camera.hex_at(p)
        };
        region.follow(doc.map(), at.filter(|_| !modal && (tool == Tool::Fill || tool == Tool::Select && shift)));
        if tool == Tool::Select && !modal {
            if let Some(hex) = at {
                selecting |= is_mouse_button_pressed(MouseButton::Left);
                let same_brush = last_selection.is_some_and(|(prior, remove, wand)| {
                    remove == ctrl && wand == shift && (prior == hex || shift && region.contains(prior))
                });
                if selecting && !same_brush {
                    if shift {
                        for at in region.hexes() { history.select(at, !ctrl); }
                    } else {
                        history.select(hex, !ctrl);
                    }
                    last_selection = Some((hex, ctrl, shift));
                }
            }
        } else if !modal && let Some(hex) = at {
            for (btn, foreground) in [(MouseButton::Left, true), (MouseButton::Right, false)] {
                if is_mouse_button_pressed(btn) {
                    if ctrl {
                        if let Some(code) = doc.map().terrain_at(hex) {
                            palette.sample(code, foreground);
                        }
                    } else if tool == Tool::Fill {
                        let i = if foreground { palette.fg } else { palette.bg };
                        rebuild |= region.paint(&mut doc, &mut history, hex, &palette.brushes[i], shift);
                        break;
                    } else {
                        drawing = Some(btn);
                    }
                }
            }
            if let Some(btn) = drawing
                && is_mouse_button_down(btn)
                && let Some(old) = doc.map().terrain_at(hex)
            {
                let i = if btn == MouseButton::Left {
                    palette.fg
                } else {
                    palette.bg
                };
                let code = palette.brushes[i].paint(old, shift);
                history.paint(&mut doc, hex, &code);
            }
        }
        if let Some(btn) = drawing
            && !is_mouse_button_down(btn)
        {
            drawing = None;
            rebuild |= history.finish();
        }

        set_camera(&dialog::setup_camera(w, h));
        clear_background(BLACK);
        if let Some(layout) = camera.layout() {
            draw_terrain(
                &rules,
                &view,
                layout,
                area,
                &history,
                &region,
                &doc,
                &palette,
                &mut art,
                &assets,
            );
            if let Some(hex) = at {
                let [x, y] = layout.tile_origin(hex);
                let rect = Rect::new(x, y, layout.tile_size(), layout.tile_size());
                art.image(&assets, "misc/hover-hex.png", rect);
                if tool == Tool::Select && !modal {
                    if !shift { art.image(&assets, "editor/brush.png", rect); }
                    let icon = if shift { "editor/tool-overlay-select-wand.png" } else { "editor/tool-overlay-select-brush.png" };
                    art.tinted(&assets, icon, rect, Color::from_rgba(255, 255, 255, 196));
                }
            }
        }
        let title = format!(
            "{}{} — Map Editor",
            name,
            if doc.dirty() { " *" } else { "" }
        );
        let mut command = chrome.draw(
            w,
            h,
            sidebar,
            point,
            &assets,
            &mut art,
            form.font.as_ref(),
            &title,
        );
        let mini_area = Viewport {
            x: w - sidebar + 8.0,
            y: 31.0,
            width: sidebar - 16.0,
            height: (h * 0.20).min(148.0),
        };
        mini.draw_map(&view, mini_area, area, camera.layout(), None, None, &[]);
        let mini_rect = Rect::new(mini_area.x, mini_area.y, mini_area.width, mini_area.height);
        if mini_rect.contains(point) && is_mouse_button_pressed(MouseButton::Left) {
            let map_w = view.width as f32 * 0.75;
            let map_h = view.height as f32;
            let zoom = (mini_area.width / map_w).min(mini_area.height / map_h);
            let origin = mini_rect.center() - vec2(map_w, map_h) * zoom / 2.0;
            let world = (point - origin) / zoom;
            camera.center_world([world.x + 0.25, world.y + 0.5]);
        }
        let tools = Rect::new(w - sidebar, mini_rect.bottom() + 6.0, sidebar, 70.0);
        tool = tool.panel(tools, point, &mut art, &assets, &mut chrome.tip);
        let panel = Rect::new(w - sidebar, tools.bottom(), sidebar, h - tools.bottom());
        art.image(&assets, "themes/editor/classic/palette.png", panel);
        art.image(
            &assets,
            "themes/editor/classic/palette_top.png",
            Rect::new(panel.x, panel.y, panel.w, 90.0),
        );
        palette.panel(panel, point, &assets, &mut art, form.font.as_ref());
        draw_rectangle(
            0.0,
            h - 26.0,
            w - sidebar,
            26.0,
            Color::from_rgba(30, 31, 34, 255),
        );
        let caption = if let Some(tip) = chrome.tip {
            tip.to_owned()
        } else if !status.is_empty() {
            status.clone()
        } else if tool == Tool::Select {
            "Drag to select · Ctrl to deselect · Shift for matching terrain · Shift+F to fill".into()
        } else if let Some(hex) = at {
            format!(
                "{},{}     {}     Ctrl-click to sample · Shift to paint one layer",
                hex.x + 1,
                hex.y + 1,
                doc.map().terrain_at(hex).unwrap_or("")
            )
        } else {
            "Left and right click paint the two selected terrains".into()
        };
        dialog::text(
            form.font.as_ref(),
            &dialog::fit(form.font.as_ref(), caption, w - sidebar - 18.0, 15),
            9.0,
            h - 8.0,
            15,
            WHITE,
        );
        set_default_camera();
        if ctrl && is_key_pressed(KeyCode::N) {
            command = Some(Command::New);
        }
        if ctrl && is_key_pressed(KeyCode::O) {
            command = Some(Command::Open);
        }
        if ctrl && is_key_pressed(KeyCode::S) {
            command = Some(Command::Save);
        }
        if ctrl && is_key_pressed(KeyCode::Z) {
            command = Some(if shift { Command::Redo } else { Command::Undo });
        }
        if ctrl && is_key_pressed(KeyCode::Y) {
            command = Some(Command::Redo);
        }
        if !modal && shift && !ctrl && is_key_pressed(KeyCode::F) {
            command = Some(Command::FillSelection);
        }
        if is_key_pressed(KeyCode::Escape) {
            if chrome.menu.take().is_none() {
                command = Some(Command::Exit);
            }
        }
        if let Some(cmd) = command {
            drawing = None;
            selecting = false;
            last_selection = None;
            history.finish_selection();
            rebuild |= history.finish();
            match cmd {
                Command::Exit => {
                    if !doc.dirty() || files::confirm(&form).await {
                        return Ok(());
                    }
                }
                Command::Undo | Command::Redo => {
                    rebuild |= history.apply(&mut doc, cmd == Command::Undo);
                    status.clear();
                }
                Command::Save => {
                    let bytes = doc.text();
                    match files::save(&folder, &form, &name, &bytes).await {
                        Ok(Some(path)) => {
                            name = path
                                .file_stem()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .into_owned();
                            doc = EditableMap::parse(bytes)?;
                            status = format!("Saved {}", path.display());
                        }
                        Err(e) => status = e.to_string(),
                        _ => {}
                    }
                }
                Command::FillSelection => {
                    rebuild |= history.fill_selected(&mut doc, &palette.brushes[palette.bg]);
                }
                Command::New | Command::Open => {
                    let incoming: Result<Option<(EditableMap, String)>, Box<dyn Error>> =
                        if cmd == Command::New {
                            Ok(Some((new_map(), "Untitled".into())))
                        } else {
                            async {
                                let Some(file) = files::open(root, &folder, &form).await? else {
                                    return Ok(None);
                                };
                                let next = EditableMap::parse(fs::read_to_string(&file)?)?;
                                if next.map().width() < 1 || next.map().height() < 1 {
                                    return Err("Map contains no playable hexes".into());
                                }
                                Ok(Some((
                                    next,
                                    file.file_stem()
                                        .unwrap_or_default()
                                        .to_string_lossy()
                                        .into_owned(),
                                )))
                            }
                            .await
                        };
                    match incoming {
                        Ok(Some((next, label))) => {
                            if !doc.dirty() || files::confirm(&form).await {
                                doc = next;
                                name = label;
                                history = History::default();
                                camera = BoardCamera::default();
                                rebuild = true;
                                status.clear();
                            }
                        }
                        Err(error) => status = error.to_string(),
                        _ => {}
                    }
                }
            }
        }
        next_frame().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_edits_and_background_fill_share_undo_order() {
        let mut doc = new_map();
        let mut history = History::default();
        let a = Hex { x: 0, y: 0 };
        let b = Hex { x: -1, y: 0 };
        history.select(a, true);
        history.select(a, true);
        history.select(b, true);
        history.finish_selection();
        assert!(!doc.dirty());
        history.select(a, false);
        history.finish_selection();
        let cfg = wml::parse("[terrain_type]\nstring=Ww\neditor_group=water\nsymbol_image=water\n[/terrain_type]").unwrap();
        let palette = Palette::new(&cfg).unwrap();
        assert!(history.fill_selected(&mut doc, &palette.brushes[palette.bg]));
        assert_eq!(doc.map().terrain_at(b), Some("Ww"));
        assert_eq!(doc.map().terrain_at(a), Some("Gg"));
        assert_eq!(history.selected, HashSet::from([b]));
        assert!(history.apply(&mut doc, true));
        assert!(!doc.dirty());
        history.apply(&mut doc, true);
        assert_eq!(history.selected, HashSet::from([a, b]));
        history.apply(&mut doc, true);
        assert!(history.selected.is_empty());
        history.apply(&mut doc, false);
        history.apply(&mut doc, false);
        assert!(history.apply(&mut doc, false));
        assert_eq!(history.selected, HashSet::from([b]));
        assert_eq!(doc.map().terrain_at(b), Some("Ww"));
    }

    #[test]
    fn default_editor_opens_at_the_northwest_border_of_its_native_grass_map() {
        let doc = new_map();
        let map = doc.map();
        assert_eq!((map.width(), map.height()), (44, 33));
        assert_eq!(map.terrain_at(Hex { x: -1, y: -1 }), Some("Gg"));
        assert_eq!(map.terrain_at(Hex { x: 44, y: 33 }), Some("Gg"));
        let view = render::map_observation(map);
        let area = map_viewport(1200.0, 820.0);
        assert_eq!((area.x, area.y, area.width, area.height), (0.0, 68.0, 1019.0, 752.0));
        let mut camera = BoardCamera::default();
        update_editor_camera(&mut camera, area, &view, false);
        let origin = Hex { x: 0, y: 0 };
        let layout = camera.layout().unwrap();
        assert_eq!(layout.tile_size(), 72.0);
        assert_eq!(layout.tile_origin(origin), [27.0, 104.0]);
        camera.pan([-54.0, -72.0]);
        update_editor_camera(&mut camera, area, &view, false);
        assert_eq!(camera.layout().unwrap().tile_origin(origin), [-27.0, 32.0]);
        update_editor_camera(&mut camera, area, &view, true);
        assert!(camera.layout().unwrap().tile_size() < 72.0);
    }
    #[test]
    fn a_brush_stroke_undoes_and_redoes_as_one_edit() {
        let mut doc = new_map();
        let mut history = History::default();
        let a = Hex { x: 0, y: 0 };
        let b = Hex { x: 1, y: 1 };
        history.paint(&mut doc, a, "Wo");
        history.paint(&mut doc, b, "Ke");
        history.paint(&mut doc, a, "Ww");
        assert!(history.finish());
        assert!(history.apply(&mut doc, true));
        assert!(!doc.dirty());
        assert!(history.apply(&mut doc, false));
        assert_eq!(doc.map().terrain_at(a), Some("Ww"));
        assert_eq!(doc.map().terrain_at(b), Some("Ke"));
    }
}
