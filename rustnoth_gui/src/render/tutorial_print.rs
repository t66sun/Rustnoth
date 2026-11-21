//! The default [print] label is screen-anchored inside the map's outside area.
use super::{BoardArt, Viewport, board_viewport, dialogue};
use macroquad::prelude::*;
use std::path::Path;

#[cfg(windows)]
#[derive(Default)]
pub(super) struct Art {
    pango: std::cell::OnceCell<Option<crate::title_screen::tip_pango::Text>>,
    cached: Option<(String, u16, Texture2D)>,
}

// intf_set_floating_label anchors the top edge at the integer map midpoint.
// floating_label::xpos then subtracts half the rendered texture width.
fn origin(map: Viewport, text_width: f32) -> Vec2 {
    vec2(
        map.x + (map.width / 2.).floor() - (text_width / 2.).floor(),
        map.y + (map.height / 2.).floor(),
    )
}

impl BoardArt {
    pub(crate) fn draw_tutorial_print(&mut self, text: &str, _root: &Path) {
        let map = board_viewport();
        if text.is_empty() || map.width < 1. || map.height < 1. { return; }
        #[cfg(windows)]
        if let Some(pango) = self.tutorial_print.pango.get_or_init(|| {
            crate::title_screen::tip_pango::Text::load(_root, "Lato")
        }) {
            let width = map.width as u16;
            let cache = &mut self.tutorial_print.cached;
            if cache.as_ref().is_none_or(|(prompt, w, _)| prompt != text || *w != width) {
                let texture = Texture2D::from_image(&pango.tutorial_print(text, width));
                texture.set_filter(FilterMode::Nearest);
                *cache = Some((text.to_owned(), width, texture));
            }
            let (_, _, texture) = cache.as_ref().unwrap();
            let at = origin(map, texture.width());
            let height = texture.height().min(map.y + map.height - at.y);
            let source = Some(Rect::new(0., 0., texture.width(), height));
            draw_texture_ex(texture, at.x, at.y, WHITE, DrawTextureParams { source,
                dest_size: Some(vec2(texture.width(), height)), ..Default::default() });
            return;
        }

        let font = self.dialogue_font.as_ref();
        let width = |s: &str| measure_text(s, font, 18, 1.).width;
        let lines = dialogue::wrap(&dialogue::plain_text(text), map.width, width);
        let widest = lines.iter().map(|line| width(line)).fold(0., f32::max);
        let at = origin(map, widest);
        for (index, line) in lines.iter().enumerate() {
            let baseline = at.y + 18. + index as f32 * 22.;
            if baseline + 2. > map.y + map.height { break; }
            for (dx, dy) in [(-1., -1.), (0., -1.), (1., -1.), (-1., 0.), (1., 0.), (-1., 1.), (0., 1.), (1., 1.)] {
                crate::dialog::text(font, line, at.x + dx, baseline + dy, 18, BLACK);
            }
            crate::dialog::text(font, line, at.x, baseline, 18, WHITE);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tutorial_print_starts_at_the_map_midpoint_including_the_top_bar_offset() {
        let map = Viewport {x: 0., y: 27., width: 1018., height: 793.};
        assert_eq!(origin(map, 117.), vec2(451., 423.));
        let resized = Viewport {x: 3., y: 20., width: 641., height: 501.};
        assert_eq!(origin(resized, 100.), vec2(273., 270.));
    }
}
