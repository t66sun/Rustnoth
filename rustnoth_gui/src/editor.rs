//! Standalone terrain editor. Only the map opened by the editor is visible here.
mod files;
mod documents;
mod menubar;
mod fill;
mod minimap_buttons;
mod palette;
mod toolbar;
mod shapes;
mod terrain_report;

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

#[derive(Copy, Clone, Debug, PartialEq)]
enum Command {
    New,
    Open,
    Save,
    Close,
    Exit,
    Undo,
    Redo,
    FillSelection,
    ToggleGrid,
    Refresh,
    Switch(usize),
}

impl Command {
    fn label(self) -> &'static str {
        match self {
            Self::New => "New Map",
            Self::Open => "Open Map…",
            Self::Save => "Save Map As…",
            Self::Close => "Close Map",
            Self::Exit => "Quit to Title Screen",
            Self::Undo => "Undo",
            Self::Redo => "Redo",
            Self::FillSelection => "Fill Selection",
            Self::ToggleGrid => "Toggle Grid",
            Self::Switch(_) => "Switch Map",
            Self::Refresh => "Refresh Display",
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
    grid: Option<&render::GridArt>,
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
                    let center = vec2(left + size * 0.5, top + size * 0.5);
                    rules.draw(
                        hex,
                        center,
                        size,
                        front,
                        "",
                        ms,
                    );
                    if !front && let Some(grid) = grid {
                        grid.draw(center, size);
                    }
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


struct Chrome {
    show_grid: bool,
    menu: Option<menubar::Menu>,
    menubar: menubar::Menubar,
    tip: Option<&'static str>,
    toolbar: toolbar::Toolbar,
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
        names: &[String],
        doc: &EditableMap,
        history: &History,
    ) -> Option<Command> {
        self.tip = None;
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
            Rect::new(0.0, 27.0, w - side - 10.0, 41.0),
        );
        art.image(assets, "themes/editor/classic/toolbar_rightside.png",
            Rect::new(w - side - 10.0, 27.0, 10.0, 41.0));
        art.image(
            assets,
            "themes/editor/classic/menubar_rightside.png",
            Rect::new(w - side, 0.0, side, 26.0),
        );
        let tool_action = self.toolbar.draw(w, point, self.menu.is_some(), self.show_grid, doc, history,
            assets, art, &mut self.tip);
        let result = self.menubar.draw(&mut self.menu, point, assets, art, font, names).or(tool_action);
        result
    }
}

// MINIMAP_THEME and its compact 1024x600 / 800x600 variant.
fn minimap_panel(w: f32, h: f32) -> (Rect, Viewport, &'static str) {
    let wide = w >= 1024.0 && h >= 768.0;
    let frame = Rect::new(w - 181.0, 6.0, 181.0, if wide { 255.0 } else { 170.0 });
    let (left, top, height, image) = if wide {
        (13.0, 5.0, 212.0, "themes/classic/minimap.png")
    } else {
        (43.0, 4.0, 158.0, "themes/classic/minimap-800.png")
    };
    (frame, Viewport { x: frame.x + left, y: frame.y + top, width: frame.w - left - 8.0, height }, image)
}

fn add_minimap_border(view: &mut Observation, doc: &EditableMap) {
    // getMinimap starts at (0, 0), including the last column and row of
    // border terrain whose symbol images overlap the edge of the canvas.
    let (w, h) = (view.width, view.height);
    let edge = (0..h).map(|y| Hex { x: w, y })
        .chain((0..=w).map(|x| Hex { x, y: h }));
    for position in edge {
        if let Some(terrain) = doc.map().terrain_at(position) {
            view.tiles.push(engine_lite::observation::ObservedTile {
                position, terrain: terrain.into(), visible: true,
                village_owner: None, time_color: [0; 3],
            });
        }
    }
    view.tiles.sort_unstable_by_key(|tile| (tile.position.y, tile.position.x));
}

pub(super) async fn run(root: &Path, core: &[u8]) -> Result<(), Box<dyn Error>> {
    let form = dialog::Dialog::new(root);
    let assets = AssetCatalog::parse(root, core, b"")?;
    let cfg = wml::parse(core)?;
    let mut palette = Palette::load(&cfg, root)?;
    let mut shapes = shapes::Shapes::load(root, &cfg)?;
    let mut art = Art::new();
    let mut documents = documents::Documents::default();
    let mut view = render::map_observation(documents.list[0].map.map());
    let mut rules = EditorTerrain::default();
    let mut mini = EditorMinimap::new(root);
    let mut reports = terrain_report::Reports::new(root);
    let mut mini_buttons = minimap_buttons::Buttons::default();
    let mut rebuild = true;
    let mut chrome = Chrome {
        show_grid: crate::display_preferences::grid()?,
        menu: None,
        menubar: menubar::Menubar::new(root),
        tip: None,
        toolbar: Default::default(),
    };
    let grid = render::GridArt::load(&root.join("data/core/images/terrain"));
    let folder = crate::display_preferences::config_directory()?.join("editor/maps");
    let mut status = String::new();
    let mut drawing = None;
    let mut selecting = false;
    let mut last_selection = None;
    let mut tool = Tool::Paint;
    let mut region = Region::default();
    next_frame().await;
    loop {
        let names = if chrome.menu == Some(menubar::Menu::Window) { documents.titles() } else { Vec::new() };
        let current = &mut documents.list[documents.current];
        let documents::Document { map: doc, history, camera, path } = current;
        if rebuild {
            region.clear();
            view = render::map_observation(doc.map());
            rules.load_editor(&assets, &view, doc.map());
            add_minimap_border(&mut view, doc);
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
        update_editor_camera(camera, area, &view, is_key_pressed(KeyCode::Home));
        camera.scroll(p, mouse_wheel().1);
        camera.drag(
            p,
            is_mouse_button_pressed(MouseButton::Middle),
            is_mouse_button_down(MouseButton::Middle),
        );
        let palette_menu = palette.group_menu_open();
        let modal = chrome.menu.is_some() || palette_menu;
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
        if !modal && !ctrl && !shift && !is_key_down(KeyCode::LeftAlt)
            && !is_key_down(KeyCode::RightAlt) && tool != Tool::Fill {
            shapes.keys();
        }
        let at = if area.contains(p) {
            camera.layout().and_then(|layout| layout.editor_hex_at(p))
        } else {
            None
        };
        let footprint = at.filter(|_| !modal && (tool == Tool::Paint || tool == Tool::Select && !shift))
            .map(|hex| shapes.footprint(doc.map(), hex)).unwrap_or_default();
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
                        for &at in &footprint { history.select(at, !ctrl); }
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
                        rebuild |= region.paint(doc, history, hex, &palette.brushes[i], shift);
                        break;
                    } else {
                        drawing = Some(btn);
                    }
                }
            }
            if let Some(btn) = drawing
                && is_mouse_button_down(btn)
            {
                let i = if btn == MouseButton::Left {
                    palette.fg
                } else {
                    palette.bg
                };
                shapes::paint(doc, history, &footprint, &palette.brushes[i], shift);
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
                chrome.show_grid.then_some(&grid),
                &view,
                layout,
                area,
                history,
                &region,
                doc,
                &palette,
                &mut art,
                &assets,
            );
            for &hex in &footprint {
                let [x, y] = layout.tile_origin(hex);
                art.image(&assets, "editor/brush.png", Rect::new(x, y, layout.tile_size(), layout.tile_size()));
            }
            if let Some(hex) = at {
                let [x, y] = layout.tile_origin(hex);
                let rect = Rect::new(x, y, layout.tile_size(), layout.tile_size());
                art.image(&assets, "misc/hover-hex.png", rect);
                if tool == Tool::Select && !modal {
                    let icon = if shift { "editor/tool-overlay-select-wand.png" } else { "editor/tool-overlay-select-brush.png" };
                    art.tinted(&assets, icon, rect, Color::from_rgba(255, 255, 255, 196));
                }
            }
        }
        let mut command = chrome.draw(
            w,
            h,
            sidebar,
            point,
            &assets,
            &mut art,
            form.font.as_ref(),
            &names,
            doc,
            history,
        );
        let (mini_frame, mini_area, mini_background) = minimap_panel(w, h);
        art.image(&assets, mini_background, mini_frame);
        mini.draw_map(&view, mini_area, area, camera.layout(), None, None, &[]);
        reports.draw(w, h, at, doc, &palette, &assets, form.font.as_ref());
        if let Err(error) = mini_buttons.draw(mini_frame, point, !modal, camera,
            &mut mini, &mut art, &assets, &mut chrome.tip) {
            status = error.to_string();
        }
        let mini_rect = Rect::new(mini_area.x, mini_area.y, mini_area.width, mini_area.height);
        if mini_rect.contains(point) && is_mouse_button_pressed(MouseButton::Left) {
            let map_w = view.width as f32 * 0.75;
            let map_h = view.height as f32;
            let zoom = (mini_area.width / map_w).min(mini_area.height / map_h);
            let origin = mini_rect.center() - vec2(map_w, map_h) * zoom / 2.0;
            let world = (point - origin) / zoom;
            camera.center_world([world.x + 0.25, world.y + 0.5]);
        }
        let compact = w < 1024.0 || h < 768.0;
        let tools = if compact { Rect::new(w - sidebar, 219., sidebar, 92.) }
                    else { Rect::new(w - sidebar, 304., sidebar, 155.) };
        tool = tool.panel(tools, compact, point, &mut art, &assets, &mut chrome.tip);
        shapes.panel(tools, compact, tool != Tool::Fill && !modal, point, &mut art, &assets, &mut chrome.tip);
        let palette_y = if compact { 312.0 } else { 467.0 };
        let panel = Rect::new(w - sidebar, palette_y, sidebar, h - palette_y);
        palette.panel(panel, compact, point, &assets, &mut art, form.font.as_ref());
        let font = form.font.as_ref();
        let lines = if let Some(tip) = chrome.tip {
            vec![tip.to_owned()]
        } else if !status.is_empty() {
            vec![status.clone()]
        } else if tool == Tool::Select {
            vec!["Drag to select · Ctrl to deselect · Shift for matching terrain · Shift+F to fill".into()]
        } else {
            palette.help().into()
        };
        let mut font_size = 18;
        let width = loop {
            let max_width = lines.iter().map(|line| measure_text(line, font, font_size, 1.0).width)
                .fold(0.0, f32::max);
            if max_width <= w * 0.5 || font_size <= 8 {
                break max_width;
            }
            font_size -= 1;
        };
        let line_height = font_size as f32 + 6.0;
        let x = (w - width) * 0.5;
        let y = h - line_height * lines.len() as f32 - 10.0;
        draw_rectangle(x - 5.0, y, width + 10.0, h - y, Color::from_rgba(0, 0, 0, 0xbb));
        for (row, line) in lines.iter().enumerate() {
            dialog::text(font, line, x, y + 5.0 + font_size as f32 + row as f32 * line_height, font_size, WHITE);
        }
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
        let grid_modifier = if cfg!(target_os = "macos") {
            is_key_down(KeyCode::LeftSuper) || is_key_down(KeyCode::RightSuper)
        } else {
            is_key_down(KeyCode::LeftControl) || is_key_down(KeyCode::RightControl)
        };
        if !modal && grid_modifier && !shift && !is_key_down(KeyCode::LeftAlt)
            && !is_key_down(KeyCode::RightAlt) && is_key_pressed(KeyCode::G) {
            command = Some(Command::ToggleGrid);
        }
        if is_key_pressed(KeyCode::Escape) && !palette_menu {
            if chrome.menu.take().is_none() {
                command = Some(Command::Exit);
            }
        }
        if let Some(cmd) = command {
            if cmd != Command::ToggleGrid {
                drawing = None;
                selecting = false;
                last_selection = None;
                history.finish_selection();
                rebuild |= history.finish();
            }
            match cmd {
                Command::Refresh => { rebuild = true; }
                Command::ToggleGrid => {
                    match crate::display_preferences::save_grid(!chrome.show_grid) {
                        Ok(()) => chrome.show_grid = !chrome.show_grid,
                        Err(error) => status = error.to_string(),
                    }
                }
                Command::Exit => {
                    if !documents.list.iter().any(|d| d.map.dirty()) || files::confirm(&form).await {
                        return Ok(());
                    }
                }
                Command::Undo | Command::Redo => {
                    rebuild |= history.apply(doc, cmd == Command::Undo);
                    status.clear();
                }
                Command::Save => {
                    let bytes = doc.text();
                    let name = path.as_deref().and_then(Path::file_stem).and_then(|s| s.to_str()).unwrap_or("Untitled");
                    match files::save(&folder, &form, name, &bytes).await {
                        Ok(Some(saved)) => {
                            *path = Some(fs::canonicalize(&saved)?);
                            *doc = EditableMap::parse(bytes)?;
                            status = format!("Saved {}", saved.display());
                        }
                        Err(e) => status = e.to_string(),
                        _ => {}
                    }
                }
                Command::FillSelection => {
                    rebuild |= history.fill_selected(doc, &palette.brushes[palette.bg]);
                }
                Command::Switch(index) => {
                    if index < documents.list.len() && index != documents.current {
                        documents.current = index;
                        rebuild = true;
                        status.clear();
                    }
                }
                Command::New => {
                    documents.new_map();
                    rebuild = true;
                    status.clear();
                }
                Command::Close => {
                    if !doc.dirty() || files::confirm(&form).await {
                        documents.close();
                        tool = Tool::Paint;
                        rebuild = true;
                        status.clear();
                    }
                }
                Command::Open => {
                    match files::open(root, &folder, &form).await {
                        Ok(Some(file)) => match documents.open(&file) {
                            Ok(()) => { rebuild = true; status.clear(); }
                            Err(error) => status = error.to_string(),
                        },
                        Ok(None) => {}
                        Err(error) => status = error.to_string(),
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
        let palette = Palette::new(&cfg, &Default::default()).unwrap();
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
    fn editor_minimap_uses_the_installed_theme_at_both_resolutions() {
        let (panel, canvas, _) = minimap_panel(1200.0, 820.0);
        assert_eq!(panel, Rect::new(1019.0, 6.0, 181.0, 255.0));
        assert_eq!(canvas, Viewport { x: 1032.0, y: 11.0, width: 160.0, height: 212.0 });
        let (_, compact, _) = minimap_panel(800.0, 600.0);
        assert_eq!(compact, Viewport { x: 662.0, y: 10.0, width: 130.0, height: 158.0 });
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
