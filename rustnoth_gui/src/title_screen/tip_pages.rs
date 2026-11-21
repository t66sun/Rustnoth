use super::{Art, dialog, panel_grid, wrap_at};
use macroquad::prelude::*;
use std::cell::{Cell, Ref, RefCell};

enum Label {
    #[cfg(windows)]
    Texture(Texture2D),
    Lines(Vec<String>),
}

impl Label {
    fn height(&self) -> f32 {
        match self {
            #[cfg(windows)]
            Self::Texture(texture) => texture.height(),
            Self::Lines(lines) => panel_grid::label_height(lines.len()),
        }
    }

    fn draw(&self, left: f32, top: f32, width: f32, right: bool, font: Option<&Font>) {
        match self {
            #[cfg(windows)]
            Self::Texture(texture) => draw_texture(texture, left, top, WHITE),
            Self::Lines(lines) => {
                for (i, line) in lines.iter().enumerate() {
                    let x = if right { left + width - measure_text(line, font, 17, 1.).width } else { left };
                    dialog::text(font, line, x, top + 17. + i as f32*22., 17, Color::from_rgba(215, 215, 215, 255));
                }
            }
        }
    }
}

pub(super) struct Page {
    width: u16,
    text: [String; 2],
    labels: [Label; 2],
}

impl Page {
    pub(super) fn height(&self) -> f32 {
        self.labels[0].height() + self.labels[1].height()
    }

    pub(super) fn draw(&self, x: f32, top: f32, bottom: f32, font: Option<&Font>, italic: Option<&Font>) {
        let width = f32::from(self.width);
        self.labels[0].draw(x, top, width, false, font);
        self.labels[1].draw(x, bottom - self.labels[1].height(), width, true, italic);
    }
}

pub(super) struct TipPages {
    pub(super) entries: Vec<(String, String)>,
    measured: Cell<Option<(u16, f32)>>,
    visible: RefCell<Option<Page>>,
}

fn fallback_lines(text: &str, width: u16, font: Option<&Font>) -> Vec<String> {
    let plain = text.replace("<i>", "").replace("</i>", "").replace("<b>", "").replace("</b>", "");
    wrap_at(&plain, font, f32::from(width), 17)
}

impl TipPages {
    pub(super) fn new(entries: Vec<(String, String)>) -> Self {
        Self {
            entries, measured: Default::default(), visible: Default::default(),
        }
    }

    pub(super) fn height(&self, width: u16, art: &Art) -> f32 {
        let font = art.font.as_ref();
        let italic = art.italic.as_ref().or(font);
        if let Some((cached, height)) = self.measured.get() && cached == width {
            return height;
        }
        let height = self.entries.iter().map(|(body, source)| {
            #[cfg(windows)]
            if let Some(r) = &art.text_renderer {
                return f32::from(r.measure(body, width, false, true)) + f32::from(r.measure(source, width, true, true));
            }
            panel_grid::label_height(fallback_lines(body, width, font).len())
                + panel_grid::label_height(fallback_lines(source, width, italic).len())
        }).fold(0.0, f32::max);
        self.measured.set(Some((width, height)));
        height
    }

    pub(super) fn page(&self, body: &str, source: &str, width: u16, art: &Art) -> Ref<'_, Page> {
        let font = art.font.as_ref();
        let italic = art.italic.as_ref().or(font);
        let mut slot = self.visible.borrow_mut();
        if slot.as_ref().is_none_or(|p| p.width != width || p.text[0] != body || p.text[1] != source) {
            let labels = [(body,false,font), (source,true,italic)].map(|(s,right,f)| {
                #[cfg(windows)]
                if let Some(r) = &art.text_renderer {
                    // Errors occupy the tip body with no attribution. Preserve their literal text.
                    let texture = Texture2D::from_image(&r.render(s, width, right, !source.is_empty()));
                    texture.set_filter(FilterMode::Nearest);
                    return Label::Texture(texture);
                }
                #[cfg(not(windows))]
                let _ = right;
                Label::Lines(fallback_lines(s, width, f))
            });
            *slot = Some(Page { width, text: [body.into(), source.into()], labels });
        }
        drop(slot);
        Ref::map(self.visible.borrow(), |p| p.as_ref().unwrap())
    }
}
