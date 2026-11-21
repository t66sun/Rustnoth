use super::{Art, Command, dialog};
use macroquad::prelude::*;

const GOLD: [u8; 3] = [186, 172, 125];

#[cfg(windows)]
pub(super) type Cache = std::cell::RefCell<std::collections::HashMap<Command, Texture2D>>;

fn font_size(cmd: Command) -> u16 {
    if cmd == Command::Language { 13 } else { 16 }
}

// The same text canvas is used in the enabled, focused and pressed states.
fn origin(rect: Rect, cmd: Command, size: Vec2) -> Vec2 {
    let x = if cmd == Command::Language {
        // Right-aligned text in the widget's (30, 0, width - 40, height) column.
        30. + (rect.w - 40. - size.x).max(0.)
    } else {
        ((rect.w - size.x) / 2.).floor().max(0.)
    };
    vec2(rect.x + x, rect.y + ((rect.h - size.y - 2.) / 2.).floor().max(0.))
}

impl Art {
    fn command_text(&self, cmd: Command) -> &str {
        if cmd == Command::Language {
            self.locale.language_name(&self.languages)
        } else {
            self.locale.ui(cmd.label())
        }
    }

    #[cfg(windows)]
    fn command_texture(&self, cmd: Command) -> Option<Texture2D> {
        let renderer = self.text_renderer.as_ref()?;
        let mut cache = self.button_labels.borrow_mut();
        Some(cache.entry(cmd).or_insert_with(|| {
            let pixels = renderer.button(self.command_text(cmd), font_size(cmd), GOLD);
            let texture = Texture2D::from_image(&pixels);
            texture.set_filter(FilterMode::Nearest);
            texture
        }).clone())
    }

    pub(super) fn command_label_width(&self, cmd: Command) -> f32 {
        #[cfg(windows)]
        if let Some(t) = self.command_texture(cmd) { return t.width(); }
        measure_text(self.command_text(cmd), self.font.as_ref(), font_size(cmd), 1.).width.ceil()
    }

    pub(super) fn draw_command_label(&self, rect: Rect, cmd: Command) {
        #[cfg(windows)]
        if let Some(t) = self.command_texture(cmd) {
            let pos = origin(rect, cmd, t.size());
            draw_texture(&t, pos.x, pos.y, WHITE);
            return;
        }
        let text = self.command_text(cmd);
        let font = self.font.as_ref();
        let size = font_size(cmd);
        let metrics = measure_text(text, font, size, 1.);
        let pos = origin(rect, cmd, vec2(metrics.width.ceil(), metrics.height.ceil()));
        let [r, g, b] = GOLD;
        dialog::text(font, text, pos.x, pos.y + metrics.offset_y, size, Color::from_rgba(r, g, b, 255));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_placement_uses_the_integer_gui2_text_canvas() {
        let bounds = Rect::new(981., 305., 116., 30.);
        assert_eq!(origin(bounds, Command::Campaigns, vec2(77., 19.)), vec2(1000., 309.));
        assert_eq!(origin(bounds, Command::Help, vec2(120., 32.)), vec2(981., 305.));
        let lang = Rect::new(1081., 780., 109., 30.);
        assert_eq!(origin(lang, Command::Language, vec2(66., 17.)), vec2(1114., 785.));
        assert_eq!((font_size(Command::Previous), font_size(Command::Language)), (16, 13));
    }
}
