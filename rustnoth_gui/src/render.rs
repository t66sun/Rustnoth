mod atlas;
mod frame;
mod surface;
mod tutorial_print;
pub(crate) use frame::StatusBox;
pub(crate) mod minimap;
pub(crate) use minimap::save::save_preview;
mod lighting;
use atlas::AtlasSprite;
use frame::{BattlefieldFrame, FrameArt};

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

use engine_lite::{fixed_match::{Action, MatchState}, hex::Hex, observation::{Observation, ObservedUnit}};
use engine_lite::time_of_day::TimeOfDay;
use macroquad::prelude::*;
use rustnoth_gui::{
    assets::AssetCatalog,
    layout::{BoardLayout, Viewport},
    team_colors::TeamColors,
};
use crate::menu::Menu;

mod label;
pub(crate) use label::Label;
pub mod objectives;
pub mod attack_dialog;
mod dialogue;
mod tutorial_hint;
pub use dialogue::draw_dialogue;
mod transitions;
mod unit_status;
mod standing;
pub(super) mod combat_motion;
mod walking;
mod ground;
mod rules;
mod terrain_clip;
pub(crate) use rules::{map_observation, map_png, Rules as EditorTerrain};
pub(crate) use minimap::Art as EditorMinimap;
mod border;
mod fog_edges;
mod reach;
mod hover;
mod footsteps;

const BACKGROUND: Color = Color::from_rgba(19, 22, 18, 255);
const PANEL: Color = Color::from_rgba(34, 39, 31, 255);
const BORDER: Color = Color::from_rgba(105, 100, 76, 255);
const TEXT: Color = Color::from_rgba(242, 235, 211, 255);
const MUTED: Color = Color::from_rgba(196, 195, 169, 255);
const BUTTON: Color = Color::from_rgba(57, 68, 51, 255);
const BUTTON_HIGHLIGHTED: Color = Color::from_rgba(79, 94, 68, 255);
const UNKNOWN_TERRAIN: Color = Color::from_rgba(51, 65, 50, 255);
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

#[derive(Default)]
struct CoverTiles {
    cells: Vec<(Hex, Cover)>,
    fog_end: usize,
    playable_end: usize,
}

impl CoverTiles {
    fn extend_border(&mut self, view: &Observation, border: &engine_lite::map::KnownBorder<'_>) {
        self.cells.extend(engine_lite::map::border_positions(view.width, view.height).filter_map(|at| {
            match border.get(&at) {
                Some((_, true)) => None,
                Some((_, false)) => Some((at, Cover::Fog)),
                None => Some((at, Cover::Shroud)),
            }
        }));
    }

    fn rows<'a>(&'a self, layout: &BoardLayout, clip: terrain_clip::TerrainClip) -> impl Iterator<Item = &'a (Hex, Cover)> {
        // The playable fog and shroud are drawn in separate row-ordered passes.
        let (playable, border) = self.cells.split_at(self.playable_end);
        let (fog, shroud) = playable.split_at(self.fog_end);
        clip.rows(layout, fog, |(hex, _)| hex.y).iter()
            .chain(clip.rows(layout, shroud, |(hex, _)| hex.y))
            .chain(border)
    }
}

fn map_cover(view: &Observation) -> CoverTiles {
    let mut cover = Vec::new();
    for tile in &view.tiles {
        if !tile.visible {
            cover.push((tile.position, Cover::Fog));
        }
    }
    let fog_end = cover.len();
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
    let playable_end = cover.len();
    CoverTiles { cells: cover, fog_end, playable_end }
}

pub(crate) struct GridArt([Option<Texture2D>; 2]);

impl GridArt {
    pub(crate) fn load(terrain_dir: &Path) -> Self {
        Self(["grid-top.png", "grid-bottom.png"].map(|name| load_texture(&terrain_dir.join(name), None)))
    }

    pub(crate) fn draw(&self, center: Vec2, size: f32, clip: Option<Viewport>) {
        for grid in self.0.iter().flatten() {
            draw_centered(grid, center, size, clip);
        }
    }
}

pub struct BoardArt {
    surface: surface::Surface,
    frame: FrameArt,
    minimap: minimap::Art,
    lighting: lighting::Lighting,
    paths: HashMap<PathBuf, Option<Texture2D>>,
    terrain: BTreeMap<String, Vec<AtlasSprite>>,
    terrain_paths: SpriteTextures<AtlasSprite>,
    terrain_texture_extent: Vec2,
    transitions: transitions::Transitions,
    ground: ground::Ground,
    rules: rules::Rules,
    border_background: Option<Texture2D>,
    grid: GridArt,
    pub show_grid: bool,
    units: BTreeMap<String, BTreeMap<usize, Option<Texture2D>>>,
    sprite_paths: SpriteTextures<Texture2D>,
    standing: HashMap<String, HashMap<usize, standing::Sprites>>,
    combat_pose: Option<combat_motion::Pose>,
    moving: HashMap<String, HashMap<usize, standing::Sprites>>,
    pub(crate) walk: walking::Walk,
    pub(crate) briefing_highlight: Option<Hex>,
    speed: crate::display_preferences::speed::Speed,
    ellipse_colors: Option<TeamColors>,
    ellipses: HashMap<String, HashMap<(usize, bool, bool, bool), [Option<Texture2D>; 2]>>,
    flag_dir: PathBuf,
    flag_colors: Option<TeamColors>,
    flags: BTreeMap<usize, [Option<Texture2D>; FLAG_FRAMES]>,
    map_items: Vec<(Hex, Texture2D, bool)>,
    tutorial_zone_image: std::cell::OnceCell<Option<Texture2D>>,
    unit_status: unit_status::StatusArt,
    unit_order: UnitDrawOrder,
    hover: hover::Art,
    footsteps: footsteps::Art,
    poisoned: Option<Texture2D>,
    slowed: Option<Texture2D>,
    petrified: Option<Texture2D>,
    fog_edges: fog_edges::Edges,
    fog: Vec<Texture2D>,
    shroud: Option<Texture2D>,
    unreachable: Option<Texture2D>,
    linger: Option<Texture2D>,
    pub(super) linger_color: Option<[i32; 3]>,
    cover: CoverTiles,
    team_colors: Option<TeamColors>,
    pub(super) font: Option<Font>,
    dialogue_font: Option<Font>,
    tutorial_hint: tutorial_hint::Art,
    #[cfg(windows)]
    tutorial_print: tutorial_print::Art,
    #[cfg(windows)]
    dialogue_text: dialogue::native::Art,
}

impl BoardArt {
    pub(crate) fn animation_rate(&self) -> f32 {
        self.speed.animation_rate(is_key_down(KeyCode::LeftShift) || is_key_down(KeyCode::RightShift))
    }

    pub fn start_walk(&mut self, route: Vec<Hex>, view: &Observation, assets: &AssetCatalog) {
        if route.len() < 2 { return; }
        let Some(unit) = view.units.iter().find(|u| Some(&u.position) == route.last() && u.side == view.side) else { return; };
        self.load_movement(&unit.type_id, unit.side, assets);
        self.walk.start(route, (get_time() * 1000.0) as u64, self.animation_rate());
    }

    fn load_movement(&mut self, type_id: &str, side: usize, assets: &AssetCatalog) {
        let palette = self.team_colors.as_ref().map(|colors| (colors, side));
        self.moving.entry(type_id.to_owned()).or_default().entry(side).or_insert_with(|| {
            standing::Sprites::load(assets.movement_animations(type_id), |image| {
                let path = assets.resolve_image(Path::new(image))?;
                self.sprite_paths.texture(path, palette)
            })
        });
    }

    pub fn start_observed_walk(&mut self, walk: engine_lite::fixed_match::walk::VisibleWalk, assets: &AssetCatalog) {
        self.load_movement(&walk.unit_type, walk.side, assets);
        if walk.visible_destination.is_none() {
            let palette = self.team_colors.as_ref().map(|colors| (colors, walk.side));
            self.units.entry(walk.unit_type.clone()).or_default().entry(walk.side).or_insert_with(|| {
                let path = assets.unit_image(&walk.unit_type)?;
                self.sprite_paths.texture(path, palette)
            });
        }
        self.walk.start_observed(walk, (get_time() * 1000.0) as u64, self.animation_rate());
    }

    pub fn new(wesnoth_root: &Path, speed: crate::display_preferences::speed::Speed,
        locale: &crate::title_screen::locale::Locale) -> Result<Self, macroquad::Error> {
        let font = fs::read(wesnoth_root.join("fonts/DejaVuSans.ttf"))
            .ok()
            .and_then(|bytes| load_ttf_font_from_bytes(&bytes).ok());
        let palette_path = wesnoth_root.join("data/core/team-colors.cfg");
        let terrain_dir = wesnoth_root.join("data/core/images/terrain");
        let palette_bytes = fs::read(&palette_path)
            .map_err(|error| eprintln!("{} ({error})", palette_path.display()))
            .ok();
        let palette = |parse: fn(&[u8]) -> std::io::Result<TeamColors>| {
            parse(palette_bytes.as_deref()?)
                .map_err(|error| eprintln!("{} ({error})", palette_path.display()))
                .ok()
        };
        Ok(Self {
            surface: surface::Surface::new()?,
            frame: FrameArt::load(wesnoth_root, locale),
            minimap: minimap::Art::new(wesnoth_root),
            lighting: lighting::Lighting::new(&terrain_dir.join("light"))?,
            paths: HashMap::new(),
            terrain: BTreeMap::new(),
            terrain_paths: SpriteTextures::default(),
            terrain_texture_extent: Vec2::ZERO,
            transitions: transitions::Transitions::default(),
            ground: ground::Ground::new(wesnoth_root),
            rules: rules::Rules::default(),
            border_background: load_texture(&terrain_dir.join("off-map/background.png"), None),
            grid: GridArt::load(&terrain_dir),
            show_grid: crate::display_preferences::grid().unwrap_or(false),
            units: BTreeMap::new(),
            sprite_paths: SpriteTextures::default(),
            standing: HashMap::new(),
            combat_pose: None,
            moving: HashMap::new(),
            walk: walking::Walk::default(),
            briefing_highlight: None,
            speed,
            ellipse_colors: palette(TeamColors::parse_ellipses),
            ellipses: HashMap::new(),
            flag_dir: wesnoth_root.join("data/core/images/flags"),
            flag_colors: palette(TeamColors::parse_flags),
            flags: BTreeMap::new(),
            map_items: Vec::new(),
            tutorial_zone_image: Default::default(),
            unit_status: unit_status::StatusArt::load(wesnoth_root, palette_bytes.as_deref()),
            unit_order: UnitDrawOrder::default(),
            footsteps: footsteps::Art::load(wesnoth_root),
            hover: hover::Art::load(wesnoth_root, palette_bytes.as_deref()),
            poisoned: load_texture(&wesnoth_root.join("images/misc/poisoned.png"), None),
            slowed: load_texture(&wesnoth_root.join("images/misc/slowed.png"), None),
            petrified: load_texture(&wesnoth_root.join("images/misc/petrified.png"), None),
            fog_edges: fog_edges::Edges::load(&terrain_dir),
            fog: (1..=3).filter_map(|i| load_texture(&terrain_dir.join(format!("fog/fog{i}.png")), None)).collect(),
            shroud: load_texture(&terrain_dir.join("void/void.png"), None),
            unreachable: load_texture(&terrain_dir.join("darken.png"), None),
            linger: load_texture(&terrain_dir.join("darken-linger.png"), None),
            linger_color: None,
            cover: CoverTiles::default(),
            team_colors: palette(TeamColors::parse),
            tutorial_hint: tutorial_hint::Art::new(wesnoth_root),
            #[cfg(windows)]
            tutorial_print: tutorial_print::Art::default(),
            dialogue_font: fs::read(wesnoth_root.join("fonts/Lato-Regular.ttf"))
                .ok().and_then(|bytes| load_ttf_font_from_bytes(&bytes).ok()).or_else(|| font.clone()),
            #[cfg(windows)]
            dialogue_text: dialogue::native::Art::new(wesnoth_root),
            font,
        })
    }

    pub fn load(&mut self, observation: &Observation, assets: &AssetCatalog, session: &rustnoth_gui::session::Session<'_>) {
        self.frame.clear_unit_report();
        self.lighting.update(observation);
        self.map_items.clear();
        for (position, image) in session.map_item_images() {
            let unlit = image.strip_suffix("~NO_TOD_SHIFT()");
            let image = unlit.unwrap_or(image);
            let path = Path::new(image);
            if !path.components().all(|c| matches!(c, std::path::Component::Normal(_)))
                || path.extension().is_none_or(|ext| ext != "png") || image.contains(['~', ':', '\\']) {
                continue;
            }
            if let Some(texture) = assets.resolve_image(path).and_then(|p| get_texture(&mut self.paths, p)) {
                self.map_items.push((position, texture, unlit.is_none()));
            }
        }
        let border = session.known_border();
        self.cover = map_cover(observation);
        self.cover.extend_border(observation, &border);
        self.fog_edges.update(observation, &border, &self.cover.cells);
        self.minimap.load(observation, assets);
        if self.rules.load(assets, observation, &border) {
            self.transitions.load(observation, assets, &mut self.rules, &mut self.terrain_paths);
            for tile in &observation.tiles {
                if self.rules.has_base(tile.position) { continue; }
                let mut texture = |path| self.terrain_paths.terrain(path, &mut self.rules);
                self.ground.load(&tile.terrain, assets, &mut texture);
                self.terrain.entry(tile.terrain.clone()).or_insert_with(|| {
                    assets.terrain_images(&tile.terrain).into_iter().filter_map(texture).collect()
                });
            }
        }
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
        }
        self.terrain_texture_extent = texture_extent(
            self.flags.values().flatten().flatten()
                .chain(self.grid.0.iter().flatten()),
        )
        .max(atlas_extent(self.terrain.values().flatten()))
        .max(self.ground.texture_extent())
        .max(self.transitions.texture_extent())
        .max(self.rules.texture_extent());
        for unit in &observation.units {
            if let Some(prefix) = assets.ellipse_prefix(&unit.type_id) {
                for selected in [false, true] {
                    let key = (unit.side, unit.can_recruit, unit.emits_zoc, selected);
                    self.ellipses.entry(prefix.to_owned()).or_default().entry(key).or_insert_with(|| {
                        ellipse_files(prefix, unit.can_recruit, unit.emits_zoc, selected).map(|file| {
                            let path = assets.resolve_image(Path::new(&file))?;
                            load_texture(&path, self.ellipse_colors.as_ref().map(|colors| (colors, unit.side)))
                        })
                    });
                }
            }
            let palette = self.team_colors.as_ref().map(|colors| (colors, unit.side));
            self.standing.entry(unit.type_id.clone()).or_default().entry(unit.side).or_insert_with(|| {
                standing::Sprites::load(assets.standing_animations(&unit.type_id), |image| {
                    let path = assets.resolve_image(Path::new(image))?;
                    self.sprite_paths.texture(path, palette)
                })
            });
            self.units
                .entry(unit.type_id.clone())
                .or_default()
                .entry(unit.side)
                .or_insert_with(|| {
                    let path = assets.unit_image(&unit.type_id)?;
                    self.sprite_paths.texture(path, palette)
                });
        }
    }
}

struct SpriteTextures<T>(HashMap<(PathBuf, Option<usize>), Option<T>>);

impl<T> Default for SpriteTextures<T> {
    fn default() -> Self {
        Self(HashMap::new())
    }
}

impl SpriteTextures<Texture2D> {
    fn texture(&mut self, path: PathBuf, palette: Option<(&TeamColors, usize)>) -> Option<Texture2D> {
        self.load(path, palette.map(|(_, side)| side), |path| load_texture(path, palette))
    }
}

impl SpriteTextures<AtlasSprite> {
    fn terrain(&mut self, path: PathBuf, rules: &mut rules::Rules) -> Option<AtlasSprite> {
        self.load(path, None, |path| {
            let bytes = fs::read(path)
                .map_err(|error| eprintln!("{} ({error})", path.display()))
                .ok()?;
            let pixels = crate::artwork::rgba(&bytes)
                .map_err(|error| eprintln!("{} ({error})", path.display()))
                .ok()?;
            rules.add_fallback_sprite(&pixels)
        })
    }
}

impl<T: Clone> SpriteTextures<T> {
    // Base sprites, standing frames and walking frames can share the same
    // installed bitmap. Team recoloring is the only change at these call sites.
    fn load(&mut self, path: PathBuf, side: Option<usize>, decode: impl FnOnce(&Path) -> Option<T>) -> Option<T> {
        self.0.entry((path, side)).or_insert_with_key(|(path, _)| decode(path)).clone()
    }
}

fn get_texture(cache: &mut HashMap<PathBuf, Option<Texture2D>>, path: PathBuf) -> Option<Texture2D> {
    cache
        .entry(path)
        .or_insert_with_key(|path| load_texture(path, None))
        .clone()
}

pub(super) fn load_texture(path: &Path, palette: Option<(&TeamColors, usize)>) -> Option<Texture2D> {
    load_recolored(path, palette)
}

fn load_recolored<'a>(path: &Path, palettes: impl IntoIterator<Item = (&'a TeamColors, usize)>) -> Option<Texture2D> {
    let bytes = fs::read(path)
        .map_err(|error| eprintln!("{} ({error})", path.display()))
        .ok()?;
    let mut image = crate::artwork::image(&bytes)
        .map_err(|error| eprintln!("{} ({error})", path.display()))
        .ok()?;
    for (colors, side) in palettes {
        colors.recolor(side, &mut image.bytes);
    }
    let texture = Texture2D::from_image(&image);
    texture.set_filter(FilterMode::Linear);
    Some(texture)
}

pub struct MovementView<'a> {
    pub route: Option<&'a engine_lite::fixed_match::move_preview::MovePreview>,
    pub session: &'a rustnoth_gui::session::Session<'a>,
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

pub fn draw_turn_button(bounds: Rect, highlighted: bool, enabled: bool, art: &mut BoardArt, text: &str) {
    frame::turn_button(art, bounds, highlighted, enabled, text);
}

pub fn menu_area() -> Rect {
    BattlefieldFrame::new(screen_width(), screen_height()).menu()
}

pub fn draw<T>(
    state: MatchState,
    observation: Option<&Observation>,
    layout: Option<&BoardLayout>,
    art: &mut BoardArt,
    movement: MovementView<'_>,
    overlay: impl FnOnce(&mut BoardArt) -> T,
) -> T {
    art.surface.begin();
    clear_background(BACKGROUND);
    let frame = BattlefieldFrame::new(screen_width(), screen_height());
    if let Some(texture) = &art.border_background { border::backdrop(texture, frame.map); }
    if let (Some(view), Some(layout)) = (observation, layout) {
        art.unit_order.update(&view.units);
        draw_board(view, layout, art, &movement);
    }
    frame::draw(&frame, state, observation, layout, art, &movement);
    art.tutorial_hint.draw(movement.session.tutorial_hint(), frame.map, art.dialogue_font.as_ref());
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
    let result = overlay(art);
    art.surface.present();
    result
}

fn draw_terrain(view: &Observation, layout: &BoardLayout, art: &BoardArt, flag_frame: (usize, [i32; 3]), tod: &str, viewport: Viewport, clip: terrain_clip::TerrainClip) {
    let now = (get_time() * 1000.0) as u64;
    let size = layout.tile_size();
    for tile in clip.rows(layout, &view.tiles, |tile| tile.position.y) {
        let [x, y] = layout.tile_origin(tile.position);
        let center = vec2(x + size / 2.0, y + size / 2.0);
        if !clip.contains(center) { continue; }
        art.lighting.terrain(tile.position, x, y, size);
        if !art.rules.has_base(tile.position) {
            let layers = &art.terrain[&tile.terrain];
            let ground = art.ground.texture(&tile.terrain, tile.position);
            if let Some(base) = ground.or_else(|| layers.first()) {
                draw_atlas_centered(base, center, size, Some(viewport));
            } else if visible_rect(viewport, vec2(x, y), Vec2::splat(size)) {
                draw_poly(center.x, center.y, 6, size / 2.0, 0.0, UNKNOWN_TERRAIN);
            }
            art.transitions.draw(tile.position, center, size, viewport);
            for overlay in layers.iter().skip(1) {
                draw_atlas_centered(overlay, center, size, Some(viewport));
            }
        }
        art.rules.draw(tile.position, center, size, false, tod, now, Some(viewport));
        if art.show_grid {
            art.lighting.flat(flag_frame.1);
            art.grid.draw(center, size, Some(viewport));
        }
        if let Some(owner) = tile.village_owner {
            art.lighting.flat(flag_frame.1);
            if let Some(flag) = art.flags.get(&owner).and_then(|frames| frames[flag_frame.0].as_ref()) {
                draw_centered(flag, center, size, Some(viewport));
                continue;
            }
            let (offset, radius) = terrain_clip::village_marker(size, 2.0);
            let marker = center + offset;
            if !visible_rect(viewport, marker - Vec2::splat(radius), Vec2::splat(2. * radius)) { continue; }
            draw_circle(
                marker.x, marker.y, radius,
                side_color(art.team_colors.as_ref(), owner),
            );
        }
    }
    art.lighting.flat([0; 3]);
}

pub fn minimap_point(view: &Observation, pointer: Vec2) -> Option<[f32; 2]> {
    let frame = BattlefieldFrame::new(screen_width(), screen_height());
    minimap::Geometry::new(view.width, view.height, frame.minimap)?.world_point(pointer)
}

fn draw_cover(layout: &BoardLayout, art: &BoardArt, viewport: Viewport) {
    let size = layout.tile_size();
    let extent = texture_extent(art.fog.iter().chain(art.shroud.iter()));
    let clip = terrain_clip::TerrainClip::new(viewport, size, extent);
    for &(hex, ref cover) in art.cover.rows(layout, clip) {
        let [x, y] = layout.tile_origin(hex);
        let center = vec2(x + size * 0.5, y + size * 0.5);
        if !clip.contains(center) { continue; }
        let texture = match cover {
            Cover::Fog if !art.fog.is_empty() => Some(&art.fog[(hex.x + hex.y).rem_euclid(art.fog.len() as i32) as usize]),
            Cover::Fog => None,
            Cover::Shroud => art.shroud.as_ref(),
        };
        if let Some(texture) = texture {
            draw_centered(texture, center, size, Some(viewport));
        } else if visible_rect(viewport, vec2(x, y), Vec2::splat(size)) {
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

impl BoardArt {
    pub(crate) fn draw_map_label(&self, layout: &BoardLayout, hex: Hex, label: &str, highlight: bool, visible_in_fog: bool) {
        if self.cover.cells.iter().any(|(at, cover)| *at == hex && (*cover == Cover::Shroud || !visible_in_fog)) { return; }
        let view = board_viewport();
        let [x, y] = layout.tile_origin(hex);
        let size = layout.tile_size();
        if x < view.x || y < view.y || x + size > view.x + view.width || y + size > view.y + view.height { return; }
        if highlight { self.hover.draw_empty_hex(hex, layout); }
        let font = self.font.as_ref();
        let left = x + (size - measure_text(label, font, 14, 1.).width) * 0.5;
        let baseline = y + size * 0.8;
        crate::dialog::text(font, label, left + 1., baseline + 1., 14, BLACK);
        crate::dialog::text(font, label, left, baseline, 14, WHITE);
    }
}

// Odd columns are half a hex lower than even columns at the same map y.
fn draw_row(hex: Hex) -> (i32, i32) { (hex.y, hex.x.rem_euclid(2)) }

#[derive(Default)]
struct UnitDrawOrder {
    units: Vec<(usize, Hex)>,
    #[cfg(test)]
    sorts: usize,
}

impl UnitDrawOrder {
    fn update(&mut self, units: &[ObservedUnit]) {
        // Combat draws the opening positions even after the live view was refreshed.
        if self.units.len() == units.len()
            && self.units.iter().all(|&(index, at)| units[index].position == at)
        {
            return;
        }
        self.units.clear();
        self.units.extend(units.iter().enumerate().map(|(index, unit)| (index, unit.position)));
        self.units.sort_unstable_by_key(|&(_, at)| (draw_row(at), at.x));
        #[cfg(test)]
        {
            self.sorts += 1;
        }
    }

    fn standing<'a>(&'a self, units: &'a [ObservedUnit], moving: Option<Hex>) -> impl Iterator<Item = &'a ObservedUnit> {
        self.units.iter().filter_map(move |&(index, at)| (Some(at) != moving).then(|| &units[index]))
    }
}

fn draw_board(view: &Observation, layout: &BoardLayout, art: &BoardArt, movement: &MovementView<'_>) {
    let size = layout.tile_size();
    let now = (get_time() * 1000.0) as u64;
    let viewport = board_viewport();
    let terrain_clip = terrain_clip::TerrainClip::new(viewport, size, art.terrain_texture_extent);
    let border_period = movement.global_time.map_or("", |t| t.id.as_str());
    art.lighting.flat(movement.global_time.map_or([0; 3], |period| period.color));
    for hex in engine_lite::map::border_positions(view.width, view.height) {
        let [x,y] = layout.tile_origin(hex);
        let center = vec2(x + size / 2.0, y + size / 2.0);
        if terrain_clip.contains(center) {
            art.rules.draw(hex, center, size, false, border_period, now, Some(viewport));
        }
    }
    let frame = village_flag_frame(now);
    let period = movement.global_time.map_or("", |t| t.id.as_str());
    let global_color = movement.global_time.map_or([0; 3], |time| time.color);
    draw_terrain(view, layout, art, (frame, global_color), period, viewport, terrain_clip);
    let observed_tile = |hex: Hex| {
        view.tiles.binary_search_by_key(&(hex.y, hex.x), |tile| (tile.position.y, tile.position.x))
            .ok().map(|i| &view.tiles[i])
    };
    for (position, texture, use_time_color) in &art.map_items {
        let tile = if *use_time_color { observed_tile(*position) } else { None };
        art.lighting.item(tile);
        let [x, y] = layout.tile_origin(*position);
        draw_centered(texture, vec2(x + size / 2.0, y + size / 2.0), size, Some(viewport));
    }
    if let Some(center) = movement.session.dialogue().and_then(|line| line.zone_center)
        && let Some(texture) = art.tutorial_zone_image.get_or_init(|| {
            let path = movement.assets.resolve_image(Path::new("misc/highlight-hex.png"))?;
            load_texture(&path, None)
        })
    {
        for tile in center.neighbors().filter_map(observed_tile) {
            art.lighting.item(Some(tile));
            let [x, y] = layout.tile_origin(tile.position);
            draw_centered(texture, vec2(x + size / 2.0, y + size / 2.0), size, Some(viewport));
        }
    }
    art.lighting.flat([0; 3]);
    let enemies = movement.session.enemy_reach();
    let selected = movement.selected.filter(|_| enemies.is_none());
    let cached = std::ptr::eq(movement.actions, movement.session.legal_actions())
        .then(|| movement.session.action_hints(selected));
    let custom;
    let hints = if let Some(hints) = cached.as_deref() {
        hints
    } else {
        // Replay, combat animation and the tutorial briefing supply their own view.
        custom = rustnoth_gui::session::ActionHints::from_actions(selected, movement.actions);
        &custom
    };
    if let Some(mask) = art.unreachable.as_ref()
        && (enemies.is_some() || selected.is_some()) {
        let attack_target = movement.session.hovered_attack().map(|plan| plan.target);
        let direct_target = selected.and_then(|_| {
            movement.menu.and_then(|menu| menu.target).or_else(|| {
                let (x, y) = mouse_position();
                viewport.contains([x, y]).then(|| layout.hex_at([x, y])).flatten()
            }).filter(|&at| hints.can_attack(at))
        });
        for tile in view.tiles.iter().filter(|tile| {
            Some(tile.position) != attack_target && Some(tile.position) != direct_target
                && !enemies.is_some_and(|reach| reach.counts.contains_key(&tile.position))
                && !selected.is_some_and(|_| hints.is_reachable(tile.position))
        }) {
            let [x, y] = layout.tile_origin(tile.position);
            draw_centered(mask, vec2(x + size / 2.0, y + size / 2.0), size, Some(viewport));
        }
    }
    if let Some(route) = movement.route { art.footsteps.draw_steps(layout, route, viewport); }
    let (pointer_x, pointer_y) = mouse_position();
    let hover_art = art.briefing_highlight.map(|hex| art.hover.at(view, hex))
        .or_else(|| art.hover.focus(view, layout, viewport, pointer_x, pointer_y));
    if let Some(focus) = &hover_art { focus.draw(layout, false); }
    let walking = art.walk.pose(now);
    let terrain_at = |at: Hex| observed_tile(at).map(|tile| tile.terrain.as_str());
    let raised = |kind: &str, from: Hex, to: Hex, offset: f32| {
        let mut point = Vec2::from_array(layout.tile_origin(from));
        let start = terrain_at(from).unwrap_or("");
        let end = if offset == 0.0 { start } else {
            point = point.lerp(Vec2::from_array(layout.tile_origin(to)), offset);
            terrain_at(to).unwrap_or("")
        };
        let height = movement.assets.unit_elevation(kind, start, end, offset);
        point - vec2(0., (height as f32 * size / ART_HEX_SIZE).trunc())
    };
    let origin = |unit: &engine_lite::observation::ObservedUnit| {
        walking.filter(|p| p.unit == Some(unit.position)).map_or_else(
            || raised(&unit.type_id, unit.position, unit.position, 0.0),
            |p| raised(&unit.type_id, p.from, p.to, p.fraction))
    };
    let draw_unit = |unit: &engine_lite::observation::ObservedUnit| {
        art.lighting.flat([0; 3]);
        let center = origin(unit) + Vec2::splat(size / 2.0);
        let marker = side_color(art.team_colors.as_ref(), unit.side);
        if let Some(prefix) = movement.assets.ellipse_prefix(&unit.type_id) {
            let selected = movement.selected == Some(unit.position)
                || enemies.is_some_and(|reach| reach.threatening.contains(&unit.position));
            let key = (unit.side, unit.can_recruit, unit.emits_zoc, selected);
            if let Some(halves) = art.ellipses.get(prefix).and_then(|variants| variants.get(&key)) {
                for texture in halves.iter().flatten() { draw_centered(texture, center, size, Some(viewport)); }
            }
        }
        let base = art
            .units
            .get(&unit.type_id)
            .and_then(|sides| sides.get(&unit.side))
            .and_then(Option::as_ref);
        let motion = walking.filter(|p| p.unit == Some(unit.position));
        let direction = motion.and_then(|p| p.direction).unwrap_or_else(|| standing::facing(unit));
        let walking_frame = motion.and_then(|p| {
            art.moving.get(&unit.type_id)?.get(&unit.side)?.frame(direction, terrain_at(p.to), p.elapsed)
        });
        let standing_frame = art.standing.get(&unit.type_id).and_then(|teams| teams.get(&unit.side))
            .and_then(|sprites| {
                if unit.petrified { return None; }
                sprites.frame(direction, terrain_at(unit.position), now)
            });
        let posed = art.combat_pose.as_ref().and_then(|p| p.sprite(unit.position, |to, offset|
            raised(&unit.type_id, unit.position, to, offset) + Vec2::splat(size * 0.5)));
        let (at, attack_frame) = posed.unwrap_or((center, None));
        if let Some((texture, flipped)) = attack_frame.or(walking_frame).or(standing_frame).or_else(|| base.map(|image| (image, standing::west(direction)))) {
            draw_centered_flipped(texture, at, size, flipped, Some(viewport));
        } else if visible_rect(viewport, center - Vec2::splat(size * 0.23), Vec2::splat(size * 0.46)) {
            draw_circle(center.x, center.y, size * 0.23, marker);
        }
    };

    let draw_foreground = |at: Hex| {
        let [x, y] = layout.tile_origin(at);
        let center = vec2(x + size / 2.0, y + size / 2.0);
        if !terrain_clip.contains(center) { return; }
        if (0..view.width).contains(&at.x) && (0..view.height).contains(&at.y) {
            art.lighting.terrain(at, x, y, size);
        } else {
            art.lighting.flat(global_color);
        }
        art.rules.draw(at, center, size, true, period, now, Some(viewport));
    };
    // Native unit_default precedes terrain_fg *within* each half-row. A rear
    // castle wall must already be in place when a raised unit overlaps it.
    let moving = walking.and_then(|p| p.unit);
    let mut foreground = terrain_clip.rows(layout, &art.rules.foreground, |at| at.y)
        .iter().copied().peekable();
    for unit in art.unit_order.standing(&view.units, moving) {
        while let Some(at) = foreground.next_if(|at| draw_row(*at) < draw_row(unit.position)) {
            draw_foreground(at);
        }
        draw_unit(unit);
    }
    foreground.for_each(draw_foreground);
    art.lighting.flat([0; 3]);
    if let Some(unit) = moving.and_then(|at| view.units.iter().find(|u| u.position == at)) {
        draw_unit(unit);
    }

    // An observer can watch the visible portion of a walk that ends back in fog.
    if let (Some(pose), Some((id, side))) = (walking, &art.walk.transient_sprite) {
        let direction = pose.direction.unwrap_or(engine_lite::hex::Direction::South);
        let terrain = terrain_at(pose.to);
        let sprite = art.moving.get(id).and_then(|s| s.get(side)).and_then(|s| s.frame(direction, terrain, pose.elapsed))
            .or_else(|| art.units.get(id)?.get(side)?.as_ref().map(|t| (t, standing::west(direction))));
        if let Some((tex, flipped)) = sprite {
            let origin = raised(id, pose.from, pose.to, pose.fraction);
            draw_centered_flipped(tex, origin + Vec2::splat(size * 0.5), size, flipped, Some(viewport));
        }
    }

    // Draw above every sprite so a tall neighboring unit cannot hide the indicators.
    let (mx, my) = mouse_position();
    let hover = art.briefing_highlight.or_else(|| layout.hex_at([mx, my]));
    let linger_side = art.linger_color.map(|_| movement.session.playing_side());
    for unit in &view.units {
        if let Some(style) = movement.assets.unit_bars(&unit.type_id) {
            let width = art.units.get(&unit.type_id).and_then(|sides| sides.get(&unit.side))
                .and_then(Option::as_ref).map_or(ART_HEX_SIZE, Texture2D::width);
            let orb = unit_status::orb_for(view, unit, hints.is_actionable(unit.position), linger_side);
            let highlighted = [hover, movement.selected].contains(&Some(unit.position));
            let origin = origin(unit);
            art.unit_status.draw(unit, style, origin, size, width, orb, highlighted, viewport);
        }
        let icons = [
            art.poisoned.as_ref().filter(|_| unit.poisoned),
            art.slowed.as_ref().filter(|_| unit.slowed),
            art.petrified.as_ref().filter(|_| unit.petrified),
        ];
        let count = icons.iter().flatten().count();
        let [x, y] = origin(unit).to_array();
        let spacing = (size * 0.30).max(10.0).min(size * 0.37);
        for (index, icon) in icons.into_iter().flatten().enumerate() {
            let slot = index as f32 + 0.5 - count as f32 * 0.5;
            let center = vec2(x + size * 0.5 + slot * spacing, y + size * 0.81);
            if !visible_rect(viewport, center - Vec2::splat(spacing * 0.5), Vec2::splat(spacing)) { continue; }
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
    art.lighting.flat(global_color);
    draw_cover(layout, art, viewport);
    art.fog_edges.draw(layout, viewport);
    art.lighting.flat([0; 3]);
    if let Some(reach) = enemies { reach::draw_counts(reach, layout, viewport, art.font.as_ref()); }
    if let Some(focus) = &hover_art { focus.draw(layout, true); }
    if let Some(plan) = movement.session.hovered_attack() {
        art.footsteps.draw_attack(layout, plan.staging, plan.target, viewport);
    }
    if let Some(pose) = &art.combat_pose { pose.draw_impact(layout, art.font.as_ref(), |at| {
        let unit = view.units.iter().find(|u| u.position == at);
        raised(unit.map_or("", |u| &u.type_id), at, at, 0.0)
    }); }
    if let Some(route) = movement.route { art.footsteps.draw_mark(layout, route, art.font.as_ref()); }
    if let (Some(color), Some(texture)) = (art.linger_color, art.linger.as_ref()) {
        // LAYER_LINGER_OVERLAY is painted after unit bars, fog and shroud.
        // Its coverage only depends on the public map dimensions.
        art.lighting.flat(color);
        for x in -1..=view.width {
            for y in -1..=view.height {
                let center = Vec2::from_array(layout.tile_origin(Hex { x, y })) + Vec2::splat(size / 2.0);
                draw_centered(texture, center, size, Some(viewport));
            }
        }
        art.lighting.flat([0; 3]);
    }
}

fn ellipse_files(prefix: &str, leader: bool, zoc: bool, selected: bool) -> [String; 2] {
    let leader = if leader { "leader-" } else { "" };
    let zoc = if zoc { "" } else { "nozoc-" };
    let selected = if selected { "selected-" } else { "" };
    ["top", "bottom"].map(|half| format!("{prefix}-{leader}{zoc}{selected}{half}.png"))
}

#[cfg(test)]
mod sprite_tests {
    use super::SpriteTextures;
    use std::{cell::Cell, path::Path};

    #[test]
    fn unit_art_uploads_a_shared_sprite_once_per_palette() {
        let mut sprites = SpriteTextures::default();
        let path = Path::new("units/elves-wood/fighter.png");
        let calls = Cell::new(0);
        let upload = |path: &Path| {
            assert!(path.ends_with("fighter.png"));
            calls.set(calls.get() + 1);
            Some(calls.get())
        };
        assert_eq!(sprites.load(path.into(), Some(1), upload), Some(1));
        assert_eq!(sprites.load(path.into(), Some(1), upload), Some(1));
        assert_eq!(sprites.load(path.into(), Some(2), upload), Some(2));
        assert_eq!(sprites.load(path.into(), None, upload), Some(3));
        assert_eq!(sprites.load(path.into(), None, upload), Some(3));
        assert_eq!(calls.get(), 3);
    }
}

#[cfg(test)]
mod ellipse_tests {
    use super::ellipse_files;

    #[test]
    fn picks_the_original_selected_leader_halves_and_non_zoc_variant() {
        assert_eq!(ellipse_files("misc/ellipse-hero", true, false, true),
            ["misc/ellipse-hero-leader-nozoc-selected-top.png", "misc/ellipse-hero-leader-nozoc-selected-bottom.png"]);
        assert_eq!(ellipse_files("misc/ellipse", false, true, false),
            ["misc/ellipse-top.png", "misc/ellipse-bottom.png"]);
    }
}

fn draw_centered(texture: &Texture2D, center: Vec2, hex_size: f32, clip: Option<Viewport>) {
    draw_centered_flipped(texture, center, hex_size, false, clip);
}

fn texture_extent<'a>(textures: impl Iterator<Item = &'a Texture2D>) -> Vec2 {
    textures.map(Texture2D::size).fold(Vec2::ZERO, Vec2::max)
}

fn atlas_extent<'a>(sprites: impl Iterator<Item = &'a AtlasSprite>) -> Vec2 {
    sprites.map(AtlasSprite::size).fold(Vec2::ZERO, Vec2::max)
}

fn draw_atlas_centered(sprite: &AtlasSprite, center: Vec2, hex_size: f32, clip: Option<Viewport>) {
    if let Some(rect) = sprite_rect(center, sprite.size(), hex_size, clip) {
        sprite.draw(rect, WHITE, false);
    }
}

fn visible_rect(view: Viewport, top: Vec2, size: Vec2) -> bool {
    view.width > 0.0 && view.height > 0.0
        && top.x < view.x + view.width && top.y < view.y + view.height
        && top.x + size.x > view.x && top.y + size.y > view.y
}

fn sprite_rect(center: Vec2, source: Vec2, hex_size: f32, clip: Option<Viewport>) -> Option<Rect> {
    let size = source * (hex_size / ART_HEX_SIZE);
    let top = center - size / 2.0;
    clip.is_none_or(|view| visible_rect(view, top, size))
        .then_some(Rect::new(top.x, top.y, size.x, size.y))
}

fn draw_centered_flipped(texture: &Texture2D, center: Vec2, hex_size: f32, flip_x: bool, clip: Option<Viewport>) {
    let Some(rect) = sprite_rect(center, texture.size(), hex_size, clip) else { return };
    draw_texture_ex(texture, rect.x, rect.y, WHITE, DrawTextureParams {
        flip_x,
        dest_size: Some(rect.size()),
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
    fn unit_order_reuses_positions_until_the_drawn_view_changes() {
        let unit = |x, y| ObservedUnit {
            facing: None, position: Hex { x, y }, side: 1, type_id: "Fighter".into(),
            can_recruit: false, emits_zoc: true, hitpoints: 30, max_hitpoints: 30,
            full_action_points: Some(true), experience: 0, max_experience: 20,
            poisoned: false, slowed: false, petrified: false, moves_left: Some(5), attacks_left: Some(1),
        };
        let mut live = [unit(0, 1), unit(1, 0), unit(4, 0), unit(2, 0)];
        let mut order = UnitDrawOrder::default();
        order.update(&live);
        let standing = |order: &UnitDrawOrder, units: &[ObservedUnit], moving| {
            order.standing(units, moving).map(|unit| unit.position.x).collect::<Vec<_>>()
        };
        assert_eq!(standing(&order, &live, None), [2, 4, 1, 0]);
        live[3].hitpoints = 15;
        order.update(&live);
        assert_eq!(standing(&order, &live, Some(Hex { x: 4, y: 0 })), [2, 1, 0]);
        assert_eq!(order.standing(&live, None).next().unwrap().hitpoints, 15);
        assert_eq!(order.sorts, 1);

        let opening = [unit(4, 0), unit(1, 0), unit(0, 0), unit(2, 1)];
        order.update(&opening);
        assert_eq!(standing(&order, &opening, None), [0, 4, 1, 2]);
        assert_eq!(order.sorts, 2);
    }

    #[test]
    fn cover_rows_skip_offscreen_masks_in_their_original_layer_order() {
        use engine_lite::observation::ObservedTile;
        let viewport = Viewport { x: 0.0, y: 0.0, width: 160.0, height: 160.0 };
        let layout = BoardLayout::battlefield(100, 100, viewport).unwrap()
            .native_scale(Some(Hex { x: 50, y: 50 }));
        let tile = |y| ObservedTile {
            position: Hex { x: 50, y }, terrain: "Gg".into(),
            visible: false, village_owner: None, time_color: [0; 3],
        };
        let view = Observation {
            side: 1, allied_sides: vec![1], gold: None, width: 100, height: 100,
            tiles: vec![tile(0), tile(50), tile(99)], units: vec![],
        };
        let mut cover = map_cover(&view);
        cover.extend_border(&view, &Default::default());
        let size = layout.tile_size();
        let clip = terrain_clip::TerrainClip::new(viewport, size, vec2(100.0, 100.0));
        let rows: Vec<_> = cover.rows(&layout, clip).collect();
        assert!(rows.len() < cover.cells.len() / 5);
        let center = |hex| Vec2::from_array(layout.tile_origin(hex)) + size * 0.5;
        let visible = |&(hex, _): &(Hex, Cover)| clip.contains(center(hex));
        assert_eq!(
            rows.into_iter().filter(|cell| visible(cell)).collect::<Vec<_>>(),
            cover.cells.iter().filter(|cell| visible(cell)).collect::<Vec<_>>(),
        );
    }

    #[test]
    fn battlefield_submits_only_sprites_overlapping_the_view_at_current_zoom() {
        let area = Viewport { x: 10.0, y: 27.0, width: 120.0, height: 80.0 };
        let view = Some(area);
        let hex = Vec2::splat(72.0);
        assert_eq!(sprite_rect(vec2(-26.0, 60.0), hex, 72.0, view), None);
        assert_eq!(sprite_rect(vec2(-26.0, 60.0), vec2(144.0, 72.0), 72.0, view),
                   Some(Rect::new(-98.0, 24.0, 144.0, 72.0)));
        assert_eq!(sprite_rect(vec2(130.0, 143.0), hex, 72.0, view), None);
        assert!(sprite_rect(vec2(130.0, 142.5), hex * 2.0, 36.0, view).is_some());
        assert_eq!(sprite_rect(vec2(70.0, 70.0), hex, 72.0, Some(Viewport { width: 0.0, ..area })), None);
        assert!(sprite_rect(vec2(220.0, 200.0), hex, 72.0, None).is_some());
    }

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
            time_color: [0; 3],
        };
        let mut view = Observation {
            side: 1, allied_sides: vec![1], gold: None, width: 3, height: 1,
            tiles: vec![tile(0, true), tile(2, false)], units: Vec::new(),
        };
        assert_eq!(map_cover(&view).cells, [(Hex { x: 2, y: 0 }, Cover::Fog), (Hex { x: 1, y: 0 }, Cover::Shroud)]);
        view.tiles = vec![tile(0, false), tile(1, true), tile(2, true)];
        assert_eq!(map_cover(&view).cells, [(Hex { x: 0, y: 0 }, Cover::Fog)]);
        view.tiles[0].visible = true;
        assert!(map_cover(&view).cells.is_empty());
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
