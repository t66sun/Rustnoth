use crate::dialog;
use macroquad::prelude::*;
use std::path::Path;

const BORDER: f32 = 5.0;

fn fit_size(canvas_width: f32, mut width: impl FnMut(u16) -> f32) -> u16 {
    (2..=20).rev().find(|&size| width(size) * 2.0 <= canvas_width).unwrap_or(1)
}

fn text_origin(canvas: Vec2, map_top: f32, top_row: i32, size: Vec2) -> Vec2 {
    // The native anchor moves by the padded label height, but refers to the text origin.
    vec2((canvas.x * 0.5).floor() - (size.x * 0.5).floor(),
        if top_row > 2 { map_top } else { canvas.y - size.y - 2.0 * BORDER })
}

fn backdrop(pos: Vec2, size: Vec2) {
    draw_rectangle(pos.x - BORDER, pos.y - BORDER, size.x + 2.0 * BORDER,
        size.y + 2.0 * BORDER, Color::from_rgba(0, 0, 0, 0xbb));
}

pub(super) struct HelpLabel {
    #[cfg(windows)]
    renderer: Option<crate::title_screen::tip_pango::Text>,
    #[cfg(windows)]
    cached: Option<(String, u16, Texture2D)>,
}

impl HelpLabel {
    pub fn new(_root: &Path, _family: &str) -> Self {
        Self {
            #[cfg(windows)]
            renderer: crate::title_screen::tip_pango::Text::load(_root, _family),
            #[cfg(windows)]
            cached: None,
        }
    }

    pub fn draw(&mut self, text: &str, font: Option<&Font>, canvas: Vec2, map_top: f32, top_row: i32) {
        #[cfg(windows)]
        if let Some(renderer) = &self.renderer {
            let width = canvas.x as u16;
            if !self.cached.as_ref().is_some_and(|(s, w, _)| s == text && *w == width) {
                let size = fit_size(canvas.x, |size| renderer.text_width(text, size) as f32);
                let image = renderer.button(text, size, [221; 3]);
                let texture = Texture2D::from_image(&image);
                texture.set_filter(FilterMode::Nearest);
                self.cached = Some((text.to_owned(), width, texture));
            }
            if let Some((_, _, texture)) = &self.cached {
                let pos = text_origin(canvas, map_top, top_row, texture.size());
                backdrop(pos, texture.size());
                draw_texture(texture, pos.x, pos.y, WHITE);
                return;
            }
        }
        let width_at = |size| text.lines().map(|s| measure_text(s, font, size, 1.).width.ceil()).fold(0., f32::max);
        let size = fit_size(canvas.x, width_at);
        let line_height = (f32::from(size) * 1.3).ceil();
        let bounds = vec2(width_at(size), text.lines().count() as f32 * line_height);
        let pos = text_origin(canvas, map_top, top_row, bounds);
        backdrop(pos, bounds);
        for (row, line) in text.lines().enumerate() {
            dialog::text(font, line, pos.x, pos.y + f32::from(size) + row as f32 * line_height,
                size, Color::from_rgba(221, 221, 221, 255));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_information_uses_a_half_canvas_and_moves_above_the_southern_map() {
        assert_eq!(fit_size(1200., |size| f32::from(size) * 37.), 16);
        let canvas = vec2(1200., 820.);
        let label = vec2(589., 40.);
        assert_eq!(text_origin(canvas, 68., 2, label), vec2(306., 770.));
        assert_eq!(text_origin(canvas, 68., 3, label), vec2(306., 68.));
    }
}
