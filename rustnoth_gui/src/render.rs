use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::{Path, PathBuf},
};

use engine_lite::{fixed_match::MatchState, observation::Observation};
use macroquad::prelude::*;
use rustnoth_gui::{
    assets::AssetCatalog,
    layout::{BoardLayout, Viewport},
};

const BACKGROUND: Color = Color::from_rgba(19, 22, 18, 255);
const PANEL: Color = Color::from_rgba(34, 39, 31, 255);
const BORDER: Color = Color::from_rgba(105, 100, 76, 255);
const TEXT: Color = Color::from_rgba(242, 235, 211, 255);
const MUTED: Color = Color::from_rgba(196, 195, 169, 255);
const UNKNOWN_TERRAIN: Color = Color::from_rgba(51, 65, 50, 255);
const HEADER_HEIGHT: f32 = 68.0;
const ART_HEX_SIZE: f32 = 72.0;

pub struct BoardArt {
    paths: HashMap<PathBuf, Option<Texture2D>>,
    terrain: BTreeMap<String, Vec<Texture2D>>,
    units: BTreeMap<String, Option<Texture2D>>,
    font: Option<Font>,
}

impl BoardArt {
    pub fn new(wesnoth_root: &Path) -> Self {
        let font = fs::read(wesnoth_root.join("fonts/DejaVuSans.ttf"))
            .ok()
            .and_then(|bytes| load_ttf_font_from_bytes(&bytes).ok());
        Self { paths: HashMap::new(), terrain: BTreeMap::new(), units: BTreeMap::new(), font }
    }

    pub fn load(&mut self, observation: &Observation, assets: &AssetCatalog) {
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
            self.units.entry(unit.type_id.clone()).or_insert_with(|| {
                assets.unit_image(&unit.type_id).and_then(|path| get_texture(&mut self.paths, path))
            });
        }
    }
}

fn get_texture(cache: &mut HashMap<PathBuf, Option<Texture2D>>, path: PathBuf) -> Option<Texture2D> {
    cache.entry(path).or_insert_with_key(|path| {
        let bytes = fs::read(path)
            .map_err(|error| eprintln!("{} ({error})", path.display()))
            .ok()?;
        let image = Image::from_file_with_format(&bytes, None)
            .map_err(|error| eprintln!("{} ({error})", path.display()))
            .ok()?;
        let texture = Texture2D::from_image(&image);
        texture.set_filter(FilterMode::Linear);
        Some(texture)
    }).clone()
}

pub fn draw(title: &str, state: MatchState, observation: Option<&Observation>, art: &BoardArt) {
    clear_background(BACKGROUND);
    let width = screen_width();
    let height = screen_height();
    let panel_width = (width * 0.23).clamp(148.0, 250.0);
    let panel_x = (width - panel_width).max(0.0);
    let viewport = Viewport {
        x: 16.0,
        y: HEADER_HEIGHT + 16.0,
        width: panel_x - 32.0,
        height: height - HEADER_HEIGHT - 32.0,
    };

    if let Some(view) = observation
        && let Some(layout) = BoardLayout::fit(view.width, view.height, viewport)
    {
        draw_board(view, &layout, art);
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
        MatchState::Draw => label(&art.font, "Draw", inset, baseline, 22, TEXT),
    }
}

fn draw_board(view: &Observation, layout: &BoardLayout, art: &BoardArt) {
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
            draw_circle(center.x + size * 0.27, center.y - size * 0.26, (size * 0.055).max(2.0), side_color(owner));
        }
    }
    for unit in &view.units {
        let [x, y] = layout.tile_origin(unit.position);
        let center = vec2(x + size / 2.0, y + size / 2.0);
        let marker = side_color(unit.side);
        draw_ellipse(center.x, center.y + size * 0.24, size * 0.23, size * 0.10, 0.0, marker);
        if let Some(texture) = art.units.get(&unit.type_id).and_then(Option::as_ref) {
            draw_centered(texture, center, size);
        } else {
            draw_circle(center.x, center.y, size * 0.23, marker);
        }
    }
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

fn side_color(side: usize) -> Color {
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
