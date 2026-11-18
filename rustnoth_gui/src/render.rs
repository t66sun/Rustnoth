mod frame;
mod minimap;
use frame::{BattlefieldFrame, FrameArt};

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

use engine_lite::{fixed_match::{Action, MatchState}, hex::Hex, observation::Observation};
use engine_lite::time_of_day::TimeOfDay;
use macroquad::prelude::*;
use rustnoth_gui::{
    assets::AssetCatalog,
    layout::{BoardLayout, Viewport},
    team_colors::TeamColors,
};
use crate::menu::Menu;

const BACKGROUND: Color = Color::from_rgba(19, 22, 18, 255);
const PANEL: Color = Color::from_rgba(34, 39, 31, 255);
const BORDER: Color = Color::from_rgba(105, 100, 76, 255);
const TEXT: Color = Color::from_rgba(242, 235, 211, 255);
const MUTED: Color = Color::from_rgba(196, 195, 169, 255);
const BUTTON: Color = Color::from_rgba(57, 68, 51, 255);
const BUTTON_HIGHLIGHTED: Color = Color::from_rgba(79, 94, 68, 255);
const UNKNOWN_TERRAIN: Color = Color::from_rgba(51, 65, 50, 255);
const REACHABLE: Color = Color::from_rgba(240, 214, 106, 190);
const ATTACKABLE: Color = Color::from_rgba(245, 127, 102, 225);
const SELECTED: Color = Color::from_rgba(255, 239, 170, 255);
const ERROR: Color = Color::from_rgba(255, 169, 142, 255);
const ART_HEX_SIZE: f32 = 72.0;
const FLAG_FRAMES: usize = 4;

fn village_flag_frame(elapsed_ms: u64) -> usize {
    ((elapsed_ms / 150) % FLAG_FRAMES as u64) as usize
}

#[derive(Debug, PartialEq, Eq)]
enum Cover {
    Fog,
    Shroud,
}

fn map_cover(view: &Observation) -> Vec<(Hex, Cover)> {
    let mut cover = Vec::new();
    for tile in &view.tiles {
        if !tile.visible {
            cover.push((tile.position, Cover::Fog));
        }
    }
    if view.tiles.len() < view.width as usize * view.height as usize {
        let explored: HashSet<Hex> = view.tiles.iter().map(|tile| tile.position).collect();
        for y in 0..view.height {
            for x in 0..view.width {
                let position = Hex { x, y };
                if !explored.contains(&position) {
                    cover.push((position, Cover::Shroud));
                }
            }
        }
    }
    cover
}

pub struct BoardArt {
    frame: FrameArt,
    minimap: minimap::Art,
    paths: HashMap<PathBuf, Option<Texture2D>>,
    portraits: HashMap<String, Option<Texture2D>>,
    terrain: BTreeMap<String, Vec<Texture2D>>,
    units: BTreeMap<String, BTreeMap<usize, Option<Texture2D>>>,
    flag_dir: PathBuf,
    flag_colors: Option<TeamColors>,
    flags: BTreeMap<usize, [Option<Texture2D>; FLAG_FRAMES]>,
    periods: HashMap<&'static str, (&'static str, Option<Texture2D>)>,
    map_items: Vec<(Hex, Texture2D)>,
    poisoned: Option<Texture2D>,
    slowed: Option<Texture2D>,
    petrified: Option<Texture2D>,
    fog: Vec<Texture2D>,
    shroud: Option<Texture2D>,
    cover: Vec<(Hex, Cover)>,
    team_colors: Option<TeamColors>,
    font: Option<Font>,
}

impl BoardArt {
    pub fn new(wesnoth_root: &Path) -> Self {
        let font = fs::read(wesnoth_root.join("fonts/DejaVuSans.ttf"))
            .ok()
            .and_then(|bytes| load_ttf_font_from_bytes(&bytes).ok());
        let palette_path = wesnoth_root.join("data/core/team-colors.cfg");
        let terrain_dir = wesnoth_root.join("data/core/images/terrain");
        let clock_dir = wesnoth_root.join("data/core/images/misc/time-schedules/default");
        let periods = [
            ("dawn", "Dawn", "dawn"),
            ("morning", "Morning", "morning"),
            ("afternoon", "Afternoon", "afternoon"),
            ("dusk", "Dusk", "dusk"),
            ("first_watch", "First Watch", "firstwatch"),
            ("second_watch", "Second Watch", "secondwatch"),
        ].into_iter().map(|(id, name, file)| {
            let image = load_texture(&clock_dir.join(format!("schedule-{file}.png")), None);
            (id, (name, image))
        }).collect();
        let palette_bytes = fs::read(&palette_path)
            .map_err(|error| eprintln!("{} ({error})", palette_path.display()))
            .ok();
        let palette = |parse: fn(&[u8]) -> std::io::Result<TeamColors>| {
            parse(palette_bytes.as_deref()?)
                .map_err(|error| eprintln!("{} ({error})", palette_path.display()))
                .ok()
        };
        Self {
            frame: FrameArt::load(wesnoth_root),
            minimap: minimap::Art::new(wesnoth_root),
            paths: HashMap::new(),
            portraits: HashMap::new(),
            terrain: BTreeMap::new(),
            units: BTreeMap::new(),
            flag_dir: wesnoth_root.join("data/core/images/flags"),
            flag_colors: palette(TeamColors::parse_flags),
            flags: BTreeMap::new(),
            periods,
            map_items: Vec::new(),
            poisoned: load_texture(&wesnoth_root.join("images/misc/poisoned.png"), None),
            slowed: load_texture(&wesnoth_root.join("images/misc/slowed.png"), None),
            petrified: load_texture(&wesnoth_root.join("images/misc/petrified.png"), None),
            fog: (1..=3).filter_map(|i| load_texture(&terrain_dir.join(format!("fog/fog{i}.png")), None)).collect(),
            shroud: load_texture(&terrain_dir.join("void/void.png"), None),
            cover: Vec::new(),
            team_colors: palette(TeamColors::parse),
            font,
        }
    }

    pub fn load(&mut self, observation: &Observation, assets: &AssetCatalog, items: Vec<(Hex, &str)>) {
        self.map_items.clear();
        for (position, image) in items {
            let path = Path::new(image);
            if !path.components().all(|c| matches!(c, std::path::Component::Normal(_)))
                || path.extension().is_none_or(|ext| ext != "png") || image.contains(['~', ':', '\\']) {
                continue;
            }
            if let Some(texture) = assets.resolve_image(path).and_then(|p| get_texture(&mut self.paths, p)) {
                self.map_items.push((position, texture));
            }
        }
        self.cover = map_cover(observation);
        self.minimap.load(observation, assets);
        for tile in &observation.tiles {
            if let Some(owner) = tile.village_owner
                && let Some(colors) = self.flag_colors.as_ref()
                && colors.marker_color(owner).is_some()
            {
                self.flags.entry(owner).or_insert_with(|| std::array::from_fn(|i| {
                    let path = self.flag_dir.join(format!("flag-{}.png", i + 1));
                    load_texture(&path, Some((colors, owner)))
                }));
            }
            self.terrain.entry(tile.terrain.clone()).or_insert_with(|| {
                assets
                    .terrain_images(&tile.terrain)
                    .into_iter()
                    .filter_map(|path| get_texture(&mut self.paths, path))
                    .collect()
            });
        }
        for unit in &observation.units {
            self.units
                .entry(unit.type_id.clone())
                .or_default()
                .entry(unit.side)
                .or_insert_with(|| {
                    let path = assets.unit_image(&unit.type_id)?;
                    let palette = self.team_colors.as_ref().map(|colors| (colors, unit.side));
                    load_texture(&path, palette)
                });
        }
    }
}

fn get_texture(cache: &mut HashMap<PathBuf, Option<Texture2D>>, path: PathBuf) -> Option<Texture2D> {
    cache
        .entry(path)
        .or_insert_with_key(|path| load_texture(path, None))
        .clone()
}

fn load_texture(path: &Path, palette: Option<(&TeamColors, usize)>) -> Option<Texture2D> {
    let bytes = fs::read(path)
        .map_err(|error| eprintln!("{} ({error})", path.display()))
        .ok()?;
    let mut image = Image::from_file_with_format(&bytes, None)
        .map_err(|error| eprintln!("{} ({error})", path.display()))
        .ok()?;
    if let Some((colors, side)) = palette {
        colors.recolor(side, &mut image.bytes);
    }
    let texture = Texture2D::from_image(&image);
    texture.set_filter(FilterMode::Linear);
    Some(texture)
}

pub struct MovementView<'a> {
    pub global_time: Option<&'a TimeOfDay>,
    pub selected: Option<Hex>,
    pub actions: &'a [Action],
    pub message: Option<&'a str>,
    pub assets: &'a AssetCatalog,
    pub menu: Option<&'a Menu>,
}

pub fn board_viewport() -> Viewport {
    BattlefieldFrame::new(screen_width(), screen_height()).map
}

pub fn end_turn_button(width: f32, height: f32, can_end_turn: bool) -> Option<Rect> {
    let bounds = BattlefieldFrame::new(width, height).end_turn;
    (can_end_turn && bounds.w > 0.0 && bounds.h > 0.0).then_some(bounds)
}

pub fn draw_end_turn_button(bounds: Rect, highlighted: bool, art: &BoardArt) {
    frame::turn_button(art, bounds, highlighted, "End Turn");
}

pub fn menu_area() -> Rect {
    BattlefieldFrame::new(screen_width(), screen_height()).menu()
}

pub fn draw(
    title: &str,
    state: MatchState,
    observation: Option<&Observation>,
    layout: Option<&BoardLayout>,
    art: &mut BoardArt,
    movement: MovementView<'_>,
) {
    clear_background(BACKGROUND);
    let frame = BattlefieldFrame::new(screen_width(), screen_height());
    if let (Some(view), Some(layout)) = (observation, layout) {
        draw_board(view, layout, art, &movement);
    }
    frame::draw(&frame, title, state, observation, layout, art, &movement);
    if let Some(menu) = movement.menu {
        let area = menu_area();
        label(&art.font, menu.title(), area.x, area.y - 13.0, 18, TEXT);
        let pointer: Vec2 = mouse_position().into();
        if let Some(bounds) = menu.advancement_button(area) {
            let pointed = bounds.contains(pointer);
            let fill = if pointed { BUTTON_HIGHLIGHTED } else { BUTTON };
            draw_rectangle(bounds.x, bounds.y, bounds.w, bounds.h, fill);
            draw_rectangle_lines(bounds.x, bounds.y, bounds.w, bounds.h, 1.0, if pointed { TEXT } else { BORDER });
            label(&art.font, "Advance to  [P]", bounds.x + 10.0, bounds.y + 16.0, 13, MUTED);
            let name = menu.preferred_advancement()
                .map(|id| movement.assets.unit_name(id).unwrap_or(id))
                .unwrap_or("Default");
            label(&art.font, name, bounds.x + 10.0, bounds.y + 35.0, 17, TEXT);
        }
        for row in menu.rows(area) {
            let bounds = row.bounds;
            let highlighted = row.focused || bounds.contains(pointer);
            let fill = if highlighted { BUTTON_HIGHLIGHTED } else { BUTTON };
            draw_rectangle(bounds.x, bounds.y, bounds.w, bounds.h, fill);
            draw_rectangle_lines(bounds.x, bounds.y, bounds.w, bounds.h, 1.0, if highlighted { TEXT } else { BORDER });
            if matches!(row.action, Action::Recall { .. } | Action::Disband { .. }) {
                for (line, (baseline, size, color)) in row.label.lines().zip([(15.0, 15, TEXT), (28.0, 12, TEXT), (40.0, 12, MUTED)]) {
                    let available = bounds.w - 16.0;
                    let width = measure_text(line, art.font.as_ref(), size, 1.0).width;
                    let font_scale = (available / width.max(1.0)).min(1.0);
                    draw_text_ex(line, bounds.x + 8.0, bounds.y + baseline, TextParams {
                        font: art.font.as_ref(), font_size: size, font_scale, color, ..Default::default()
                    });
                }
            } else {
                label(&art.font, row.label, bounds.x + 10.0, bounds.y + 27.0, 17, TEXT);
            }
        }
    }
}

fn draw_terrain(view: &Observation, layout: &BoardLayout, art: &BoardArt, flag_frame: Option<usize>) {
    let size = layout.tile_size();
    for tile in &view.tiles {
        let [x, y] = layout.tile_origin(tile.position);
        let center = vec2(x + size / 2.0, y + size / 2.0);
        let layers = &art.terrain[&tile.terrain];
        if layers.is_empty() {
            draw_poly(center.x, center.y, 6, size / 2.0, 0.0, UNKNOWN_TERRAIN);
        } else {
            for texture in layers {
                draw_centered(texture, center, size);
            }
        }
        if let Some(owner) = tile.village_owner {
            if let Some(flag) = flag_frame.and_then(|index| art.flags.get(&owner)?[index].as_ref()) {
                draw_centered(flag, center, size);
                continue;
            }
            let min_marker = if flag_frame.is_some() { 2.0 } else { 0.0 };
            draw_circle(
                center.x + size * 0.27,
                center.y - size * 0.26,
                (size * 0.055).max(min_marker),
                side_color(art.team_colors.as_ref(), owner),
            );
        }
    }
}

pub fn minimap_point(view: &Observation, pointer: Vec2) -> Option<[f32; 2]> {
    let frame = BattlefieldFrame::new(screen_width(), screen_height());
    minimap::Geometry::new(view.width, view.height, frame.minimap)?.world_point(pointer)
}

fn draw_cover(layout: &BoardLayout, art: &BoardArt) {
    let size = layout.tile_size();
    for &(hex, ref cover) in &art.cover {
        let [x, y] = layout.tile_origin(hex);
        let center = vec2(x + size * 0.5, y + size * 0.5);
        let texture = match cover {
            Cover::Fog if !art.fog.is_empty() => Some(&art.fog[(hex.x as usize + hex.y as usize) % art.fog.len()]),
            Cover::Fog => None,
            Cover::Shroud => art.shroud.as_ref(),
        };
        if let Some(texture) = texture {
            draw_centered(texture, center, size);
        } else {
            let color = match cover {
                Cover::Fog => Color::from_rgba(70, 91, 111, 165),
                Cover::Shroud => BLACK,
            };
            let points = [(0.25, 0.0), (0.75, 0.0), (1.0, 0.5), (0.75, 1.0), (0.25, 1.0), (0.0, 0.5)];
            let vertices = points.map(|(dx, dy)| vec2(x + dx * size, y + dy * size));
            for i in 0..vertices.len() {
                draw_triangle(center, vertices[i], vertices[(i + 1) % vertices.len()], color);
            }
        }
    }
}

fn draw_board(view: &Observation, layout: &BoardLayout, art: &BoardArt, movement: &MovementView<'_>) {
    let size = layout.tile_size();
    let frame = village_flag_frame((get_time() * 1000.0) as u64);
    draw_terrain(view, layout, art, Some(frame));
    for (position, texture) in &art.map_items {
        let [x, y] = layout.tile_origin(*position);
        draw_centered(texture, vec2(x + size / 2.0, y + size / 2.0), size);
    }
    let mut choices = BTreeMap::new();
    for action in movement.actions {
        match action {
            Action::Move { from, to } if Some(*from) == movement.selected => {
                choices.insert(*to, REACHABLE);
            }
            Action::Attack(attack) if Some(attack.from) == movement.selected => {
                choices.insert(attack.to, ATTACKABLE);
            }
            Action::Recruit { to, .. } | Action::Recall { to, .. } if movement.selected.is_none() => {
                choices.insert(*to, REACHABLE);
            }
            _ => {}
        }
    }
    for (to, color) in choices {
        let [x, y] = layout.tile_origin(to);
        draw_circle(x + size / 2.0, y + size / 2.0, (size * 0.11).max(2.0), color);
    }
    if let Some(selected) = movement.selected {
        let [x, y] = layout.tile_origin(selected);
        draw_poly_lines(x + size / 2.0, y + size / 2.0, 6, size * 0.48, 0.0, 2.0, SELECTED);
    }
    if let Some(target) = movement.menu.and_then(|menu| menu.target) {
        let [x, y] = layout.tile_origin(target);
        draw_poly_lines(x + size / 2.0, y + size / 2.0, 6, size * 0.42, 0.0, 2.0, SELECTED);
    }
    for unit in &view.units {
        let [x, y] = layout.tile_origin(unit.position);
        let center = vec2(x + size / 2.0, y + size / 2.0);
        let marker = side_color(art.team_colors.as_ref(), unit.side);
        draw_ellipse(center.x, center.y + size * 0.24, size * 0.23, size * 0.10, 0.0, marker);
        let texture = art
            .units
            .get(&unit.type_id)
            .and_then(|sides| sides.get(&unit.side))
            .and_then(Option::as_ref);
        if let Some(texture) = texture {
            draw_centered(texture, center, size);
        } else {
            draw_circle(center.x, center.y, size * 0.23, marker);
        }
    }

    // Draw above every sprite so a tall neighboring unit cannot hide the indicators.
    for unit in &view.units {
        let icons = [
            art.poisoned.as_ref().filter(|_| unit.poisoned),
            art.slowed.as_ref().filter(|_| unit.slowed),
            art.petrified.as_ref().filter(|_| unit.petrified),
        ];
        let count = icons.iter().flatten().count();
        let [x, y] = layout.tile_origin(unit.position);
        let spacing = (size * 0.30).max(10.0).min(size * 0.37);
        for (index, icon) in icons.into_iter().flatten().enumerate() {
            let slot = index as f32 + 0.5 - count as f32 * 0.5;
            let center = vec2(x + size * 0.5 + slot * spacing, y + size * 0.81);
            let original = vec2(icon.width(), icon.height());
            let dimensions = original * (spacing * 0.80 / original.x.max(original.y));
            let top = center - dimensions * 0.5;
            draw_circle(center.x, center.y, spacing * 0.49, BACKGROUND);
            draw_texture_ex(icon, top.x, top.y, WHITE, DrawTextureParams {
                dest_size: Some(dimensions),
                ..DrawTextureParams::default()
            });
        }
    }
    draw_cover(layout, art);
}

fn draw_centered(texture: &Texture2D, center: Vec2, hex_size: f32) {
    let scale = hex_size / ART_HEX_SIZE;
    let size = vec2(texture.width(), texture.height()) * scale;
    let top = center - size / 2.0;
    draw_texture_ex(texture, top.x, top.y, WHITE, DrawTextureParams {
        dest_size: Some(size),
        ..DrawTextureParams::default()
    });
}

fn side_color(palette: Option<&TeamColors>, side: usize) -> Color {
    if let Some([red, green, blue]) = palette.and_then(|colors| colors.marker_color(side)) {
        return Color::from_rgba(red, green, blue, 255);
    }
    const COLORS: [Color; 5] = [
        Color::from_rgba(227, 105, 82, 255),
        Color::from_rgba(108, 163, 231, 255),
        Color::from_rgba(237, 200, 102, 255),
        Color::from_rgba(135, 196, 123, 255),
        Color::from_rgba(172, 128, 216, 255),
    ];
    COLORS[(side - 1) % COLORS.len()]
}

fn label(font: &Option<Font>, message: &str, x: f32, y: f32, size: u16, color: Color) {
    draw_text_ex(message, x, y, TextParams { font: font.as_ref(), font_size: size, color, ..TextParams::default() });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn village_flag_cycles_through_four_original_frames_every_600ms() {
        assert_eq!([0, 149, 150, 300, 450, 599, 600].map(village_flag_frame),
                   [0, 0, 1, 2, 3, 3, 0]);
    }

    #[test]
    fn map_cover_follows_the_viewing_side_and_stops_at_the_board_edge() {
        use engine_lite::observation::ObservedTile;

        let tile = |x, visible| ObservedTile {
            position: Hex { x, y: 0 },
            terrain: "Gg".into(),
            visible,
            village_owner: None,
        };
        let mut view = Observation {
            side: 1, allied_sides: vec![1], gold: None, width: 3, height: 1,
            tiles: vec![tile(0, true), tile(2, false)], units: Vec::new(),
        };
        assert_eq!(map_cover(&view), [(Hex { x: 2, y: 0 }, Cover::Fog), (Hex { x: 1, y: 0 }, Cover::Shroud)]);
        view.tiles = vec![tile(0, false), tile(1, true), tile(2, true)];
        assert_eq!(map_cover(&view), [(Hex { x: 0, y: 0 }, Cover::Fog)]);
        view.tiles[0].visible = true;
        assert!(map_cover(&view).is_empty());
    }

    #[test]
    fn board_markers_use_the_native_palette_and_fall_back_for_unlisted_sides() {
        let palette = TeamColors::parse(
            br#"
[color_range]
    id=red
    default=yes
    rgb=FF0000,FFFFFF,000000,0C2238
[/color_range]
[color_palette]
    magenta=F49AC1
[/color_palette]
"#,
        )
        .unwrap();

        assert_eq!(side_color(Some(&palette), 1), Color::from_rgba(12, 34, 56, 255));
        assert_eq!(side_color(Some(&palette), 2), Color::from_rgba(108, 163, 231, 255));
    }

    #[test]
    fn end_turn_button_stays_inside_the_sidebar_and_disappears_when_unavailable() {
        for (width, height) in [(1200.0, 820.0), (480.0, 320.0)] {
            let button = end_turn_button(width, height, true).unwrap();
            let frame = BattlefieldFrame::new(width, height);
            assert!(button.x >= frame.sidebar.x);
            assert!(button.x + button.w < width);
            assert!(button.y > frame.map.y && button.y + button.h < height);
            assert!(button.contains(vec2(button.x + button.w / 2.0, button.y + button.h / 2.0)));
            assert!(!button.contains(vec2(0.0, button.y + button.h / 2.0)));
            assert!(end_turn_button(width, height, false).is_none());
        }
    }
}
