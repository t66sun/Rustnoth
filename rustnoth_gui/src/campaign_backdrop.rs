//! The campaign_menu_bg canvas and its list's translucent black background.
use macroquad::prelude::*;

use super::layout;

pub(super) struct Backdrop {
    pub id: String,
    pub bounds: Rect,
    base: Texture2D,
    list: Texture2D,
}

impl Backdrop {
    pub fn new(id: &str, window: Rect, panel: Rect, tile: Option<&Image>, scene: Option<&Image>) -> Self {
        let pixels = compose(window, panel, tile, scene);
        let base = Texture2D::from_image(&pixels);
        let mut list = pixels;
        for pixel in list.get_image_data_mut() {
            for c in &mut pixel[..3] {
                *c = multiply(*c, 255 - 89);
            }
        }
        let list = Texture2D::from_image(&list);
        for texture in [&base, &list] {
            texture.set_filter(FilterMode::Nearest);
        }
        Self { id: id.to_owned(), bounds: panel, base, list }
    }

    pub fn draw(&self) -> Texture2D {
        super::image(&self.base, self.bounds);
        self.list.clone()
    }
}

pub(super) fn draw_list(texture: &Texture2D, panel: Rect, area: Rect) {
    if let Some(area) = area.intersect(panel) {
        let src = Rect::new(area.x - panel.x, area.y - panel.y, area.w, area.h);
        draw_texture_ex(texture, area.x, area.y, WHITE, DrawTextureParams {
            source: Some(src), ..Default::default()
        });
    }
}

// Wesnoth's color_multiply truncates each contribution to an 8-bit channel.
fn multiply(color: u8, alpha: u8) -> u8 {
    (u16::from(color) * u16::from(alpha) / 255) as u8
}

// SDL's nearest stretch advances from the center of the first destination pixel.
fn sample_axis(source: u16, dest: i32, offset: i32) -> usize {
    let step = (i64::from(source) << 16) / i64::from(dest);
    ((step / 2 + i64::from(offset) * step) >> 16) as usize
}

fn compose(window: Rect, panel: Rect, tile: Option<&Image>, scene: Option<&Image>) -> Image {
    let mut out = Image::gen_image_color(panel.w as u16, panel.h as u16, BLACK);
    let width = usize::from(out.width);
    let tile_x = (panel.x - window.x - 2.) as usize;
    let tile_y = (panel.y - window.y - 2.) as usize;
    let source = scene.map(|scene| {
        let rect = layout::backdrop(Rect::new(0., 0., panel.w, panel.h),
            vec2(f32::from(scene.width), f32::from(scene.height)));
        (scene, rect)
    });
    for (y, row) in out.get_image_data_mut().chunks_exact_mut(width).enumerate() {
        for (x, dest) in row.iter_mut().enumerate() {
            let tile_pixel = tile.map_or([16, 22, 35, 255], |tile| {
                let tw = usize::from(tile.width);
                let th = usize::from(tile.height);
                tile.get_image_data()[((y + tile_y) % th) * tw + ((x + tile_x) % tw)]
            });
            let mut landscape = [0; 4];
            if let Some((scene, rect)) = source {
                let sx = x as i32 - rect.x as i32;
                let sy = y as i32 - rect.y as i32;
                if (0..rect.w as i32).contains(&sx) && (0..rect.h as i32).contains(&sy) {
                    let ix = sample_axis(scene.width, rect.w as i32, sx);
                    let iy = sample_axis(scene.height, rect.h as i32, sy);
                    landscape = scene.get_image_data()[iy * usize::from(scene.width) + ix];
                }
            }
            // panel_campaign_menu.cfg draws the scene at ~O(0.25), then a black 138/255 fill.
            let opacity = multiply(landscape[3], 64);
            for c in 0..3 {
                let mixed = multiply(landscape[c], opacity) + multiply(tile_pixel[c], 255 - opacity);
                dest[c] = multiply(mixed, 255 - 138);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn campaign_canvas_repeats_window_grain_and_truncates_each_color_layer() {
        let window = Rect::new(5., 8., 30., 18.);
        let panel = Rect::new(8., 11., 3., 2.);
        let tile = Image { width: 2, height: 1, bytes: vec![16,22,35,255, 17,20,33,255] };
        let scene = Image { width: 2, height: 2, bytes: vec![168,172,175,255, 67,71,33,255,
            255,255,255,0, 255,255,255,128] };
        let result = compose(window, panel, Some(&tile), Some(&scene));
        assert_eq!(result.get_image_data(), &[
            [24,26,30,255], [24,27,31,255], [12,14,14,255],
            [7,9,15,255], [7,10,16,255], [21,22,27,255],
        ]);
        assert_eq!(multiply(24, 255 - 89), 15);
    }
}
