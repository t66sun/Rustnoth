//! Cache the shaped GUI2 title and message while the battlefield continues drawing.
use crate::title_screen::tip_pango::Text;
use macroquad::prelude::*;
use std::{cell::OnceCell, path::{Path, PathBuf}};

pub(in crate::render) struct Art {
    root: PathBuf,
    pango: OnceCell<Option<Text>>,
    cached: Option<Entry>,
}

struct Entry {
    caption: String,
    text: String,
    viewport: (u16, u16),
    page: usize,
    labels: Labels,
}

#[derive(Clone)]
pub(super) struct Labels {
    caption: Option<Texture2D>,
    body: Texture2D,
    overflow: bool,
    last_page: bool,
}

fn texture(image: Image) -> Texture2D {
    let result = Texture2D::from_image(&image);
    result.set_filter(FilterMode::Nearest);
    result
}

impl Art {
    pub(in crate::render) fn new(root: &Path) -> Self {
        Self { root: root.to_owned(), pango: OnceCell::new(), cached: None }
    }

    pub(in crate::render) fn renderer(&self) -> Option<&Text> {
        self.pango.get_or_init(|| Text::load(&self.root, "Lato")).as_ref()
    }

    pub(super) fn labels(&mut self, caption: &str, text: &str, width: u16, max_height: u16, page: usize) -> Option<Labels> {
        let renderer = self.renderer()?;
        let viewport = (width, max_height);
        if let Some(entry) = &self.cached && entry.caption == caption && entry.text == text
            && entry.viewport == viewport && entry.page == page {
            return Some(entry.labels.clone());
        }
        let title = (!caption.is_empty()).then(|| texture(renderer.dialogue_caption(caption, width)));
        // GUI2's panel borders are 10 and 13. Both label cells have 5px padding.
        let fixed = 43 + title.as_ref().map_or(0, |t| t.height() as u16);
        let (image, overflow, last_page) = renderer.dialogue_body(text, width, max_height.saturating_sub(fixed).max(1), page);
        let labels = Labels { caption: title, body: texture(image), overflow, last_page };
        self.cached = Some(Entry { caption: caption.into(), text: text.into(), viewport, page, labels: labels.clone() });
        Some(labels)
    }
}

impl Labels {
    pub(super) fn height(&self, max_height: f32) -> f32 {
        if self.overflow { return max_height; }
        let title = self.caption.as_ref().map_or(0., Texture2D::height);
        (43. + title + self.body.height()).max(98.).min(max_height)
    }

    pub(super) fn draw(&self, left: f32, top: f32, height: f32, width: f32, font: &Option<Font>) -> bool {
        let title = self.caption.as_ref().map_or(0., Texture2D::height);
        if let Some(texture) = &self.caption {
            draw_texture(texture, left, top + 15., WHITE);
        }
        let extra = if self.overflow { 0. } else { (height - 43. - title - self.body.height()).max(0.) };
        draw_texture(&self.body, left, top + 25. + title + (extra * 0.5).floor(), WHITE);
        if self.overflow {
            let size = super::FONT_SIZE;
            let w = measure_text("OK", font.as_ref(), size, 1.).width;
            super::label(font, "OK", left + (width - w) * 0.5, top + height - 18., size, super::TITLE);
        }
        self.last_page
    }
}
