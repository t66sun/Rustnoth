use super::*;
#[cfg(windows)]
use crate::title_screen::tip_pango;

pub(super) struct Art {
    backgrounds: [[Texture; 4]; 2],
    #[cfg(windows)] root: std::path::PathBuf,
    #[cfg(windows)] font: std::cell::OnceCell<Option<tip_pango::Text>>,
    #[cfg(windows)] label: Option<(String, bool, Texture2D)>,
}

fn greyscale(pixel: &[u8]) -> u8 {
    ((77 * u32::from(pixel[0]) + 150 * u32::from(pixel[1]) + 29 * u32::from(pixel[2])) / 256) as u8
}

impl Art {
    pub(super) fn load(root: &Path) -> Self {
        let images = root.join("images/buttons");
        let backgrounds = ["large-button", "button_normal/button_H22"].map(|name| {
            let path = |suffix| images.join(format!("{name}{suffix}.png"));
            let disabled = fs::read(path("")).ok().and_then(|bytes| crate::artwork::image(&bytes).ok()).map(|mut image| {
                for pixel in image.bytes.chunks_exact_mut(4) { let shade = greyscale(pixel); pixel[..3].fill(shade); }
                Texture2D::from_image(&image)
            });
            [load_texture(&path(""), None), load_texture(&path("-active"), None),
             load_texture(&path("-active-pressed"), None), disabled]
        });
        Self { backgrounds, #[cfg(windows)] root: root.to_owned(),
            #[cfg(windows)] font: Default::default(), #[cfg(windows)] label: None }
    }

    pub(super) fn draw(&mut self, bounds: Rect, highlighted: bool, enabled: bool, title: &str, fallback: &Option<Font>) {
        let compact = bounds.w <= 80.;
        let active = enabled && highlighted;
        let pressed = active && is_mouse_button_down(MouseButton::Left);
        let index = if !enabled { 3 } else if pressed { 2 } else { usize::from(active) };
        let variants = &self.backgrounds[usize::from(compact)];
        if let Some(texture) = variants[index].as_ref().or(variants[0].as_ref()) {
            draw_texture_ex(texture, bounds.x, bounds.y, WHITE, DrawTextureParams {
                dest_size: Some(bounds.size()), ..Default::default()
            });
        } else {
            fill(bounds, if active { BUTTON_HIGHLIGHTED } else { BUTTON });
            draw_rectangle_lines(bounds.x, bounds.y, bounds.w, bounds.h, 1., BORDER);
        }
        let rgb = if enabled { [186, 172, 125] } else { [136; 3] };
        #[cfg(windows)]
        if let Some(font) = self.font.get_or_init(|| tip_pango::Text::load(&self.root, "Lato")) {
            if self.label.as_ref().is_none_or(|(old, was_enabled, _)| old != title || enabled != *was_enabled) {
                let texture = Texture2D::from_image(&font.button(title, 16, rgb));
                texture.set_filter(FilterMode::Nearest);
                self.label = Some((title.to_owned(), enabled, texture));
            }
            let texture = &self.label.as_ref().unwrap().2;
            let scale = (bounds.w / (texture.width() + 8.)).min(bounds.h / texture.height()).min(1.);
            let size = (texture.size() * scale).floor();
            let xy = bounds.point() + (bounds.size() * 0.5).floor() - (size * 0.5).floor() + Vec2::splat(if pressed { 1. } else { 0. });
            draw_texture_ex(texture, xy.x, xy.y, WHITE, DrawTextureParams { dest_size: Some(size), ..Default::default() });
            return;
        }
        text_in(fallback, title, bounds, 16, Color::from_rgba(rgb[0], rgb[1], rgb[2], 255), true);
    }
}

#[test]
fn disabled_buttons_use_the_stock_weighted_greyscale() {
    assert_eq!(greyscale(&[64, 128, 192, 255]), 116);
    assert_eq!(greyscale(&[255, 255, 255, 128]), 255);
}
