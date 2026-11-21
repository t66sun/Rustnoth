use super::plain;
use crate::dialog::text as dialog_text;
use macroquad::prelude::*;
use std::path::Path;
#[cfg(windows)]
use std::{cell::RefCell, collections::HashMap};

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum Caption { Title, Row, Header, Play, Button }

impl Caption {
    fn size(self) -> u16 {
        match self { Self::Title => 22, Self::Row => 17, Self::Header => 15, Self::Play => 20, Self::Button => 16 }
    }
    fn color(self) -> [u8; 3] {
        if self == Self::Row { [215; 3] } else { [186, 172, 125] }
    }
    fn origin(self, r: Rect, size: Vec2) -> Vec2 {
        let x = if matches!(self, Self::Title | Self::Row) { 0. } else { ((r.w - size.x)/2.).floor().max(0.) };
        let y = match self {
            Self::Title => 0.,
            Self::Play | Self::Button => ((r.h - size.y - 2.)/2.).floor().max(0.),
            _ => ((r.h - size.y)/2.).floor().max(0.),
        };
        vec2(r.x + x, r.y + y)
    }
}

pub(super) struct Typography {
    #[cfg(windows)]
    renderer: Option<crate::title_screen::tip_pango::Text>,
    #[cfg(windows)]
    labels: RefCell<HashMap<(u16, [u8; 3]), HashMap<String, Texture2D>>>,
    #[cfg(windows)]
    paragraph: Option<(String, u16, i32, Texture2D)>,
}

impl Typography {
    pub fn new(_root: &Path, _family: &str) -> Self {
        Self {
            #[cfg(windows)]
            renderer: crate::title_screen::tip_pango::Text::load(_root, _family),
            #[cfg(windows)]
            labels: RefCell::default(),
            #[cfg(windows)]
            paragraph: None,
        }
    }

    #[cfg(windows)]
    fn label(&self, text: &str, style: Caption) -> Option<Texture2D> {
        let renderer = self.renderer.as_ref()?;
        let mut cache = self.labels.borrow_mut();
        let group = cache.entry((style.size(), style.color())).or_default();
        if let Some(old) = group.get(text) { return Some(old.clone()); }
        let texture = Texture2D::from_image(&renderer.button(text, style.size(), style.color()));
        texture.set_filter(FilterMode::Nearest);
        group.insert(text.into(), texture.clone());
        Some(texture)
    }

    pub fn width(&self, text: &str, style: Caption, font: Option<&Font>) -> f32 {
        #[cfg(windows)]
        if let Some(label) = self.label(text, style) { return label.width(); }
        measure_text(text, font, style.size(), 1.).width.ceil()
    }

    pub fn draw(&self, text: &str, style: Caption, r: Rect, font: Option<&Font>) {
        #[cfg(windows)]
        if let Some(label) = self.label(text, style) {
            let p = style.origin(r, label.size());
            draw_texture(&label, p.x, p.y, WHITE);
            return;
        }
        let m = measure_text(text, font, style.size(), 1.);
        let p = style.origin(r, vec2(m.width.ceil(), m.height.ceil()));
        let [r, g, b] = style.color();
        dialog_text(font, text, p.x, p.y + m.offset_y, style.size(), Color::from_rgba(r,g,b,255));
    }

    pub fn description(&mut self, raw: &str, width: f32, align: Option<&str>, font: Option<&Font>) -> Paragraph {
        let width = width.floor().max(1.) as u16;
        let alignment = match align { Some("center") => 1, Some("right") => 2, _ => 0 };
        #[cfg(windows)]
        if let Some(renderer) = &self.renderer {
            if let Some((text, w, a, image)) = &self.paragraph
                && text == raw && *w == width && *a == alignment {
                return Paragraph::Texture(image.clone());
            }
            let image = Texture2D::from_image(&renderer.paragraph(raw, width, alignment));
            image.set_filter(FilterMode::Nearest);
            self.paragraph = Some((raw.to_owned(), width, alignment, image.clone()));
            return Paragraph::Texture(image);
        }
        Paragraph::Lines(crate::title_screen::wrap(&plain(raw), font, f32::from(width)), width, alignment)
    }
}

pub(super) enum Paragraph {
    #[cfg(windows)]
    Texture(Texture2D),
    Lines(Vec<String>, u16, i32),
}

impl Paragraph {
    pub fn height(&self) -> f32 {
        match self {
            #[cfg(windows)]
            Self::Texture(image) => image.height(),
            Self::Lines(lines, ..) => lines.len() as f32 * 21.,
        }
    }
    pub fn draw(&self, x: f32, y: f32, font: Option<&Font>) {
        match self {
            #[cfg(windows)]
            Self::Texture(image) => draw_texture(image, x, y, WHITE),
            Self::Lines(lines, width, alignment) => for (i, line) in lines.iter().enumerate() {
                let extra = f32::from(*width) - measure_text(line, font, 17, 1.).width;
                let dx = match alignment { 1 => (extra/2.).floor(), 2 => extra.floor(), _ => 0. };
                dialog_text(font, line, x + dx, y + 17. + i as f32 * 21., 17, Color::from_rgba(215,215,215,255));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn list_row_and_footer_buttons_position_the_native_label_surface() {
        assert_eq!(Caption::Row.origin(Rect::new(185.,128.,200.,82.),vec2(90.,21.)), vec2(185.,158.));
        assert_eq!(Caption::Play.origin(Rect::new(811.,727.,172.,40.),vec2(38.,24.)), vec2(878.,734.));
        assert_eq!((Caption::Play.size(), Caption::Button.size(), Caption::Header.size()), (20,16,15));
    }
}
