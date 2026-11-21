//! Fixed floating text used by the installed tutorial's hint_message.lua.
use super::{dialogue, Viewport};
use macroquad::prelude::*;
use std::path::Path;

enum Label {
    #[cfg(windows)]
    Texture(Texture2D),
    Lines(Vec<String>),
}

pub(super) struct Art {
    #[cfg(windows)]
    root: std::path::PathBuf,
    #[cfg(windows)]
    pango: std::cell::OnceCell<Option<crate::title_screen::tip_pango::Text>>,
    cached: Option<(String, u16, Label)>,
}

impl Art {
    pub(super) fn new(_root: &Path) -> Self {
        Self {
            #[cfg(windows)] root: _root.to_owned(),
            #[cfg(windows)] pango: Default::default(),
            cached: None,
        }
    }

    fn label(&self, markup: &str, width: u16, font: Option<&Font>) -> Label {
        #[cfg(windows)]
        if let Some(pango) = self.pango.get_or_init(|| crate::title_screen::tip_pango::Text::load(&self.root, "Lato")) {
            let texture = Texture2D::from_image(&pango.tutorial_hint(markup, width));
            texture.set_filter(FilterMode::Nearest);
            return Label::Texture(texture);
        }
        Label::Lines(dialogue::wrap(&dialogue::plain_text(markup), f32::from(width),
            |text| measure_text(text, font, 18, 1.).width))
    }

    pub(super) fn draw(&mut self, markup: &str, map: Viewport, font: Option<&Font>) {
        if markup.is_empty() || map.width <= 5. || map.height <= 5. { return; }
        let width = (map.width * 0.4).round().max(1.) as u16;
        if self.cached.as_ref().is_none_or(|(text, w, _)| text != markup || *w != width) {
            self.cached = Some((markup.to_owned(), width, self.label(markup, width, font)));
        }
        let (x, y) = (map.x + 5., map.y + 5.);
        // Native floating labels outline white glyphs on transparent terrain.
        let outline = [(-1.,0.), (1.,0.), (0.,-1.), (0.,1.)];
        match &self.cached.as_ref().unwrap().2 {
            #[cfg(windows)]
            Label::Texture(texture) => {
                let height = texture.height().min(map.height - 5.);
                let source = Some(Rect::new(0., 0., texture.width(), height));
                for (dx, dy) in outline {
                    draw_texture_ex(texture, x + dx, y + dy, BLACK, DrawTextureParams { source, ..Default::default() });
                }
                draw_texture_ex(texture, x, y, WHITE, DrawTextureParams { source, ..Default::default() });
            }
            Label::Lines(lines) => {
                for (index, line) in lines.iter().enumerate() {
                    let baseline = y + 18. + index as f32 * 22.;
                    if baseline + 2. > map.y + map.height { break; }
                    for (dx, dy) in outline {
                        crate::dialog::text(font, line, x + dx, baseline + dy, 18, BLACK);
                    }
                    crate::dialog::text(font, line, x, baseline, 18, WHITE);
                }
            }
        }
    }
}
