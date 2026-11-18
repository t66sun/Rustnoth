use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

use engine_lite::{fixed_match::{Action, MatchState}, hex::Hex, observation::Observation};
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
const HEADER_HEIGHT: f32 = 68.0;
const ART_HEX_SIZE: f32 = 72.0;

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
    paths: HashMap<PathBuf, Option<Texture2D>>,
    terrain: BTreeMap<String, Vec<Texture2D>>,
    units: BTreeMap<String, BTreeMap<usize, Option<Texture2D>>>,
    poisoned: Option<Texture2D>,
    slowed: Option<Texture2D>,
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
        let team_colors = fs::read(&palette_path)
            .and_then(|bytes| TeamColors::parse(&bytes))
            .map_err(|error| eprintln!("{} ({error})", palette_path.display()))
            .ok();
        Self {
            paths: HashMap::new(),
            terrain: BTreeMap::new(),
            units: BTreeMap::new(),
            poisoned: load_texture(&wesnoth_root.join("images/misc/poisoned.png"), None),
            slowed: load_texture(&wesnoth_root.join("images/misc/slowed.png"), None),
            fog: (1..=3).filter_map(|i| load_texture(&terrain_dir.join(format!("fog/fog{i}.png")), None)).collect(),
            shroud: load_texture(&terrain_dir.join("void/void.png"), None),
            cover: Vec::new(),
            team_colors,
            font,
        }
    }

    pub fn load(&mut self, observation: &Observation, assets: &AssetCatalog) {
        self.cover = map_cover(observation);
        for tile in &observation.tiles {
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
    pub selected: Option<Hex>,
    pub actions: &'a [Action],
    pub message: Option<&'a str>,
    pub assets: &'a AssetCatalog,
    pub menu: Option<&'a Menu>,
}

fn sidebar_width(width: f32) -> f32 {
    (width * 0.23).clamp(148.0, 250.0)
}

pub fn board_viewport() -> Viewport {
    Viewport {
        x: 0.0,
        y: HEADER_HEIGHT,
        width: screen_width() - sidebar_width(screen_width()),
        height: screen_height() - HEADER_HEIGHT,
    }
}

pub fn end_turn_button(width: f32, height: f32, can_end_turn: bool) -> Option<Rect> {
    let left = (width - sidebar_width(width)).max(0.0) + 12.0;
    let bounds = Rect::new(left, height - 62.0, width - left - 12.0, 46.0);
    (can_end_turn && bounds.w >= 112.0 && bounds.y >= HEADER_HEIGHT).then_some(bounds)
}

pub fn draw_end_turn_button(bounds: Rect, highlighted: bool, art: &BoardArt) {
    let (background, outline) = if highlighted {
        (BUTTON_HIGHLIGHTED, TEXT)
    } else {
        (BUTTON, BORDER)
    };
    draw_rectangle(bounds.x, bounds.y, bounds.w, bounds.h, background);
    draw_rectangle_lines(bounds.x, bounds.y, bounds.w, bounds.h, 1.0, outline);
    let baseline = bounds.y + 28.0;
    label(&art.font, "End turn", bounds.x + 10.0, baseline, 16, TEXT);
    let hint_width = measure_text("Space", art.font.as_ref(), 12, 1.0).width;
    label(&art.font, "Space", bounds.x + bounds.w - hint_width - 10.0, baseline, 12, MUTED);
}

pub fn menu_area() -> Rect {
    let width = screen_width();
    let sidebar = sidebar_width(width);
    Rect::new(width - sidebar + 18.0, 333.0, sidebar - 36.0, (screen_height() - 409.0).max(0.0))
}

pub fn draw(
    title: &str,
    state: MatchState,
    observation: Option<&Observation>,
    layout: Option<&BoardLayout>,
    art: &BoardArt,
    movement: MovementView<'_>,
) {
    clear_background(BACKGROUND);
    let width = screen_width();
    let height = screen_height();
    let panel_width = sidebar_width(width);
    let panel_x = (width - panel_width).max(0.0);
    if let (Some(view), Some(layout)) = (observation, layout) {
        draw_board(view, layout, art, &movement);
    }
    draw_rectangle(0.0, 0.0, width, HEADER_HEIGHT, PANEL);
    draw_rectangle(panel_x, HEADER_HEIGHT, panel_width, (height - HEADER_HEIGHT).max(0.0), PANEL);
    draw_line(0.0, HEADER_HEIGHT, width, HEADER_HEIGHT, 1.0, BORDER);
    draw_line(panel_x, HEADER_HEIGHT, panel_x, height, 1.0, BORDER);
    label(&art.font, title, 24.0, 44.0, 29, TEXT);
    let inset = panel_x + 18.0;
    let baseline = HEADER_HEIGHT + 40.0;
    match state {
        MatchState::Playing(turn) => {
            label(&art.font, &format!("Round {}", turn.round), inset, baseline, 25, TEXT);
            label(&art.font, &format!("Side {}", turn.side), inset, baseline + 38.0, 19, TEXT);
            if let Some(gold) = observation.and_then(|view| view.gold) {
                label(&art.font, &format!("Gold {gold}"), inset, baseline + 69.0, 18, MUTED);
            }
        }
        MatchState::Won(side) => {
            label(&art.font, &format!("Side {side} won"), inset, baseline, 22, TEXT);
        }
        MatchState::AlliedVictory(sides) => {
            let winners = sides.iter().map(usize::to_string).collect::<Vec<_>>().join(", ");
            label(&art.font, &format!("Sides {winners} won"), inset, baseline, 22, TEXT);
        }
        MatchState::Draw => label(&art.font, "Draw", inset, baseline, 22, TEXT),
    }
    if let Some(unit) = observation
        .and_then(|view| view.units.iter().find(|unit| Some(unit.position) == movement.selected))
    {
        draw_line(inset, baseline + 93.0, width - 18.0, baseline + 93.0, 1.0, BORDER);
        let name = movement.assets.unit_name(&unit.type_id).unwrap_or(&unit.type_id);
        label(&art.font, name, inset, baseline + 126.0, 20, TEXT);
        label(&art.font, &format!("HP {}", unit.hitpoints), inset, baseline + 155.0, 17, MUTED);
        if let Some(moves) = unit.moves_left {
            label(&art.font, &format!("Moves {moves}"), inset, baseline + 181.0, 17, MUTED);
        }
        let condition = match (unit.poisoned, unit.slowed) {
            (true, true) => "Poisoned, Slowed",
            (true, false) => "Poisoned",
            (false, true) => "Slowed",
            (false, false) => "",
        };
        if !condition.is_empty() {
            label(&art.font, condition, inset, baseline + 195.0, 13, TEXT);
        }
    }
    if movement.menu.is_none()
        && let Some(view) = observation
    {
        let top = baseline + 216.0;
        let available = (height - 104.0 - top).min(168.0);
        let viewport = Viewport { x: inset, y: top, width: panel_width - 36.0, height: available };
        if available >= 72.0
            && let Some(mini) = BoardLayout::fit(view.width, view.height, viewport)
        {
            draw_minimap(view, &mini, art);
        }
    }
    if let Some(message) = movement.message {
        label(&art.font, message, inset, height - 76.0, 16, ERROR);
    }
    if let Some(menu) = movement.menu {
        let area = menu_area();
        label(&art.font, menu.title(), area.x, area.y - 13.0, 18, TEXT);
        let pointer: Vec2 = mouse_position().into();
        for row in menu.rows(area) {
            let bounds = row.bounds;
            let highlighted = row.focused || bounds.contains(pointer);
            let fill = if highlighted { BUTTON_HIGHLIGHTED } else { BUTTON };
            draw_rectangle(bounds.x, bounds.y, bounds.w, bounds.h, fill);
            draw_rectangle_lines(bounds.x, bounds.y, bounds.w, bounds.h, 1.0, if highlighted { TEXT } else { BORDER });
            label(&art.font, row.label, bounds.x + 10.0, bounds.y + 27.0, 17, TEXT);
        }
    }
}

fn draw_terrain(view: &Observation, layout: &BoardLayout, art: &BoardArt, min_marker: f32) {
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
            draw_circle(
                center.x + size * 0.27,
                center.y - size * 0.26,
                (size * 0.055).max(min_marker),
                side_color(art.team_colors.as_ref(), owner),
            );
        }
    }
}

fn draw_minimap(view: &Observation, layout: &BoardLayout, art: &BoardArt) {
    draw_terrain(view, layout, art, 0.0);
    let size = layout.tile_size();
    for unit in &view.units {
        let [x, y] = layout.tile_origin(unit.position);
        draw_circle(x + size * 0.5, y + size * 0.5, size * 0.33, side_color(art.team_colors.as_ref(), unit.side));
    }
    draw_cover(layout, art);
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
    draw_terrain(view, layout, art, 2.0);
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
    if let Some(menu) = movement.menu {
        let [x, y] = layout.tile_origin(menu.target);
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
            assert!(button.x >= width - sidebar_width(width));
            assert!(button.x + button.w < width);
            assert!(button.y > HEADER_HEIGHT && button.y + button.h < height);
            assert!(button.contains(vec2(button.x + button.w / 2.0, button.y + button.h / 2.0)));
            assert!(!button.contains(vec2(0.0, button.y + button.h / 2.0)));
            assert!(end_turn_button(width, height, false).is_none());
        }
    }
}
