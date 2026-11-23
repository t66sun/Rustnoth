use image::RgbaImage;
use macroquad::prelude::{
    Color, DrawTextureParams, FilterMode, Image, Rect, Texture2D, Vec2, draw_texture_ex, vec2,
};

const PAGE_SIDE: u16 = 2048;
const PADDING: u16 = 1;
const CHANNELS: usize = 4;

#[derive(Default)]
pub(super) struct Atlas {
    pages: Vec<Texture2D>,
    layout: PageLayout,
}

impl Atlas {
    pub(super) fn with_page_side(side: u16) -> Self {
        Self { pages: Vec::new(), layout: PageLayout::new(side) }
    }

    pub(super) fn add(&mut self, pixels: &RgbaImage) -> Option<AtlasSprite> {
        let width = u16::try_from(pixels.width()).ok()?;
        let height = u16::try_from(pixels.height()).ok()?;
        if width == 0 || height == 0 {
            return None;
        }

        let size = vec2(f32::from(width), f32::from(height));
        let Some(slot) = self.layout.place(width, height) else {
            let len = usize::from(width) * usize::from(height) * CHANNELS;
            let texture = linear_texture(width, height, &pixels.as_raw()[..len]);
            return Some(AtlasSprite {
                texture,
                source: None,
                size,
            });
        };

        if slot.page == self.pages.len() {
            let side = self.layout.side;
            let len = usize::from(side).pow(2) * CHANNELS;
            self.pages
                .push(linear_texture(side, side, &vec![0; len]));
        }
        let texture = &self.pages[slot.page];
        let image = pad_edges(pixels, width, height);
        texture.update_part(
            &image,
            i32::from(slot.x),
            i32::from(slot.y),
            i32::from(image.width),
            i32::from(image.height),
        );
        Some(AtlasSprite {
            texture: texture.clone(),
            source: Some(slot.source(width, height)),
            size,
        })
    }

    #[cfg(test)]
    pub(super) fn is_empty(&self) -> bool {
        self.pages.is_empty()
    }
}

#[derive(Clone)]
pub(super) struct AtlasSprite {
    texture: Texture2D,
    source: Option<Rect>,
    size: Vec2,
}

impl AtlasSprite {
    pub(super) fn size(&self) -> Vec2 {
        self.size
    }

    pub(super) fn draw(&self, dst: Rect, tint: Color, flip_x: bool) {
        self.draw_region(dst, tint, self.source, (flip_x, false));
    }

    pub(super) fn draw_part(&self, dst: Rect, tint: Color, mut source: Rect, flip: (bool, bool)) {
        if let Some(sprite) = self.source {
            source.x += sprite.x;
            source.y += sprite.y;
        }
        self.draw_region(dst, tint, Some(source), flip);
    }

    fn draw_region(&self, dst: Rect, tint: Color, source: Option<Rect>, (flip_x, flip_y): (bool, bool)) {
        draw_texture_ex(
            &self.texture,
            dst.x,
            dst.y,
            tint,
            DrawTextureParams {
                dest_size: Some(vec2(dst.w, dst.h)),
                source,
                flip_x,
                flip_y,
                ..Default::default()
            },
        );
    }
}

fn linear_texture(width: u16, height: u16, rgba: &[u8]) -> Texture2D {
    let texture = Texture2D::from_rgba8(width, height, rgba);
    texture.set_filter(FilterMode::Linear);
    texture
}

fn pad_edges(pixels: &RgbaImage, width: u16, height: u16) -> Image {
    let padded_width = width + 2 * PADDING;
    let padded_height = height + 2 * PADDING;
    let mut bytes =
        Vec::with_capacity(usize::from(padded_width) * usize::from(padded_height) * CHANNELS);
    let row_len = usize::from(width) * CHANNELS;
    let raw = &pixels.as_raw()[..row_len * usize::from(height)];
    let rows = std::iter::once(&raw[..row_len])
        .chain(raw.chunks_exact(row_len))
        .chain(std::iter::once(&raw[raw.len() - row_len..]));
    // Copy complete RGBA pixels so interpolation at the sprite boundary matches its own texture.
    for row in rows {
        bytes.extend_from_slice(&row[..CHANNELS]);
        bytes.extend_from_slice(row);
        bytes.extend_from_slice(&row[row_len - CHANNELS..]);
    }
    Image {
        bytes,
        width: padded_width,
        height: padded_height,
    }
}

struct PageLayout {
    side: u16,
    page: usize,
    x: u16,
    y: u16,
    row_height: u16,
}

impl Default for PageLayout {
    fn default() -> Self {
        Self::new(PAGE_SIDE)
    }
}

impl PageLayout {
    fn new(side: u16) -> Self {
        Self { side, page: 0, x: 0, y: 0, row_height: 0 }
    }

    fn place(&mut self, width: u16, height: u16) -> Option<Slot> {
        let max_side = self.side - 2 * PADDING;
        if width > max_side || height > max_side {
            return None;
        }
        let width = width + 2 * PADDING;
        let height = height + 2 * PADDING;

        if self.x + width > self.side {
            self.x = 0;
            self.y += self.row_height;
            self.row_height = 0;
        }
        if self.y + height > self.side {
            *self = Self {
                page: self.page + 1,
                ..Self::new(self.side)
            };
        }

        let slot = Slot {
            page: self.page,
            x: self.x,
            y: self.y,
        };
        self.x += width;
        self.row_height = self.row_height.max(height);
        Some(slot)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Slot {
    page: usize,
    x: u16,
    y: u16,
}

impl Slot {
    fn source(self, width: u16, height: u16) -> Rect {
        Rect::new(
            f32::from(self.x + PADDING),
            f32::from(self.y + PADDING),
            f32::from(width),
            f32::from(height),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn padding_extrudes_all_edges_and_corners_with_straight_alpha() {
        let a = [255, 120, 70, 0];
        let b = [55, 190, 40, 255];
        let c = [30, 50, 235, 100];
        let d = [240, 220, 20, 40];
        let pixels = RgbaImage::from_raw(2, 2, [a, b, c, d].concat()).unwrap();
        let padded = pad_edges(&pixels, 2, 2);

        assert_eq!((padded.width, padded.height), (4, 4));
        assert_eq!(
            padded.bytes,
            [a, a, b, b, a, a, b, b, c, c, d, d, c, c, d, d].concat()
        );
    }

    #[test]
    fn terrain_sprites_share_a_page_until_it_fills() {
        let mut layout = PageLayout::default();
        let width = 72;
        let stride = width + 2 * PADDING;
        let row_count = PAGE_SIDE / stride;
        let first = layout.place(width, width).unwrap();
        for row in 0..row_count {
            for column in 0..row_count {
                let slot = if row == 0 && column == 0 {
                    first
                } else {
                    layout.place(width, width).unwrap()
                };
                assert_eq!(
                    slot,
                    Slot {
                        page: 0,
                        x: column * stride,
                        y: row * stride,
                    }
                );
            }
        }
        let spill = layout.place(width, width).unwrap();
        assert_eq!(
            spill,
            Slot {
                page: 1,
                x: 0,
                y: 0,
            }
        );
        assert_eq!(first.page, 0);
        assert_eq!(first.source(width, width), Rect::new(1., 1., 72., 72.));
    }

    #[test]
    fn a_new_row_uses_the_tallest_sprite_to_avoid_overlap() {
        let mut layout = PageLayout::default();
        let left = layout.place(1000, 96).unwrap();
        let right = layout.place(1000, 40).unwrap();
        let below = layout.place(72, 72).unwrap();

        assert_eq!(left.y, right.y);
        assert_eq!(right.x, left.x + 1000 + 2 * PADDING);
        assert_eq!(below.page, left.page);
        assert_eq!(below.x, 0);
        assert_eq!(below.y, left.y + 96 + 2 * PADDING);
    }

    #[test]
    fn oversized_sprite_leaves_the_current_page_available() {
        let mut layout = PageLayout::default();
        let first = layout.place(72, 72).unwrap();
        assert_eq!(layout.place(PAGE_SIDE, 72), None);
        assert_eq!(layout.place(72, u16::MAX), None);
        let second = layout.place(72, 72).unwrap();
        assert_eq!(first.page, second.page);
        assert_eq!(second.x, first.x + 72 + 2 * PADDING);
    }

    #[test]
    fn compact_pages_fit_status_art_without_reserving_terrain_sized_pages() {
        let mut layout = PageLayout::new(256);
        assert_eq!(layout.place(1, 1).unwrap().page, 0);
        for _ in 0..7 {
            assert_eq!(layout.place(72, 72).unwrap().page, 0);
        }
        assert_eq!(layout.place(250, 250).unwrap().page, 1);
        assert!(layout.place(300, 72).is_none());
        assert_eq!(layout.side, 256);
    }

    #[test]
    fn empty_sprite_does_not_allocate_a_page() {
        let mut atlas = Atlas::default();
        assert!(atlas.add(&RgbaImage::new(0, 72)).is_none());
        assert!(atlas.is_empty());
    }
}
