//! Sample the title artwork at the pixel centers of the destination rectangle.
use ::image::RgbaImage;
use macroquad::prelude::*;
use std::cell::RefCell;

pub(super) struct Artwork {
    pixels: RgbaImage,
    cache: RefCell<Option<(u16, u16, Texture2D)>>,
}

impl Artwork {
    pub fn new(pixels: RgbaImage) -> Self {
        Self { pixels, cache: RefCell::new(None) }
    }

    pub fn size(&self) -> Vec2 {
        vec2(self.pixels.width() as f32, self.pixels.height() as f32)
    }

    pub fn draw(&self, bounds: Rect, view: Vec2) {
        let (fw, fh) = macroquad::miniquad::window::screen_size();
        let width = (bounds.w * fw / view.x).round().max(1.) as u16;
        let height = (bounds.h * fh / view.y).round().max(1.) as u16;
        let mut cache = self.cache.borrow_mut();
        if cache.as_ref().is_none_or(|(w, h, _)| (*w, *h) != (width, height)) {
            let bytes = resize(&self.pixels, width, height);
            let texture = Texture2D::from_rgba8(width, height, &bytes);
            texture.set_filter(FilterMode::Nearest);
            *cache = Some((width, height, texture));
        }
        if let Some((_, _, texture)) = cache.as_ref() {
            super::image(bounds, texture);
        }
    }
}

// SDL's D3D linear sampler quantizes fractional texel coordinates to eight bits.
// Keep the weighted sum in full precision until converting the final color to UNORM8.
fn taps(src: u32, dst: u16) -> Vec<(usize, usize, u32)> {
    let denom = 2 * i64::from(dst);
    (0..i64::from(dst)).map(|i| {
        let coord = (2*i+1)*i64::from(src) - i64::from(dst);
        let first = coord.div_euclid(denom);
        let part = coord.rem_euclid(denom)*256;
        let weight = round_even(part, denom) as u32;
        let bound = |p: i64| p.clamp(0, i64::from(src)-1) as usize;
        (bound(first), bound(first+1), weight)
    }).collect()
}

fn round_even(value: i64, divisor: i64) -> i64 {
    let quotient = value / divisor;
    let remainder = 2 * (value % divisor);
    quotient + i64::from(remainder > divisor || remainder == divisor && quotient % 2 != 0)
}

fn resize(pixels: &RgbaImage, width: u16, height: u16) -> Vec<u8> {
    let xs = taps(pixels.width(), width);
    let stride = pixels.width() as usize * 4;
    let src = pixels.as_raw();
    let mut output = Vec::with_capacity(usize::from(width) * usize::from(height) * 4);
    for (y0, y1, v) in taps(pixels.height(), height) {
        let upper = &src[y0 * stride..][..stride];
        let lower = &src[y1 * stride..][..stride];
        for &(x0, x1, u) in &xs {
            for ch in 0..4 {
                let left = x0 * 4 + ch;
                let right = x1 * 4 + ch;
                let top = u32::from(upper[left])*(256-u) + u32::from(upper[right])*u;
                let bottom = u32::from(lower[left])*(256-u) + u32::from(lower[right])*u;
                output.push(round_even(i64::from(top*(256-v) + bottom*v), 65536) as u8);
            }
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_texels_round_to_even_and_edges_replicate() {
        let im = RgbaImage::from_raw(2, 1, vec![0, 1, 128, 255, 1, 2, 255, 255]).unwrap();
        assert_eq!(resize(&im, 3, 1),
            [0, 1, 128, 255, 0, 2, 192, 255, 1, 2, 255, 255]);
        assert_eq!(taps(3, 5)[1], (0, 1, 102));
    }
}
