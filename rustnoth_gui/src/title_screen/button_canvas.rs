use macroquad::prelude::*;

fn backing(bounds: Rect) -> Rect {
    Rect::new(bounds.x + 2., bounds.y + 2., bounds.w - 2., bounds.h - 2.)
}

// GUI__BUTTON_NORMAL_FRAME in data/gui/macros/button.cfg. SDL line endpoints
// are inclusive. The four final points round off the gold edges.
fn edges(w: f32, h: f32, accent: Color, disabled: bool) -> [(Rect, Color); 14] {
    let dark = Color::from_rgba(1, 10, 16, 255);
    let gold = if disabled { Color::from_rgba(128, 128, 128, 255) } else { Color::from_rgba(162, 127, 68, 255) };
    let bronze = if disabled { Color::from_rgba(89, 89, 89, 255) } else { Color::from_rgba(114, 79, 46, 255) };
    [
        (Rect::new(0., 1., 1., h - 2.), dark),
        (Rect::new(2., 1., w - 3., 1.), dark),
        (Rect::new(1., h - 1., w - 2., 1.), dark),
        (Rect::new(w - 2., 1., 1., h - 1.), dark),
        (Rect::new(2., 0., w - 3., 1.), gold),
        (Rect::new(w - 1., 1., 1., h - 3.), gold),
        (Rect::new(1., 1., 1., h - 3.), bronze),
        (Rect::new(2., h - 2., w - 3., 1.), bronze),
        (Rect::new(3., 2., w - 5., 1.), accent),
        (Rect::new(w - 3., 2., 1., h - 5.), accent),
        (Rect::new(2., 1., 1., 1.), bronze),
        (Rect::new(2., h - 3., 1., 1.), bronze),
        (Rect::new(w - 2., 1., 1., 1.), gold),
        (Rect::new(w - 2., h - 3., 1., 1.), gold),
    ]
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum State { Enabled, Focused, Pressed, Disabled }

pub(crate) fn draw(bounds: Rect, background: &Texture2D, state: State) {
    frame(bounds, background, state, false);
}

pub(crate) fn icon_button(bounds: Rect, background: &Texture2D, state: State) {
    frame(bounds, background, state, true);
}

fn frame(bounds: Rect, background: &Texture2D, state: State, icon_only: bool) {
    super::image(backing(bounds), background);
    let [r, g, b] = match state {
        State::Enabled => [21, 79, 109],
        State::Focused => [12, 108, 157],
        State::Pressed => [1, 10, 16],
        State::Disabled if icon_only => [1, 10, 16],
        State::Disabled => [60, 60, 60],
    };
    for (edge, color) in edges(bounds.w, bounds.h, Color::from_rgba(r, g, b, 255), state == State::Disabled) {
        draw_rectangle(bounds.x + edge.x, bounds.y + edge.y, edge.w, edge.h, color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn large_button_keeps_its_last_backing_texels_and_rounded_points() {
        let button = Rect::new(981., 305., 116., 30.);
        assert_eq!(backing(button), Rect::new(983., 307., 114., 28.));
        let frame = edges(button.w, button.h, WHITE, false);
        let color_at = |x, y| frame.iter().rev()
            .find(|(r, _)| r.contains(vec2(x, y))).map(|(_, c)| *c);
        assert_eq!(color_at(0.5, 0.5), None);
        assert_eq!(color_at(2.5, 1.5), Some(Color::from_rgba(114, 79, 46, 255)));
        assert_eq!(color_at(114.5, 27.5), Some(Color::from_rgba(162, 127, 68, 255)));
        assert_eq!(color_at(113.5, 26.5), Some(WHITE));
    }
}
