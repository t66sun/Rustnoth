//! Wesnoth's image-coded minimap, using the terrain symbol images from terrain.cfg.
use super::{side_color, AssetCatalog, BoardLayout, Hex, MovementView, Observation, TeamColors, Viewport};
use image::{imageops, Pixel, Rgba, RgbaImage};
use macroquad::prelude::*;
use std::{collections::{HashMap, HashSet}, path::Path};

const SAMPLE: u32 = 24;
const UNKNOWN: Rgba<u8> = Rgba([0, 0, 0, 255]);
const NEUTRAL_VILLAGE: Rgba<u8> = Rgba([30, 30, 30, 255]); // stock "white" range minimum

pub(super) struct Geometry {
    bounds: Rect,
    scale: Vec2,
}

impl Geometry {
    pub(super) fn new(cols: i32, rows: i32, area: Viewport) -> Option<Self> {
        if cols <= 0 || rows <= 0 || area.width < 1.0 || area.height < 1.0 { return None; }
        let extent = vec2(cols as f32 * 0.75, rows as f32);
        let ratio = (area.width / extent.x).min(area.height / extent.y);
        let w = (extent.x * ratio).floor().max(1.0);
        let h = (extent.y * ratio).floor().max(1.0);
        Some(Self {
            bounds: Rect::new(area.x + ((area.width - w) / 2.0).floor(),
                              area.y + ((area.height - h) / 2.0).floor(), w, h),
            scale: vec2(w, h) / extent,
        })
    }

    pub(super) fn world_point(&self, point: Vec2) -> Option<[f32; 2]> {
        self.bounds.contains(point).then(|| {
            let local = (point - self.bounds.point()) / self.scale;
            [local.x + 0.25, local.y + 0.5]
        })
    }

    fn outline(&self, viewport: Viewport, layout: &BoardLayout) -> Option<Rect> {
        let [x, y] = layout.tile_origin(Hex { x: 0, y: 0 });
        let zoom = layout.tile_size();
        let local = vec2((viewport.x - x) / zoom - 0.25, (viewport.y - y) / zoom - 0.5);
        let pos = self.bounds.point() + local * self.scale;
        let size = vec2(viewport.width, viewport.height) * self.scale / zoom;
        Rect::new(pos.x, pos.y, size.x, size.y).intersect(self.bounds)
    }

    fn tile(&self, hex: Hex) -> Rect {
        let x = hex.x as f32 * 0.75 - 0.25;
        let y = hex.y as f32 + (hex.x % 2) as f32 * 0.5 - 0.5;
        Rect::new(x * self.scale.x, y * self.scale.y, self.scale.x, self.scale.y)
    }
}

struct Symbol {
    pixels: Option<RgbaImage>,
    village: bool,
}

pub(super) struct Art {
    mask: Option<RgbaImage>,
    symbols: HashMap<String, Symbol>,
    texture: Option<Texture2D>,
    selected: Option<Hex>,
}

impl Art {
    pub(super) fn new(root: &Path) -> Self {
        let mask = image::open(root.join("data/core/images/terrain/alphamask.png"))
            .ok().map(|image| image.to_rgba8());
        Self { mask, symbols: HashMap::new(), texture: None, selected: None }
    }

    pub(super) fn load(&mut self, view: &Observation, assets: &AssetCatalog) {
        self.texture = None;
        for tile in &view.tiles {
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
                    imageops::resize(&base, SAMPLE, SAMPLE, imageops::FilterType::Nearest)
                });
                Symbol { pixels, village }
            });
        }
    }

    pub(super) fn draw(&mut self, view: &Observation, area: Viewport, viewport: Viewport,
                       layout: Option<&BoardLayout>, colors: Option<&TeamColors>, movement: &MovementView<'_>) {
        let Some(geometry) = Geometry::new(view.width, view.height, area) else { return };
        draw_rectangle(area.x, area.y, area.width, area.height, Color::from_rgba(31, 31, 23, 255));
        let bounds = geometry.bounds;
        if self.selected != movement.selected || !self.texture.as_ref().is_some_and(|t| t.width() == bounds.w && t.height() == bounds.h) {
            self.selected = movement.selected;
            let mut reachable = HashSet::new();
            if let Some(selected) = movement.selected {
                reachable.insert(selected);
                for action in movement.actions {
                    if let engine_lite::fixed_match::Action::Move { from, to } = action && *from == selected {
                        reachable.insert(*to);
                    }
                }
            }
            let pixels = raster(view, &geometry, &self.symbols, colors, &reachable);
            let texture = Texture2D::from_rgba8(pixels.width() as u16, pixels.height() as u16, pixels.as_raw());
            texture.set_filter(FilterMode::Nearest);
            self.texture = Some(texture);
        }
        if let Some(texture) = &self.texture {
            draw_texture(texture, bounds.x, bounds.y, WHITE);
        }
        if let Some(rect) = layout.and_then(|layout| geometry.outline(viewport, layout)) {
            draw_rectangle_lines(rect.x.round(), rect.y.round(), rect.w.round(), rect.h.round(), 1.0, WHITE);
        }
    }
}

// Sample pixel centers into the 24-pixel native symbols, clipping partial edge tiles.
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
          colors: Option<&TeamColors>, reachable: &HashSet<Hex>) -> RgbaImage {
    let mut image = RgbaImage::from_pixel(geometry.bounds.w as u32, geometry.bounds.h as u32, UNKNOWN);
    let marker = |side| Rgba(<[u8; 4]>::from(side_color(colors, side)));
    for tile in &view.tiles {
        let rect = geometry.tile(tile.position);
        let Some(symbol) = symbols.get(&tile.terrain) else { continue };
        if let Some(pixels) = &symbol.pixels {
            let highlighted = reachable.contains(&tile.position);
            paint(&mut image, rect, |x, y| {
                let mut pixel = *pixels.get_pixel(x, y);
                if highlighted {
                    for channel in &mut pixel.0[..3] { *channel = channel.saturating_add(50); }
                } else if !tile.visible {
                    for channel in &mut pixel.0[..3] { *channel = channel.saturating_sub(50); }
                }
                pixel
            });
        }
        if symbol.village {
            let color = tile.village_owner.filter(|side| tile.visible && *side > 0)
                .map(marker).unwrap_or(NEUTRAL_VILLAGE);
            paint(&mut image, Rect { w: rect.w * 0.75, ..rect }, |_, _| color);
        }
    }
    for unit in &view.units {
        let tile = geometry.tile(unit.position);
        // The native unit overlay is one full hex wide, offset from the terrain thumbnail.
        let rect = Rect::new((tile.x + tile.w * 0.25).round(), (tile.y + tile.h * 0.25).round(),
                             tile.w.round().max(1.0), tile.h.round().max(1.0));
        let color = marker(unit.side);
        paint(&mut image, rect, |_, _| color);
    }
    image
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_lite::observation::ObservedTile;

    #[test]
    fn observed_symbols_darkened_by_fog_and_neutral_villages_survive_scaling() {
        let area = Viewport { x: 8.0, y: 4.0, width: 90.0, height: 60.0 };
        let geometry = Geometry::new(4, 2, area).unwrap();
        let grass = RgbaImage::from_pixel(SAMPLE, SAMPLE, Rgba([70, 100, 20, 255]));
        let symbols = HashMap::from([
            ("Gg".into(), Symbol { pixels: Some(grass.clone()), village: false }),
            ("Gg^Vh".into(), Symbol { pixels: Some(grass), village: true }),
        ]);
        let tile = |x, terrain: &str, visible, owner| ObservedTile {
            position: Hex { x, y: 0 }, terrain: terrain.into(), visible, village_owner: owner,
        };
        let view = Observation { width: 4, height: 2, side: 1, gold: None, allied_sides: vec![1],
            units: vec![], tiles: vec![tile(0,"Gg",true,None), tile(1,"Gg",false,None),
                                        tile(2,"Gg^Vh",false,Some(1))] };
        let image = raster(&view, &geometry, &symbols, None, &HashSet::new());
        assert_eq!(*image.get_pixel(3, 4), Rgba([70, 100, 20, 255]));
        assert_eq!(*image.get_pixel(27, 16), Rgba([20, 50, 0, 255]));
        assert_eq!(*image.get_pixel(42, 5), NEUTRAL_VILLAGE);
        assert_eq!(*image.get_pixel(82, 40), UNKNOWN);
        assert_eq!(geometry.world_point(vec2(53.0, 34.0)), Some([1.75, 1.5]));
        assert_eq!(geometry.world_point(vec2(5.0, 4.0)), None);
    }
}
