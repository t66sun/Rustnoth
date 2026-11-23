use super::*;
use crate::title_screen;

pub(crate) struct Label {
    pub(crate) size: Vec2,
    texture: Option<Texture2D>,
    lines: Vec<String>,
    font_size: u16,
    color: Color,
}

impl Label {
    pub(crate) fn new(text: &str, markup: bool, size: u16, rgb: [u8; 3], limit: f32, font: Option<&Font>,
        #[cfg(windows)] renderer: Option<&title_screen::tip_pango::Text>) -> Self
    {
        let color = Color::from_rgba(rgb[0], rgb[1], rgb[2], 255);
        #[cfg(windows)]
        if let Some(image) = renderer.and_then(|r| r.fitted_label(text, size, limit as u16, markup, rgb)) {
            let texture = Texture2D::from_image(&image);
            texture.set_filter(FilterMode::Nearest);
            return Self { size: texture.size(), texture: Some(texture), lines: Vec::new(), font_size: size, color };
        }
        let plain = if markup { super::dialogue::plain_text(text) } else { text.into() };
        let lines = title_screen::wrap_at(&plain, font, limit, size);
        let width = lines.iter().map(|line| measure_text(line, font, size, 1.).width.ceil()).fold(0., f32::max);
        let height = lines.len() as f32 * f32::from(size + 5);
        Self { size: vec2(width, height), texture: None, lines, font_size: size, color }
    }

    pub(crate) fn draw(&self, position: Vec2, scroll: f32, height: f32, font: Option<&Font>) {
        if let Some(texture) = &self.texture {
            let h = height.min(texture.height() - scroll);
            if h > 0. {
                draw_texture_ex(texture, position.x, position.y, WHITE, DrawTextureParams {
                    source: Some(Rect::new(0., scroll, texture.width(), h)), ..Default::default()
                });
            }
        } else {
            let step = f32::from(self.font_size + 5);
            for (index, line) in self.lines.iter().enumerate().skip((scroll / step).ceil() as usize) {
                let top = index as f32 * step - scroll;
                if top + f32::from(self.font_size) > height { break; }
                crate::dialog::text(font, line, position.x, position.y + top + f32::from(self.font_size),
                    self.font_size, self.color);
            }
        }
    }
}
