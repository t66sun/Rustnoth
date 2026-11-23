use image::RgbaImage;
use macroquad::{
    models::Vertex,
    prelude::{Color, DrawMode, FilterMode, Image, Rect, Texture2D, Vec2, get_internal_gl, vec2},
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
                uv: Rect::new(0.0, 0.0, 1.0, 1.0),
                size,
            });
        };

        if slot.page == self.pages.len() {
            let len = usize::from(PAGE_SIDE).pow(2) * CHANNELS;
            self.pages
                .push(linear_texture(PAGE_SIDE, PAGE_SIDE, &vec![0; len]));
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
        let source = slot.source(width, height);
        let page = texture.size();
        Some(AtlasSprite {
            texture: texture.clone(),
            uv: Rect::new(source.x / page.x, source.y / page.y, source.w / page.x, source.h / page.y),
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
    uv: Rect,
    size: Vec2,
}

impl AtlasSprite {
    pub(super) fn size(&self) -> Vec2 {
        self.size
    }

    pub(super) fn draw(&self, dst: Rect, tint: Color, flip_x: bool) {
        let vertices = quad(dst, self.uv, tint, flip_x);
        // The borrowed queue belongs to the render thread. Geometry is appended
        // to the active camera/material without flushing preceding primitives.
        let gl = unsafe { get_internal_gl() }.quad_gl;
        gl.texture(Some(&self.texture));
        gl.draw_mode(DrawMode::Triangles);
        gl.geometry(&vertices, &[0, 1, 2, 0, 2, 3]);
    }
}

fn quad(dst: Rect, uv: Rect, tint: Color, flip_x: bool) -> [Vertex; 4] {
    let (first_u, last_u) = if flip_x { (uv.right(), uv.x) } else { (uv.x, uv.right()) };
    [
        Vertex::new(dst.x, dst.y, 0.0, first_u, uv.y, tint),
        Vertex::new(dst.right(), dst.y, 0.0, last_u, uv.y, tint),
        Vertex::new(dst.right(), dst.bottom(), 0.0, last_u, uv.bottom(), tint),
        Vertex::new(dst.x, dst.bottom(), 0.0, first_u, uv.bottom(), tint),
    ]
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

#[derive(Default)]
struct PageLayout {
    page: usize,
    x: u16,
    y: u16,
    row_height: u16,
}

impl PageLayout {
    fn place(&mut self, width: u16, height: u16) -> Option<Slot> {
        let max_side = PAGE_SIDE - 2 * PADDING;
        if width > max_side || height > max_side {
            return None;
        }
        let width = width + 2 * PADDING;
        let height = height + 2 * PADDING;

        if self.x + width > PAGE_SIDE {
            self.x = 0;
            self.y += self.row_height;
            self.row_height = 0;
        }
        if self.y + height > PAGE_SIDE {
            *self = Self {
                page: self.page + 1,
                ..Self::default()
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
    fn packed_quads_flip_only_the_horizontal_uv_and_keep_tint_and_corners() {
        let dst = Rect::new(10.5, 5.25, 12.0, 10.0);
        let uv = Rect::new(0.25, 0.5, 0.125, 0.25);
        let tint = Color::from_rgba(5, 60, 200, 144);
        let plain = quad(dst, uv, tint, false);
        let flipped = quad(dst, uv, tint, true);
        assert_eq!(plain.map(|v| v.position.to_array()), [
            [10.5, 5.25, 0.0], [22.5, 5.25, 0.0], [22.5, 15.25, 0.0], [10.5, 15.25, 0.0],
        ]);
        assert_eq!(flipped.map(|v| v.position), plain.map(|v| v.position));
        assert_eq!(plain.map(|v| v.uv.to_array()), [[0.25, 0.5], [0.375, 0.5], [0.375, 0.75], [0.25, 0.75]]);
        assert_eq!(flipped.map(|v| v.uv.to_array()), [[0.375, 0.5], [0.25, 0.5], [0.25, 0.75], [0.375, 0.75]]);
        assert_eq!(flipped.map(|v| v.color), [Vertex::new(0., 0., 0., 0., 0., tint).color; 4]);
    }

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
    fn empty_sprite_does_not_allocate_a_page() {
        let mut atlas = Atlas::default();
        assert!(atlas.add(&RgbaImage::new(0, 72)).is_none());
        assert!(atlas.is_empty());
    }
}
