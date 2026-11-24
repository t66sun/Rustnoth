//! Wesnoth's image-coded minimap, using the terrain symbol images from terrain.cfg.
use super::{side_color, AssetCatalog, BoardLayout, Hex, MovementView, Observation, TeamColors, Viewport};
use engine_lite::{map::ScenarioMap, observation::ObservedTile};
use image::{imageops, Pixel, Rgba, RgbaImage};
use macroquad::prelude::*;
use crate::camera::PointerEvent;
use crate::display_preferences::{MiniLayer, MiniLayers};
pub(crate) mod save;
use std::{collections::{HashMap, HashSet}, path::Path};

const SAMPLE: u32 = 24;
const UNKNOWN: Rgba<u8> = Rgba([0, 0, 0, 255]);
const NEUTRAL_VILLAGE: Rgba<u8> = Rgba([30, 30, 30, 255]); // stock "white" range minimum

#[derive(Default)]
pub(crate) struct Drag {
    active: Option<(MouseButton, [f32; 2])>,
}

impl Drag {
    pub(crate) fn update(
        &mut self,
        pressed: Option<MouseButton>,
        is_held: impl FnOnce(MouseButton) -> bool,
        world_point: impl FnOnce() -> Option<[f32; 2]>,
    ) -> Option<[f32; 2]> {
        let previous = self.active.take();
        let button = pressed.or_else(|| previous.map(|(button, _)| button))?;
        let held = is_held(button);
        if !held && pressed.is_none() { return None; }

        let point = world_point()?;
        self.active = held.then_some((button, point));
        (pressed.is_some() || previous.map(|(_, last)| last) != Some(point)).then_some(point)
    }

    pub(crate) fn cancel(&mut self) {
        self.active = None;
    }

    pub(crate) fn release_if_unheld(&mut self, held: impl FnOnce(MouseButton) -> bool) {
        if self.active.is_some_and(|(button, _)| !held(button)) {
            self.cancel();
        }
    }

    pub(crate) fn pointer_event(
        &mut self,
        event: PointerEvent,
        world_point: impl FnOnce(Vec2) -> Option<[f32; 2]>,
        center: impl FnOnce([f32; 2]),
    ) -> bool {
        let (button, point, previous, held) = match event {
            PointerEvent::Press(button @ (MouseButton::Left | MouseButton::Middle), point) => {
                (button, point, None, true)
            }
            PointerEvent::Move(point) => {
                let Some((button, last)) = self.active else { return false; };
                (button, point, Some(last), true)
            }
            PointerEvent::Release(button, point) => {
                let Some((held, last)) = self.active else { return false; };
                if held != button { return false; }
                (button, point, Some(last), false)
            }
            PointerEvent::Cancel => {
                self.cancel();
                return false;
            }
            _ => return false,
        };
        let next = world_point(Vec2::from_array(point));
        self.active = if held { next.map(|world| (button, world)) } else { None };
        if let Some(world) = next && previous != Some(world) {
            center(world);
        }
        next.is_some() || previous.is_some()
    }
}

pub(crate) struct Geometry {
    bounds: Rect,
    scale: Vec2,
    // Source pixels per display pixel in Wesnoth's 24-pixel minimap surface.
    sample_step: Vec2,
}

impl Geometry {
    pub(crate) fn new(cols: i32, rows: i32, area: Viewport) -> Option<Self> {
        if cols <= 0 || rows <= 0 || area.width < 1.0 || area.height < 1.0 { return None; }
        let extent = vec2(cols as f32 * 0.75, rows as f32);
        let ratio = (area.width / extent.x).min(area.height / extent.y);
        let w = (extent.x * ratio).floor().max(1.0);
        let h = (extent.y * ratio).floor().max(1.0);
        Some(Self {
            bounds: Rect::new(area.x + ((area.width - w) / 2.0).floor(),
                              area.y + ((area.height - h) / 2.0).floor(), w, h),
            scale: vec2(w, h) / extent,
            sample_step: extent * SAMPLE as f32 / vec2(w, h),
        })
    }

    pub(crate) fn world_point(&self, point: Vec2) -> Option<[f32; 2]> {
        self.bounds.contains(point).then(|| {
            let local = (point - self.bounds.point()) / self.scale;
            [local.x + 0.25, local.y + 0.5]
        })
    }

    fn outline(&self, viewport: Viewport, layout: &BoardLayout) -> Rect {
        let [x, y] = layout.tile_origin(Hex { x: 0, y: 0 });
        let zoom = layout.tile_size();
        let local = vec2((viewport.x - x) / zoom, (viewport.y - y) / zoom - 0.25);
        let offset = local * self.scale;
        let pos = self.bounds.point() + vec2(offset.x.trunc(), offset.y.trunc());
        let size = vec2(viewport.width, viewport.height) * self.scale / zoom;
        Rect::new(pos.x - 1.0, pos.y - 1.0, size.x.trunc() + 2.0, size.y.trunc() + 2.0)
    }

    fn tile(&self, hex: Hex) -> Rect {
        let x = hex.x as f32 * 0.75 - 0.25;
        let y = hex.y as f32 + (hex.x % 2) as f32 * 0.5 - 0.5;
        Rect::new(x * self.scale.x, y * self.scale.y, self.scale.x, self.scale.y)
    }
}

struct Symbol {
    id: usize,
    pixels: Option<RgbaImage>,
    village: bool,
    flat: [u8; 3],
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct TerrainTile {
    position: Hex,
    symbol: usize,
    visible: bool,
    village_owner: usize,
}

#[derive(Default)]
struct TerrainLayer {
    size: (i32, i32),
    side: usize,
    allies: Vec<usize>,
    tiles: Vec<TerrainTile>,
    raster: Option<TerrainRaster>,
}

struct TerrainRaster {
    size: Vec2,
    reachable: HashSet<Hex>,
    pixels: RgbaImage,
}

impl TerrainLayer {
    fn load(&mut self, view: &Observation, symbols: &HashMap<String, Symbol>) {
        let tiles = || view.tiles.iter().map(|tile| TerrainTile {
            position: tile.position,
            symbol: symbols[&tile.terrain].id,
            visible: tile.visible,
            village_owner: tile.village_owner.unwrap_or(0),
        });
        let size = (view.width, view.height);
        if self.size == size && self.side == view.side && self.allies == view.allied_sides
            && self.tiles.iter().copied().eq(tiles())
        {
            return;
        }
        self.size = size;
        self.side = view.side;
        self.allies.clone_from(&view.allied_sides);
        self.tiles.clear();
        self.tiles.extend(tiles());
        self.raster = None;
    }

    fn pixels(
        &mut self,
        size: Vec2,
        reachable: HashSet<Hex>,
        paint: impl FnOnce(&HashSet<Hex>) -> RgbaImage,
    ) -> RgbaImage {
        if self.raster.as_ref().is_none_or(|raster| raster.size != size || raster.reachable != reachable) {
            let pixels = paint(&reachable);
            self.raster = Some(TerrainRaster { size, reachable, pixels });
        }
        self.raster.as_ref().unwrap().pixels.clone()
    }
}

pub(crate) struct Art {
    mask: Option<RgbaImage>,
    symbols: HashMap<String, Symbol>,
    texture: Option<Texture2D>,
    terrain: TerrainLayer,
    selected: Option<Hex>,
    pub(crate) layers: MiniLayers,
}

impl Art {
    pub(crate) fn new(root: &Path) -> Self {
        let mask = image::open(root.join("data/core/images/terrain/alphamask.png"))
            .ok().map(|image| image.to_rgba8());
        Self {
            mask, symbols: HashMap::new(), texture: None, terrain: TerrainLayer::default(),
            selected: None, layers: MiniLayers::load(),
        }
    }

    pub(crate) fn toggle(&mut self, layer: MiniLayer) -> std::io::Result<()> {
        self.layers.toggle(layer, &crate::display_preferences::config_directory()?)?;
        self.texture = None;
        if layer != MiniLayer::Units {
            self.terrain.raster = None;
        }
        Ok(())
    }

    pub(crate) fn load(&mut self, view: &Observation, assets: &AssetCatalog) {
        // The upload includes the current units. The terrain beneath it can
        // survive an action that leaves all of its painting inputs unchanged.
        self.texture = None;
        self.load_symbols(view, assets);
        self.terrain.load(view, &self.symbols);
    }

    fn load_symbols(&mut self, view: &Observation, assets: &AssetCatalog) {
        for tile in &view.tiles {
            if self.symbols.contains_key(tile.terrain.as_str()) {
                continue;
            }
            let id = self.symbols.len();
            self.symbols.entry(tile.terrain.clone()).or_insert_with(|| {
                let (layers, village) = assets.minimap_terrain(&tile.terrain);
                let mut layers = layers.iter().map(|path| image::open(path).ok().map(|img| img.to_rgba8()));
                let pixels = layers.next().flatten().map(|mut base| {
                    for overlay in layers.flatten() {
                        let x = (base.width().saturating_sub(overlay.width()) / 2).into();
                        let y = (base.height().saturating_sub(overlay.height()) / 2).into();
                        imageops::overlay(&mut base, &overlay, x, y);
                    }
                    if let Some(mask) = self.mask.as_ref().filter(|m| m.dimensions() == base.dimensions()) {
                        for (pixel, mask) in base.pixels_mut().zip(mask.pixels()) {
                            pixel[3] = pixel[3].min(mask[3]);
                        }
                    }
                    // scale_surface_sharp takes floor(dst * scale) at both resize stages.
                    let dx = base.width() as f32 / SAMPLE as f32;
                    let dy = base.height() as f32 / SAMPLE as f32;
                    RgbaImage::from_fn(SAMPLE, SAMPLE, |x, y| *base.get_pixel(
                        (x as f32 * dx) as u32, (y as f32 * dy) as u32,
                    ))
                });
                Symbol { id, pixels, village, flat: assets.minimap_color(&tile.terrain) }
            });
        }
    }

    /// Build the create-game minimap from the public scenario map, before a match exists.
    pub(crate) fn skirmish_preview(&mut self, assets: &AssetCatalog, map: &ScenarioMap, size: f32) -> Option<Texture2D> {
        let pixels = self.skirmish_pixels(assets, map, size)?;
        let texture = Texture2D::from_rgba8(pixels.width() as u16, pixels.height() as u16, pixels.as_raw());
        texture.set_filter(FilterMode::Nearest);
        Some(texture)
    }

    fn skirmish_pixels(&mut self, assets: &AssetCatalog, map: &ScenarioMap, size: f32) -> Option<RgbaImage> {
        let width = map.width();
        let height = map.height();
        let geometry = Geometry::new(width, height, Viewport { x: 0.0, y: 0.0, width: size, height: size })?;
        let mut tiles = Vec::new();
        for y in 0..height {
            for x in 0..width {
                let position = Hex { x, y };
                let terrain = map.terrain_at(position)?.to_owned();
                tiles.push(ObservedTile { position, terrain, visible: true, village_owner: None, time_color: [0; 3] });
            }
        }
        let view = Observation { width, height, side: 1, gold: None,
            allied_sides: vec![], tiles, units: vec![] };
        self.load_symbols(&view, assets);
        Some(raster(&view, &geometry, &self.symbols, None, &HashSet::new(), MiniLayers::default()))
    }

    pub(super) fn draw(&mut self, view: &Observation, area: Viewport, viewport: Viewport,
                       layout: Option<&BoardLayout>, colors: Option<&TeamColors>, movement: &MovementView<'_>) {
        self.draw_map(view, area, viewport, layout, colors, movement.selected, movement.actions);
    }

    pub(crate) fn draw_map(&mut self, view: &Observation, area: Viewport, viewport: Viewport,
                       layout: Option<&BoardLayout>, colors: Option<&TeamColors>, selected: Option<Hex>, actions: &[engine_lite::fixed_match::Action]) {
        let Some(geometry) = Geometry::new(view.width, view.height, area) else { return };
        draw_rectangle(area.x, area.y, area.width, area.height, Color::from_rgba(31, 31, 23, 255));
        let bounds = geometry.bounds;
        if self.selected != selected || !self.texture.as_ref().is_some_and(|t| t.width() == bounds.w && t.height() == bounds.h) {
            self.selected = selected;
            let mut reachable = HashSet::new();
            if let Some(selected) = selected {
                reachable.insert(selected);
                for action in actions {
                    if let engine_lite::fixed_match::Action::Move { from, to } = action && *from == selected {
                        reachable.insert(*to);
                    }
                }
            }
            let mut pixels = self.terrain.pixels(bounds.size(), reachable, |reachable| {
                raster_terrain(view, &geometry, &self.symbols, colors, reachable, self.layers)
            });
            paint_units(&mut pixels, view, &geometry, colors, self.layers);
            let texture = Texture2D::from_rgba8(pixels.width() as u16, pixels.height() as u16, pixels.as_raw());
            texture.set_filter(FilterMode::Nearest);
            self.texture = Some(texture);
        }
        if let Some(texture) = &self.texture {
            draw_texture(texture, bounds.x, bounds.y, WHITE);
        }
        if let Some(layout) = layout {
            let rect = geometry.outline(viewport, layout);
            let clip = Rect::new(area.x, area.y, area.width, area.height);
            for side in [
                Rect::new(rect.x, rect.y, rect.w, 1.0),
                Rect::new(rect.x, rect.bottom() - 1.0, rect.w, 1.0),
                Rect::new(rect.x, rect.y, 1.0, rect.h),
                Rect::new(rect.right() - 1.0, rect.y, 1.0, rect.h),
            ] {
                if let Some(line) = side.intersect(clip) {
                    draw_rectangle(line.x, line.y, line.w, line.h, WHITE);
                }
            }
        }
    }
}

// Status and ownership overlays use the final minimap's coordinates.
fn paint(image: &mut RgbaImage, rect: Rect, mut sample: impl FnMut(u32, u32) -> Rgba<u8>) {
    let first = |p: f32| (p - 0.5).ceil().max(0.0) as u32;
    for y in first(rect.y)..first(rect.bottom()).min(image.height()) {
        let sy = (((y as f32 + 0.5 - rect.y) / rect.h) * SAMPLE as f32) as u32;
        for x in first(rect.x)..first(rect.right()).min(image.width()) {
            let sx = (((x as f32 + 0.5 - rect.x) / rect.w) * SAMPLE as f32) as u32;
            image.get_pixel_mut(x, y).blend(&sample(sx.min(SAMPLE - 1), sy.min(SAMPLE - 1)));
        }
    }
}

fn raster(view: &Observation, geometry: &Geometry, symbols: &HashMap<String, Symbol>,
          colors: Option<&TeamColors>, reachable: &HashSet<Hex>, layers: MiniLayers) -> RgbaImage {
    let mut image = raster_terrain(view, geometry, symbols, colors, reachable, layers);
    paint_units(&mut image, view, geometry, colors, layers);
    image
}

fn marker(view: &Observation, colors: Option<&TeamColors>, layers: MiniLayers, side: usize, own_color: Rgba<u8>) -> Rgba<u8> {
    if layers.enabled(MiniLayer::TeamColors) { Rgba(<[u8; 4]>::from(side_color(colors, side))) }
    else if side == view.side { own_color }
    else if view.allied_sides.contains(&side) { Rgba([0,164,255,255]) }
    else { UNKNOWN }
}

fn raster_terrain(view: &Observation, geometry: &Geometry, symbols: &HashMap<String, Symbol>,
                  colors: Option<&TeamColors>, reachable: &HashSet<Hex>, layers: MiniLayers) -> RgbaImage {
    let background = if layers.enabled(MiniLayer::Terrain) { UNKNOWN } else { Rgba([0;4]) };
    let mut image = RgbaImage::from_pixel(geometry.bounds.w as u32, geometry.bounds.h as u32, background);
    for tile in &view.tiles {
        let rect = geometry.tile(tile.position);
        let Some(symbol) = symbols.get(&tile.terrain) else { continue };
        if layers.enabled(MiniLayer::Terrain) {
            let highlighted = reachable.contains(&tile.position);
            let shade = |mut pixel: Rgba<u8>| {
                for channel in &mut pixel.0[..3] {
                    if highlighted { *channel = channel.saturating_add(50); }
                    else if !tile.visible { *channel = channel.saturating_sub(50); }
                }
                pixel
            };
            if !layers.enabled(MiniLayer::TerrainImages) {
                let [r,g,b] = symbol.flat;
                let color = shade(Rgba([r,g,b,255]));
                paint(&mut image, Rect {w:rect.w * 0.75, ..rect}, |_,_| color);
            } else if let Some(pixels) = &symbol.pixels {
                paint_symbol(&mut image, geometry, tile.position, |x,y| shade(*pixels.get_pixel(x,y)));
            }
        }
        if symbol.village && layers.enabled(MiniLayer::Villages) {
            let color = tile.village_owner.filter(|side| tile.visible && *side > 0)
                .map(|side| marker(view, colors, layers, side, Rgba([140,255,0,255])))
                .unwrap_or(NEUTRAL_VILLAGE);
            paint(&mut image, Rect { w: rect.w * 0.75, ..rect }, |_, _| color);
        }
    }
    image
}

fn paint_units(image: &mut RgbaImage, view: &Observation, geometry: &Geometry,
               colors: Option<&TeamColors>, layers: MiniLayers) {
    if !layers.enabled(MiniLayer::Units) { return; }
    for unit in &view.units {
        let tile = geometry.tile(unit.position);
        // The native unit overlay is one full hex wide, offset from the terrain thumbnail.
        let rect = Rect::new((tile.x + tile.w * 0.25).round(), (tile.y + tile.h * 0.25).round(),
                             tile.w.round().max(1.0), tile.h.round().max(1.0));
        let orb = if unit.full_action_points == Some(true) { Rgba([140,255,0,255]) }
            else if unit.moves_left.unwrap_or(0) > 0 { Rgba([255,198,0,255]) }
            else { Rgba([255,0,0,255]) };
        let color = marker(view, colors, layers, unit.side, orb);
        paint(image, rect, |_, _| color);
    }
}

// Project each destination pixel into the unscaled composite before choosing a
// symbol pixel. Sampling each tile separately at its rounded output width shifts
// the texture on nearly every hex of a map that does not scale by an integer.
fn paint_symbol(image: &mut RgbaImage, geometry: &Geometry, hex: Hex,
                mut sample: impl FnMut(u32, u32) -> Rgba<u8>) {
    let span = SAMPLE as i32;
    let left = hex.x * (span * 3 / 4) - span / 4;
    let top = hex.y * span + (hex.x.rem_euclid(2) - 1) * (span / 2);
    let step = geometry.sample_step;
    let rows = ((top as f32 / step.y).floor() as u32)
        ..(((top + span) as f32 / step.y).ceil() as u32).min(image.height());
    let cols = ((left as f32 / step.x).floor() as u32)
        ..(((left + span) as f32 / step.x).ceil() as u32).min(image.width());
    for y in rows {
        let sy = (y as f32 * step.y) as i32 - top;
        if !(0..span).contains(&sy) { continue; }
        for x in cols.clone() {
            let sx = (x as f32 * step.x) as i32 - left;
            if (0..span).contains(&sx) {
                image.get_pixel_mut(x, y).blend(&sample(sx as u32, sy as u32));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skirmish_previews_reuse_terrain_pixels_after_switching_maps() {
        let dir = std::env::temp_dir().join(format!("rustnoth-skirmish-minimap-{}", std::process::id()));
        let terrain = dir.join("data/core/images/terrain");
        std::fs::create_dir_all(&terrain).unwrap();
        let grass = terrain.join("grass.png");
        let grass_pixel = Rgba([80, 130, 20, 255]);
        RgbaImage::from_pixel(72, 72, grass_pixel).save(&grass).unwrap();
        let assets = AssetCatalog::for_setup(
            &dir,
            b"[terrain_type]\nstring=Gg\nsymbol_image=grass\n[/terrain_type]",
        ).unwrap();
        let first = ScenarioMap::parse("Gg,Gg,Gg\nGg,Gg,Gg\nGg,Gg,Gg").unwrap();
        let second = ScenarioMap::parse("Gg,Gg,Gg,Gg\nGg,Gg,Gg,Gg\nGg,Gg,Gg,Gg").unwrap();
        let mut reused = Art::new(&dir);
        reused.skirmish_pixels(&assets, &first, 64.0).unwrap();
        let reference = Art::new(&dir).skirmish_pixels(&assets, &second, 80.0).unwrap();
        assert!(reference.pixels().any(|pixel| *pixel == grass_pixel));

        std::fs::remove_file(&grass).unwrap();
        assert_eq!(reused.skirmish_pixels(&assets, &second, 80.0), Some(reference.clone()));
        assert_ne!(Art::new(&dir).skirmish_pixels(&assets, &second, 80.0), Some(reference));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn editor_camera_extends_into_the_minimap_letterbox_at_zero_scroll() {
        let area = Viewport { x: 1032.0, y: 11.0, width: 160.0, height: 212.0 };
        let geo = Geometry::new(44, 33, area).unwrap();
        assert_eq!(geo.bounds, Rect::new(1032., 37., 160., 160.));
        let map = Viewport { x: 0., y: 68., width: 1019., height: 752. };
        let layout = BoardLayout::battlefield(44, 33, map).unwrap()
            .native_scale(Some(Hex { x: 0, y: 0 }));
        assert_eq!(geo.outline(map, &layout), Rect::new(1030., 33., 70., 52.));

        let mut image = RgbaImage::new(160, 160);
        paint_symbol(&mut image, &geo, Hex { x: 0, y: 0 },
                     |x, y| Rgba([x as u8, y as u8, 0, 255]));
        assert_eq!(*image.get_pixel(0, 0), Rgba([6, 12, 0, 255]));
        assert_eq!(*image.get_pixel(1, 1), Rgba([10, 16, 0, 255]));
    }

    #[test]
    fn ordered_minimap_drag_keeps_its_press_origin_and_last_valid_position() {
        use MouseButton::{Left, Middle};
        use PointerEvent::{Move, Press, Release};

        let area = Viewport { x: 1040., y: 10., width: 160., height: 210. };
        let geometry = Geometry::new(48, 36, area).unwrap();
        let first = [1060., 60.];
        let second = [1150., 130.];
        let board = [980., 130.];
        let zoom_control = [1045., 230.];
        let expected = [first, second].map(|point| geometry.world_point(Vec2::from_array(point)).unwrap());

        for button in [Left, Middle] {
            let mut drag = Drag::default();
            let mut centers = Vec::new();
            let mut handle = |event| {
                drag.pointer_event(event, |point| geometry.world_point(point), |world| centers.push(world))
            };

            assert!(!handle(Press(button, board)));
            assert!(!handle(Move(first)));
            assert!(!handle(Release(button, first)));
            assert!(!handle(Press(button, zoom_control)));
            assert!(!handle(Move(first)));
            assert!(!handle(Release(button, first)));

            assert!(handle(Press(button, first)));
            assert!(handle(Move(second)));
            assert!(handle(Move(board)));
            assert!(!handle(Move(first)));
            assert!(!handle(Release(button, first)));
            assert_eq!(centers, expected);
        }
    }

    #[test]
    fn minimap_drag_tracks_new_positions_until_the_initiating_button_releases_or_exits() {
        use MouseButton::{Left, Middle};

        let area = Viewport { x: 1040., y: 10., width: 160., height: 210. };
        let geometry = Geometry::new(48, 36, area).unwrap();
        let first = vec2(1060., 60.);
        let second = vec2(1150., 130.);
        let letterbox = vec2(1060., 20.);
        let first_world = geometry.world_point(first).unwrap();
        let second_world = geometry.world_point(second).unwrap();
        let mut drag = Drag::default();
        let mut update = |pointer, pressed, held| {
            drag.update(pressed, |button| Some(button) == held, || geometry.world_point(pointer))
        };

        assert_eq!(update(first, None, Some(Middle)), None);
        assert_eq!(update(first, Some(Middle), Some(Middle)), Some(first_world));
        assert_eq!(update(second, None, Some(Middle)), Some(second_world));
        assert_eq!(update(second, None, Some(Middle)), None);
        assert_eq!(update(first, None, Some(Left)), None);
        assert_eq!(update(first, None, Some(Middle)), None);

        assert_eq!(update(second, Some(Left), Some(Left)), Some(second_world));
        assert_eq!(update(letterbox, None, Some(Left)), None);
        assert_eq!(update(first, None, Some(Left)), None);
        assert_eq!(update(first, Some(Left), None), Some(first_world));
    }

    #[test]
    fn observed_symbols_darkened_by_fog_and_neutral_villages_survive_scaling() {
        let area = Viewport { x: 8.0, y: 4.0, width: 90.0, height: 60.0 };
        let geometry = Geometry::new(4, 2, area).unwrap();
        let grass = RgbaImage::from_pixel(SAMPLE, SAMPLE, Rgba([70, 100, 20, 255]));
        let symbols = HashMap::from([
            ("Gg".into(), Symbol { id: 0, pixels: Some(grass.clone()), village: false, flat:[115,151,63] }),
            ("Gg^Vh".into(), Symbol { id: 1, pixels: Some(grass), village: true, flat:[115,151,63] }),
        ]);
        let tile = |x, terrain: &str, visible, owner| ObservedTile {
            position: Hex { x, y: 0 }, terrain: terrain.into(), visible, village_owner: owner, time_color: [0; 3],
        };
        let view = Observation { width: 4, height: 2, side: 1, gold: None, allied_sides: vec![1],
            units: vec![], tiles: vec![tile(0,"Gg",true,None), tile(1,"Gg",false,None),
                                        tile(2,"Gg^Vh",false,Some(1))] };
        let image = raster(&view, &geometry, &symbols, None, &HashSet::new(), MiniLayers::default());
        assert_eq!(*image.get_pixel(3, 4), Rgba([70, 100, 20, 255]));
        assert_eq!(*image.get_pixel(27, 16), Rgba([20, 50, 0, 255]));
        assert_eq!(*image.get_pixel(42, 5), NEUTRAL_VILLAGE);
        assert_eq!(*image.get_pixel(82, 40), UNKNOWN);
        let dir = std::env::temp_dir().join(format!("rustnoth-minimap-{}", std::process::id()));
        let mut layers = MiniLayers::default();
        layers.toggle(MiniLayer::TerrainImages, &dir).unwrap();
        let draw = |options| raster(&view, &geometry, &symbols, None, &HashSet::new(), options);
        assert_eq!(*draw(layers).get_pixel(3, 4), Rgba([115,151,63,255]));
        layers.toggle(MiniLayer::Terrain, &dir).unwrap();
        assert_eq!(*draw(layers).get_pixel(3, 4), Rgba([0;4]));
        assert_eq!(*draw(layers).get_pixel(42, 5), NEUTRAL_VILLAGE);
        layers.toggle(MiniLayer::Villages, &dir).unwrap();
        assert_eq!(*draw(layers).get_pixel(42, 5), Rgba([0;4]));
        std::fs::remove_dir_all(dir).unwrap();
        assert_eq!(geometry.world_point(vec2(53.0, 34.0)), Some([1.75, 1.5]));
        assert_eq!(geometry.world_point(vec2(5.0, 4.0)), None);
    }

    #[test]
    fn moving_a_unit_reuses_the_terrain_raster_until_its_inputs_change() {
        use engine_lite::observation::ObservedUnit;
        use std::cell::Cell;

        let area = Viewport { x: 0., y: 0., width: 96., height: 96. };
        let geometry = Geometry::new(4, 3, area).unwrap();
        let position = Hex { x: 1, y: 1 };
        let tile = ObservedTile {
            position, terrain: "Gg^Vh".into(), visible: true,
            village_owner: Some(1), time_color: [0; 3],
        };
        let unit = ObservedUnit {
            facing: None, position, side: 1, type_id: "Fighter".into(),
            can_recruit: false, emits_zoc: true, hitpoints: 24, max_hitpoints: 24,
            full_action_points: Some(true), experience: 0, max_experience: 30,
            poisoned: false, slowed: false, petrified: false, moves_left: Some(5),
            attacks_left: Some(1),
        };
        let mut view = Observation {
            side: 1, allied_sides: vec![1], gold: None, width: 4, height: 3,
            tiles: vec![tile], units: vec![unit],
        };
        let symbols = HashMap::from([("Gg^Vh".into(), Symbol {
            id: 0,
            pixels: Some(RgbaImage::from_pixel(SAMPLE, SAMPLE, Rgba([70, 100, 20, 127]))),
            village: true,
            flat: [115, 151, 63],
        })]);
        let layers = MiniLayers::default();
        let draws = Cell::new(0);
        let compose = |terrain: &mut TerrainLayer, view: &Observation, reachable: HashSet<Hex>| {
            terrain.load(view, &symbols);
            let mut pixels = terrain.pixels(geometry.bounds.size(), reachable.clone(), |reachable| {
                draws.set(draws.get() + 1);
                raster_terrain(view, &geometry, &symbols, None, reachable, layers)
            });
            paint_units(&mut pixels, view, &geometry, None, layers);
            assert_eq!(pixels, raster(view, &geometry, &symbols, None, &reachable, layers));
            pixels
        };
        let mut terrain = TerrainLayer::default();
        let before = compose(&mut terrain, &view, HashSet::new());
        view.units[0].position = Hex { x: 2, y: 1 };
        let after = compose(&mut terrain, &view, HashSet::new());
        assert_ne!(before, after);
        assert_eq!(draws.get(), 1);

        view.tiles[0].visible = false;
        compose(&mut terrain, &view, HashSet::new());
        view.tiles[0].visible = true;
        compose(&mut terrain, &view, HashSet::new());
        view.tiles[0].village_owner = Some(2);
        compose(&mut terrain, &view, HashSet::new());
        compose(&mut terrain, &view, HashSet::from([position]));
        assert_eq!(draws.get(), 5);
    }
}
