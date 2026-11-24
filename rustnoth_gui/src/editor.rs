//! Standalone terrain editor. Only the map opened by the editor is visible here.
mod files;
mod new_map_dialog;
mod documents;
mod menubar;
mod locations;
mod hex_labels;
mod clipboard;
mod fill;
mod help_label;
use crate::minimap_buttons;
mod palette;
mod toolbar;
mod shapes;
mod terrain_report;
mod village_report;

use crate::{
    camera::BoardCamera,
    dialog,
    render::{self, EditorMinimap, EditorTerrain},
};
use render::minimap::{Drag as MinimapDrag, Geometry as MinimapGeometry};
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
    Location(engine_lite::map::editor::LocationEdit),
    Resize(Box<EditableMap>),
    Terrain(Changes),
    Selection(HashSet<Hex>),
    Stroke(Vec<Edit>),
}

#[derive(Default)]
struct History {
    stroke: Changes,
    terrain_steps: Vec<Changes>,
    selected: HashSet<Hex>,
    selection_stroke: HashSet<Hex>,
    selection_steps: Vec<HashSet<Hex>>,
    undo: Vec<Edit>,
    redo: Vec<Edit>,
}

impl History {
    fn place(&mut self, doc: &mut EditableMap, id: &str, at: Option<Hex>) {
        if let Some(undo) = doc.set_location(id, at) {
            self.undo.push(Edit::Location(undo));
            self.redo.clear();
        }
    }
    fn paint(&mut self, doc: &mut EditableMap, at: Hex, code: &str) {
        if let Some(old) = doc.paint_stored(at, code) {
            self.stroke.entry(at).or_insert(old);
        }
    }

    fn finish_brush_step(&mut self) {
        if !self.stroke.is_empty() {
            self.terrain_steps.push(std::mem::take(&mut self.stroke));
        }
    }

    fn finish(&mut self) -> bool {
        self.finish_brush_step();
        if self.terrain_steps.is_empty() {
            return false;
        }
        self.undo.push(Edit::Stroke(self.terrain_steps.drain(..).map(Edit::Terrain).collect()));
        self.redo.clear();
        true
    }

    fn select(&mut self, at: Hex, add: bool) {
        let changed = if add { self.selected.insert(at) } else { self.selected.remove(&at) };
        if changed && !self.selection_stroke.insert(at) {
            self.selection_stroke.remove(&at);
        }
    }

    fn finish_selection_step(&mut self) {
        if !self.selection_stroke.is_empty() {
            self.selection_steps.push(std::mem::take(&mut self.selection_stroke));
        }
    }

    fn finish_selection(&mut self) {
        self.finish_selection_step();
        if !self.selection_steps.is_empty() {
            self.undo.push(Edit::Stroke(self.selection_steps.drain(..).map(Edit::Selection).collect()));
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
        let (inverse, dirty) = edit.perform(doc, &mut self.selected);
        to.push(inverse);
        dirty
    }

    fn partial_undo(&mut self, doc: &mut EditableMap) -> bool {
        if let Some(Edit::Stroke(steps)) = self.undo.last_mut()
            && steps.len() > 1 {
            let (inverse, dirty) = steps.pop().unwrap().perform(doc, &mut self.selected);
            self.redo.push(inverse);
            return dirty;
        }
        self.apply(doc, true)
    }
}

impl Edit {
    fn perform(self, doc: &mut EditableMap, selected: &mut HashSet<Hex>) -> (Edit, bool) {
        match self {
            Edit::Resize(map) => (Edit::Resize(Box::new(doc.swap_geometry(*map))), true),
            Edit::Location(edit) => (Edit::Location(doc.restore_locations(edit)), false),
            Edit::Stroke(steps) => {
                let mut inverse = Vec::with_capacity(steps.len());
                let mut dirty = false;
                for step in steps.into_iter().rev() {
                    let (opposite, changed) = step.perform(doc, selected);
                    inverse.push(opposite);
                    dirty |= changed;
                }
                (Edit::Stroke(inverse), dirty)
            }
            Edit::Selection(delta) => {
                for &hex in &delta {
                    if !selected.remove(&hex) { selected.insert(hex); }
                }
                (Edit::Selection(delta), false)
            }
            Edit::Terrain(changes) => {
                let mut inverse = Changes::new();
                for (hex, code) in changes {
                    if let Some(old) = doc.paint_stored(hex, &code) {
                        inverse.insert(hex, old);
                    }
                }
                (Edit::Terrain(inverse), true)
            }
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
enum Command {
    New,
    Resize,
    Open,
    Save,
    SaveAs,
    SaveAll,
    Revert,
    Close,
    Exit,
    PartialUndo,
    Undo,
    Redo,
    FillSelection,
    SelectAll,
    SelectNone,
    SelectInverse,
    Shuffle,
    Cut,
    Copy,
    Paste,
    ToggleGrid,
    Refresh,
    ToggleHelp,
    Switch(usize),
    Coordinates,
    TerrainCodes,
}

impl Command {
    fn label(self) -> &'static str {
        match self {
            Self::New => "New Map",
            Self::Resize => "Resize Map…",
            Self::Open => "Open Map…",
            Self::Save => "Save Map",
            Self::SaveAs => "Save Map As…",
            Self::SaveAll => "Save All Maps",
            Self::Revert => "Revert Map",
            Self::Close => "Close Map",
            Self::Exit => "Quit to Title Screen",
            Self::PartialUndo => "Partial Undo",
            Self::Undo => "Undo",
            Self::Redo => "Redo",
            Self::FillSelection => "Fill Selection",
            Self::SelectAll => "Select All",
            Self::SelectNone => "Select None",
            Self::SelectInverse => "Invert Selection",
            Self::Shuffle => "Randomize Tiles in Selection",
            Self::Cut => "Cut",
            Self::Copy => "Copy",
            Self::Paste => "Paste",
            Self::ToggleGrid => "Toggle Grid",
            Self::Switch(_) => "Switch Map",
            Self::Refresh => "Refresh Display",
            Self::ToggleHelp => "Show Tool Information",
            Self::Coordinates => "Show Coordinates",
            Self::TerrainCodes => "Show Terrain Codes",
        }
    }
}

fn new_map() -> EditableMap {
    // Wesnoth's default context is 44 by 33 playable hexes with one stored border.
    new_map_dialog::make(44, 33, "Gg").expect("valid blank grass map")
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
    labels: &mut hex_labels::Labels,
    font: Option<&Font>,
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
    labels.begin();
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
                        None,
                    );
                    if !front && let Some(grid) = grid {
                        grid.draw(center, size, None);
                    }
                    if front && history.selected.contains(&hex) {
                        art.image(assets, "editor/selection-overlay.png", Rect::new(left, top, size, size));
                    }
                    if front && region.contains(hex) {
                        art.image(assets, "editor/brush.png", Rect::new(left, top, size, size));
                    }
                    if front && (labels.coordinates || labels.terrain) && doc.map().is_playable(hex)
                        && let Some(code) = doc.map().terrain_at(hex) {
                        labels.draw(hex, code, center, font);
                    }
                }
            }
        }
    }
    // Show brush feedback while the pointer is held. Rebuild connected terrain once per stroke.
    for &at in history.terrain_steps.iter().flat_map(|step| step.keys()).chain(history.stroke.keys()) {
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
    for (id, at) in doc.locations() {
        let [x, y] = layout.tile_origin(at);
        let center = vec2(x + size * 0.5, y + size * 0.5);
        let text = locations::label(id);
        let width = measure_text(&text, None, 17, 1.0).width;
        draw_text(&text, center.x + 1. - width * 0.5, center.y + 6., 17., BLACK);
        draw_text(&text, center.x - width * 0.5, center.y + 5., 17., WHITE);
    }
}


struct Chrome {
    labels: hex_labels::Labels,
    show_grid: bool,
    menu: Option<menubar::Menu>,
    menubar: menubar::Menubar,
    show_help: bool,
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
        can_revert: bool,
        doc: &EditableMap,
        history: &History,
        selection: clipboard::SelectionState,
    ) -> Option<Command> {
        self.tip = None;
        let sidebar = Rect::new(w - side, 0.0, side, h);
        art.panel(
            assets,
            "themes/classic/menubar.png",
            Rect::new(0.0, 0.0, w, 27.0),
        );
        art.panel(
            assets,
            "themes/editor/classic/toolbar.png",
            Rect::new(0.0, 27.0, w - side - 10.0, 41.0),
        );
        art.panel(assets, "themes/editor/classic/toolbar_rightside.png",
            Rect::new(w - side - 10.0, 27.0, 10.0, 41.0));
        art.panel(assets, "themes/classic/sidebar.png", sidebar);
        let tool_action = self.toolbar.draw(w, point, self.menu.is_some(), self.show_grid, self.show_help, &self.labels, doc, history,
            assets, art, &mut self.tip, selection);
        let result = self.menubar.draw(&mut self.menu, point, assets, art, font, names, can_revert).or(tool_action);
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

fn pan_editor_minimap(
    drag: &mut MinimapDrag,
    camera: &mut BoardCamera,
    view: &Observation,
    area: Viewport,
    point: Vec2,
    pressed: Option<MouseButton>,
    is_held: impl FnOnce(MouseButton) -> bool,
) {
    if let Some(world) = drag.update(pressed, is_held, || {
        MinimapGeometry::new(view.width, view.height, area)?.world_point(point)
    }) {
        camera.center_world(world);
    }
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

pub(super) async fn run(root: &Path, core: &[u8], font_family: &str) -> Result<(), Box<dyn Error>> {
    let form = dialog::Dialog::new(root);
    let mut help_label = help_label::HelpLabel::new(root, font_family);
    let assets = AssetCatalog::parse(root, core, b"")?;
    let cfg = wml::parse(core)?;
    let mut palette = Palette::load(&cfg, root)?;
    let mut shapes = shapes::Shapes::load(root, &cfg)?;
    let mut art = Art::new();
    let mut documents = documents::Documents::default();
    let mut view = render::map_observation(documents.list[0].map.map());
    let mut rules = EditorTerrain::default();
    let mut mini = EditorMinimap::new(root);
    let mut minimap_drag = MinimapDrag::default();
    let mut reports = terrain_report::Reports::new(root);
    let mut villages = village_report::Villages::new(root);
    let mut mini_buttons = minimap_buttons::Buttons::load(root);
    let mut rebuild = true;
    let mut clipboard = clipboard::Clipboard::default();
    let mut transform_buttons = clipboard::TransformButtons::default();
    let mut chrome = Chrome {
        labels: hex_labels::Labels::new(root)?,
        show_grid: crate::display_preferences::grid()?,
        show_help: crate::display_preferences::editor_help()?,
        menu: None,
        menubar: menubar::Menubar::new(root),
        tip: None,
        toolbar: Default::default(),
    };
    let grid = render::GridArt::load(&root.join("data/core/images/terrain"), &mut rules);
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
        let can_revert = current.can_revert();
        let documents::Document { map: doc, history, camera, locations, .. } = current;
        if rebuild {
            region.clear();
            shapes.clear_footprint();
            view = render::map_observation(doc.map());
            rules.load_editor(&assets, &view, doc.map());
            villages.update(&view, &assets);
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
        let fit = is_key_pressed(KeyCode::Home);
        update_editor_camera(camera, area, &view, fit);
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
        let (mini_frame, mini_area, mini_background) = minimap_panel(w, h);
        if modal {
            minimap_drag.cancel();
        } else {
            let buttons_handled = match mini_buttons.input(mini_frame, point, camera, &mut mini) {
                Ok(handled) => handled,
                Err(error) => { status = error.to_string(); true }
            };
            if buttons_handled || fit || drawing.is_some() || selecting {
                minimap_drag.cancel();
            } else {
                let pressed = [MouseButton::Left, MouseButton::Middle]
                    .into_iter().find(|&button| is_mouse_button_pressed(button));
                pan_editor_minimap(&mut minimap_drag, camera, &view, mini_area, point, pressed, is_mouse_button_down);
            }
        }
        if !modal && !ctrl && !shift && !is_key_down(KeyCode::LeftAlt)
            && !is_key_down(KeyCode::RightAlt) && matches!(tool, Tool::Paint | Tool::Select) {
            shapes.keys();
        }
        let at = if area.contains(p) {
            camera.layout().and_then(|layout| layout.editor_hex_at(p))
        } else {
            None
        };
        let paste_footprint;
        let footprint: &[Hex] = match (at, tool) {
            (Some(hex), Tool::Paste) if !modal => {
                paste_footprint = clipboard.translated(hex)
                    .map(|(dest, _)| dest).filter(|dest| doc.map().terrain_at(*dest).is_some()).collect::<Vec<_>>();
                &paste_footprint
            }
            (Some(hex), Tool::Paint | Tool::Select) if !modal && (tool == Tool::Paint || !shift) =>
                shapes.footprint(hex, |at| doc.map().terrain_at(at).is_some()),
            _ => &[],
        };
        region.follow(doc.map(), at.filter(|_| !modal && (tool == Tool::Fill || tool == Tool::Select && shift)));
        if tool == Tool::Location && !modal {
            locations.input(at, ctrl, doc, history, camera);
        } else if tool == Tool::Paste && !modal {
            if let Some(hex) = at && is_mouse_button_pressed(MouseButton::Left) {
                rebuild |= clipboard.paste(doc, history, hex);
            }
        } else if tool == Tool::Select && !modal {
            if let Some(hex) = at {
                selecting |= is_mouse_button_pressed(MouseButton::Left);
                let same_brush = last_selection.is_some_and(|(prior, remove, wand)| {
                    remove == ctrl && wand == shift && (prior == hex || shift && region.contains(prior))
                });
                if selecting && !same_brush {
                    if shift {
                        for at in region.hexes() { history.select(at, !ctrl); }
                    } else {
                        for &at in footprint { history.select(at, !ctrl); }
                    }
                    history.finish_selection_step();
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
                shapes::paint(doc, history, footprint, &palette.brushes[i], shift);
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
                &mut chrome.labels,
                form.font.as_ref(),
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
            for &hex in footprint {
                let [x, y] = layout.tile_origin(hex);
                art.image(&assets, "editor/brush.png", Rect::new(x, y, layout.tile_size(), layout.tile_size()));
            }
            if let Some(hex) = at {
                let [x, y] = layout.tile_origin(hex);
                let rect = Rect::new(x, y, layout.tile_size(), layout.tile_size());
                art.image(&assets, "misc/hover-hex.png", rect);
                if tool == Tool::Location && !modal {
                    let padding = rect.w / 12.;
                    art.image(&assets, "icons/action/editor-tool-starting-position_60.png",
                        Rect::new(x + padding, y + padding, rect.w - 2. * padding, rect.h - 2. * padding));
                } else if tool == Tool::Paste && !modal {
                    let inset = layout.tile_size() / 12.0;
                    let size = layout.tile_size() - inset * 2.;
                    art.tinted(&assets, "icons/action/editor-paste_60.png",
                        Rect::new(x + inset, y + inset, size, size), Color::from_rgba(255,255,255,196));
                }
                if tool == Tool::Select && !modal {
                    let icon = if shift { "editor/tool-overlay-select-wand.png" } else { "editor/tool-overlay-select-brush.png" };
                    art.tinted(&assets, icon, rect, Color::from_rgba(255, 255, 255, 196));
                }
            }
        }
        let selection = clipboard::SelectionState::new(doc, history, tool, &clipboard);
        let mut command = chrome.draw(
            w,
            h,
            sidebar,
            point,
            &assets,
            &mut art,
            form.font.as_ref(),
            &names,
            can_revert,
            doc,
            history,
            selection,
        );
        villages.draw(w, &mut art, &assets, form.font.as_ref());
        art.image(&assets, mini_background, mini_frame);
        mini.draw_map(&view, mini_area, area, camera.layout(), None, None, &[]);
        reports.draw(w, h, at, doc, &palette, &assets, form.font.as_ref());
        let zoom = camera.layout().map_or(72., |layout| layout.tile_size());
        if let Some(tip) = mini_buttons.draw(mini_frame, (!modal).then_some(point), zoom, mini.layers) {
            chrome.tip = Some(tip);
        }
        let compact = w < 1024.0 || h < 768.0;
        let tools = if compact { Rect::new(w - sidebar, 219., sidebar, 92.) }
                    else { Rect::new(w - sidebar, 304., sidebar, 155.) };
        tool = tool.panel(tools, compact, point, &mut art, &assets, &mut chrome.tip, !clipboard.empty());
        shapes.panel(tools, compact, matches!(tool, Tool::Paint | Tool::Select) && !modal, point, &mut art, &assets, &mut chrome.tip);
        transform_buttons.draw(tools, compact, point, !modal && tool == Tool::Paste,
            &mut clipboard, &mut art, &assets, &mut chrome.tip);
        let palette_y = if compact { 312.0 } else { 467.0 };
        let panel = Rect::new(w - sidebar, palette_y, sidebar, h - palette_y);
        if tool == Tool::Location {
            let panel_pointer = if modal { vec2(-1., -1.) } else { point };
            if let Some(action) = locations.panel(panel, compact, panel_pointer, &assets, &mut art, form.font.as_ref()) {
                match action {
                    locations::Action::Add => {
                        if let Some(id) = files::prompt(&form, "New Location Identifier", "", "OK").await {
                            if !locations.add(&id) { status = "Invalid location id".into(); }
                        }
                    }
                    locations::Action::Delete => history.place(doc, locations.selected(), None),
                    locations::Action::GoTo => locations.go_to(doc, camera),
                }
            }
        } else {
            palette.panel(panel, compact, point, &assets, &mut art, form.font.as_ref());
        }
        let text = if let Some(tip) = chrome.tip {
            Some(tip.to_owned())
        } else if !status.is_empty() {
            Some(status.clone())
        } else if !chrome.show_help {
            None
        } else if tool == Tool::Location {
            Some("Left-click to place or clear a location | Ctrl + click to select | Right-click to clear".into())
        } else if tool == Tool::Select {
            Some("Drag to select · Ctrl to deselect · Shift for matching terrain · Shift+F to fill".into())
        } else {
            Some(palette.help().join("\n"))
        };
        if let Some(text) = text {
            let top_row = camera.layout().map_or(0, |layout| {
                let [_, zero_y] = layout.tile_origin(Hex { x: 0, y: 0 });
                ((area.y - zero_y) / layout.tile_size()).floor() as i32
            });
            help_label.draw(&text, form.font.as_ref(), vec2(w, h), area.y, top_row);
        }
        set_default_camera();
        if !modal && !ctrl && !shift && !is_key_down(KeyCode::LeftAlt) && !is_key_down(KeyCode::RightAlt) {
            if is_key_pressed(KeyCode::O) { command = Some(Command::Coordinates); }
            if is_key_pressed(KeyCode::I) { command = Some(Command::TerrainCodes); }
        }
        if ctrl && is_key_pressed(KeyCode::N) {
            command = Some(Command::New);
        }
        if ctrl && is_key_pressed(KeyCode::O) {
            command = Some(Command::Open);
        }
        if ctrl && is_key_pressed(KeyCode::S) {
            command = Some(if shift { Command::SaveAs } else { Command::Save });
        }
        if !ctrl && !shift && (is_key_down(KeyCode::LeftAlt) || is_key_down(KeyCode::RightAlt))
            && is_key_pressed(KeyCode::S) { command = Some(Command::SaveAll); }
        if ctrl && is_key_pressed(KeyCode::Z) {
            command = Some(if shift { Command::Redo } else { Command::PartialUndo });
        }
        if !modal && !ctrl && !shift && !is_key_down(KeyCode::LeftAlt) && !is_key_down(KeyCode::RightAlt) {
            if is_key_pressed(KeyCode::U) { command = Some(Command::Undo); }
            if is_key_pressed(KeyCode::R) { command = Some(Command::Redo); }
        }
        if ctrl && is_key_pressed(KeyCode::Y) {
            command = Some(Command::Redo);
        }
        if !modal && shift && !ctrl && is_key_pressed(KeyCode::F) {
            command = Some(Command::FillSelection);
        }
        let shortcut_modifier = if cfg!(target_os = "macos") {
            is_key_down(KeyCode::LeftSuper) || is_key_down(KeyCode::RightSuper)
        } else {
            is_key_down(KeyCode::LeftControl) || is_key_down(KeyCode::RightControl)
        };
        if !modal && shortcut_modifier && !shift && !is_key_down(KeyCode::LeftAlt) && !is_key_down(KeyCode::RightAlt) {
            for (key, cmd) in [(KeyCode::A, Command::SelectAll), (KeyCode::I, Command::SelectInverse),
                (KeyCode::X, Command::Cut), (KeyCode::C, Command::Copy), (KeyCode::V, Command::Paste)] {
                if is_key_pressed(key) { command = Some(cmd); }
            }
        }
        if !modal && shift && !ctrl && is_key_pressed(KeyCode::R) {
            command = Some(Command::Shuffle);
        }
        if !modal && shortcut_modifier && !shift && !is_key_down(KeyCode::LeftAlt)
            && !is_key_down(KeyCode::RightAlt) && is_key_pressed(KeyCode::G) {
            command = Some(Command::ToggleGrid);
        }
        if is_key_pressed(KeyCode::Escape) && !palette_menu {
            if chrome.menu.take().is_none() {
                command = Some(Command::Exit);
            }
        }
        if !modal && !ctrl && !shift && (is_key_down(KeyCode::LeftAlt) || is_key_down(KeyCode::RightAlt))
            && is_key_pressed(KeyCode::R) {
            command = Some(Command::Resize);
        }
        if let Some(cmd) = command.filter(|&cmd| selection.enabled(cmd)) {
            if !matches!(cmd, Command::ToggleGrid | Command::ToggleHelp | Command::Coordinates | Command::TerrainCodes) {
                minimap_drag.cancel();
                drawing = None;
                selecting = false;
                last_selection = None;
                history.finish_selection();
                rebuild |= history.finish();
            }
            match cmd {
                Command::Refresh => { rebuild = true; }
                Command::ToggleHelp => {
                    match crate::display_preferences::save_editor_help(!chrome.show_help) {
                        Ok(()) => chrome.show_help = !chrome.show_help,
                        Err(error) => status = error.to_string(),
                    }
                }
                Command::Coordinates | Command::TerrainCodes => {
                    if let Err(e) = chrome.labels.toggle(cmd) { status = e.to_string(); }
                }
                Command::SelectAll | Command::SelectNone | Command::SelectInverse =>
                    clipboard::select(doc, history, cmd),
                Command::Copy | Command::Cut => {
                    clipboard.copy(doc, &history.selected);
                    if cmd == Command::Cut {
                        rebuild |= history.fill_selected(doc, &palette.brushes[palette.bg]);
                    }
                }
                Command::Paste => tool = Tool::Paste,
                Command::Shuffle => rebuild |= clipboard::shuffle(doc, history),
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
                Command::PartialUndo => {
                    rebuild |= history.partial_undo(doc);
                    status.clear();
                }
                Command::Undo | Command::Redo => {
                    rebuild |= history.apply(doc, cmd == Command::Undo);
                    status.clear();
                }
                Command::Save | Command::SaveAs | Command::SaveAll => {
                    let targets = documents.save_targets(cmd == Command::SaveAll);
                    let mut saved = Vec::new();
                    let mut errors = Vec::new();
                    for index in targets {
                        let record = &documents.list[index];
                        let bytes = record.map.text();
                        let other_paths = documents.list.iter().enumerate().filter(|(i, _)| *i != index)
                            .filter_map(|(_, item)| item.path.clone()).collect::<Vec<_>>();
                        let result = files::save(&folder, root, record.path.as_deref(), cmd == Command::SaveAs,
                            &other_paths, &form, record.name(), &bytes).await;
                        match result {
                            Ok(Some(path)) => {
                                let document = &mut documents.list[index];
                                document.path = Some(fs::canonicalize(&path)?);
                                document.map = EditableMap::parse(bytes)?;
                                saved.push(path.display().to_string());
                            }
                            Ok(None) => {}
                            Err(error) => errors.push(format!("{}: {error}", record.name())),
                        }
                    }
                    if !errors.is_empty() { status = errors.join("\n"); }
                    else if !saved.is_empty() { status = format!("Saved {}", saved.join(", ")); }
                }
                Command::Revert => {
                    if can_revert && files::confirm(&form).await {
                        match documents.list[documents.current].revert() {
                            Ok(()) => { status.clear(); rebuild = true; }
                            Err(error) => status = error.to_string(),
                        }
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
                Command::Resize => {
                    if let Some(choice) = new_map_dialog::dimensions(root, &assets, &mut art,
                        form.font.as_ref(), doc, true).await?
                        && choice.size != [doc.map().width() as usize, doc.map().height() as usize] {
                        let fill = (!choice.copy_edges).then_some(palette.brushes[palette.bg].code.as_str());
                        let next = doc.resized(choice.size, choice.anchor, fill)?;
                        history.undo.push(Edit::Resize(Box::new(doc.swap_geometry(next))));
                        history.redo.clear();
                        rebuild = true;
                        status.clear();
                    }
                }
                Command::New => {
                    if let Some([width,height]) = new_map_dialog::show(root, &assets, &mut art,
                        form.font.as_ref(), doc).await? {
                        let map = new_map_dialog::make(width, height, &palette.brushes[palette.bg].code)?;
                        documents.new_map(map);
                        rebuild = true;
                        status.clear();
                    }
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
    fn resize_undo_and_redo_follow_the_current_saved_geometry() {
        let mut map = new_map();
        let mut history = History::default();
        let next = map.resized([8,6], 0, None).unwrap();
        history.undo.push(Edit::Resize(Box::new(map.swap_geometry(next))));
        assert!(map.dirty());
        assert!(history.apply(&mut map, true));
        assert!(!map.dirty());
        assert!(history.apply(&mut map, false));
        map = EditableMap::parse(map.text()).unwrap();
        assert!(history.apply(&mut map, true));
        assert_eq!((map.map().width(),map.map().height()), (44,33));
        assert!(map.dirty());
        assert!(history.apply(&mut map, false));
        assert_eq!((map.map().width(),map.map().height()), (8,6));
        assert!(!map.dirty());
    }

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
    fn editor_minimap_centers_each_held_position_inside_the_drawn_map() {
        let doc = new_map();
        let view = render::map_observation(doc.map());
        let (_, canvas, _) = minimap_panel(1200.0, 820.0);
        let mut camera = BoardCamera::default();
        update_editor_camera(&mut camera, map_viewport(1200.0, 820.0), &view, false);
        let mut drag = MinimapDrag::default();
        let mut input = |point, pressed, held| {
            pan_editor_minimap(&mut drag, &mut camera, &view, canvas, point, pressed,
                |button| held && button == MouseButton::Left);
            camera.checkpoint().unwrap()
        };
        let letterbox = vec2(1100.0, 20.0);
        let first = vec2(1080.0, 75.0);
        let second = vec2(1146.0, 130.0);
        let opening = input(first, None, false);
        assert_eq!(input(letterbox, Some(MouseButton::Left), true), opening);
        assert_eq!(input(first, None, true), opening);
        let after_press = input(first, Some(MouseButton::Left), true);
        assert_ne!(after_press, opening);
        let after_drag = input(second, None, true);
        assert!(after_drag[0] > after_press[0] && after_drag[1] > after_press[1]);
        assert_eq!(after_drag[2], opening[2]);
        assert_eq!(input(letterbox, None, true), after_drag);
        assert_eq!(input(first, None, true), after_drag);
        assert_eq!(input(first, None, false), after_drag);
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
    fn partial_undo_reverses_one_complete_brush_stamp_and_redoes_each_step() {
        let mut doc = new_map();
        let mut history = History::default();
        let specs = wml::parse("[terrain_type]\nstring=Ww\neditor_group=water\nsymbol_image=water\n[/terrain_type]").unwrap();
        let palette = Palette::new(&specs, &Default::default()).unwrap();
        let [a,b,c,d,e] = [0,1,2,3,4].map(|x| Hex { x, y: 0 });
        let brush = &palette.brushes[0];
        shapes::paint(&mut doc, &mut history, &[a,b], brush, false);
        shapes::paint(&mut doc, &mut history, &[b,c], brush, false);
        shapes::paint(&mut doc, &mut history, &[d,e], brush, false);
        assert!(history.finish());
        assert!(history.partial_undo(&mut doc));
        assert!([a,b,c].iter().all(|&at| doc.map().terrain_at(at) == Some("Ww")));
        assert!([d,e].iter().all(|&at| doc.map().terrain_at(at) == Some("Gg")));
        assert!(history.apply(&mut doc, true));
        assert!(!doc.dirty());
        assert!(history.apply(&mut doc, false));
        assert_eq!(doc.map().terrain_at(c), Some("Ww"));
        assert_eq!(doc.map().terrain_at(d), Some("Gg"));
        assert!(history.apply(&mut doc, false));
        assert!([a,b,c,d,e].iter().all(|&at| doc.map().terrain_at(at) == Some("Ww")));
        assert!(history.partial_undo(&mut doc));
        assert_eq!(doc.map().terrain_at(e), Some("Gg"));
    }

    #[test]
    fn selection_partial_undo_restores_one_movement_including_overlapping_hexes() {
        let mut doc = new_map();
        let mut history = History::default();
        let [a,b,c] = [0,1,2].map(|x| Hex { x, y: 0 });
        for at in [a,b,c] { history.select(at, true); }
        history.finish_selection_step();
        for at in [a,c] { history.select(at, false); }
        history.finish_selection();
        history.partial_undo(&mut doc);
        assert_eq!(history.selected, HashSet::from([a,b,c]));
        history.partial_undo(&mut doc);
        assert!(history.selected.is_empty());
        history.apply(&mut doc, false);
        history.apply(&mut doc, false);
        assert_eq!(history.selected, HashSet::from([b]));
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
