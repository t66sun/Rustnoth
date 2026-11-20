//! The title's box_display pre-commit blur, evaluated on the artwork behind both panels.
use macroquad::prelude::*;
use std::cell::RefCell;

#[derive(PartialEq)]
struct Layout {
    regions: [Rect; 2],
    view: Vec2,
    framebuffer: (f32, f32),
}

pub(super) struct Panels {
    layout: Layout,
    textures: [Texture2D; 2],
}

pub(super) fn draw(cache: &RefCell<Option<Panels>>, regions: [Rect; 2], view: Vec2) {
    let layout = Layout { regions, view, framebuffer: macroquad::miniquad::window::screen_size() };
    let mut cache = cache.borrow_mut();
    if cache.as_ref().is_none_or(|old| old.layout != layout) {
        let frame = get_screen_data();
        let textures = regions.map(|region| {
            let pixels = backdrop(&frame, view, region);
            let texture = Texture2D::from_image(&pixels);
            texture.set_filter(FilterMode::Nearest);
            texture
        });
        *cache = Some(Panels { layout, textures });
    }
    if let Some(panels) = cache.as_ref() {
        for (region, texture) in regions.into_iter().zip(&panels.textures) {
            super::image(region, texture);
        }
    }
}

fn backdrop(frame: &Image, view: Vec2, region: Rect) -> Image {
    let width = region.w.round().max(1.) as u16;
    let height = region.h.round().max(1.) as u16;
    let mut pixels = Image::gen_image_color(width, height, BLACK);
    let src = frame.get_image_data();
    for (y, row) in pixels.get_image_data_mut().chunks_exact_mut(width.into()).enumerate() {
        let fy = ((region.y + y as f32 + 0.5) * f32::from(frame.height) / view.y)
            .clamp(0., f32::from(frame.height - 1)) as usize;
        let row_start = (usize::from(frame.height) - fy - 1) * usize::from(frame.width);
        for (x, pixel) in row.iter_mut().enumerate() {
            let fx = ((region.x + x as f32 + 0.5) * f32::from(frame.width) / view.x)
                .clamp(0., f32::from(frame.width - 1)) as usize;
            *pixel = src[row_start + fx];
        }
    }
    // The SDL surface operation truncates the horizontal average before the vertical pass.
    let (w, h) = (usize::from(width), usize::from(height));
    for vertical in [false, true] {
        let prior = pixels.clone();
        let src = prior.get_image_data();
        for (index, dst) in pixels.get_image_data_mut().iter_mut().enumerate() {
            let (pos, length, stride) = if vertical { (index / w, h, w) } else { (index % w, w, 1) };
            let begin = index - if pos > 0 { stride } else { 0 };
            let end = index + if pos + 1 < length { stride } else { 0 };
            let count = ((end - begin) / stride + 1) as u16;
            for channel in 0..3 {
                let sum: u16 = (begin..=end).step_by(stride).map(|i| u16::from(src[i][channel])).sum();
                dst[channel] = (sum / count) as u8;
            }
            dst[3] = 255;
        }
    }
    pixels
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blur_reads_only_the_panel_and_truncates_after_each_pass() {
        // Bottom-up framebuffer. The leftmost pixel of each row is outside the panel.
        let values = [255, 11, 7, 0, 255, 0, 2, 3];
        let frame = Image { width: 4, height: 2, bytes: values.into_iter().flat_map(|n| [n,n,n,99]).collect() };
        let result = backdrop(&frame, vec2(4.,2.), Rect::new(1.,0.,3.,2.));
        assert_eq!(result.get_image_data(), &[[5,5,5,255],[3,3,3,255],[2,2,2,255],
            [5,5,5,255],[3,3,3,255],[2,2,2,255]]);
    }
}
