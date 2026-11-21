use macroquad::prelude::*;

pub(super) struct Panels {
    pub tip: Rect,
    pub menu: Rect,
}

// title_screen.cfg gives the horizontal spacers weights 1:3:1, and the
// vertical spacers weights 5:1. GUI2 divides before multiplying each weight.
pub(super) fn place(view: Vec2, tip: Vec2, menu: Vec2, footer_height: f32) -> Panels {
    let horizontal = ((view.x - 34.0 - tip.x - menu.x).max(0.0) / 5.0).floor();
    let height = tip.y.max(menu.y);
    let vertical = ((view.y - footer_height - 20.0 - height).max(0.0) / 6.0).floor();
    let left = 17.0 + horizontal;
    let bottom = 20.0 + 5.0 * vertical + height;
    Panels {
        tip: Rect::new(left, bottom - tip.y, tip.x, tip.y),
        menu: Rect::new(left + tip.x + 3.0 * horizontal, bottom - menu.y, menu.x, menu.y),
    }
}

pub(super) fn label_height(lines: usize) -> f32 {
    // The 17px title tip label occupies 21px, with 22px between baselines.
    (lines.max(1) * 22 - 1) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_panels_match_gui2_placements_and_discard_growth_remainders() {
        let tips = vec2(710.0, label_height(6) + label_height(1) + 106.0);
        let menu = vec2(176.0, 450.0);
        for window in [vec2(1200.0, 820.0), vec2(1204.0, 821.0)] {
            let panels = place(window, tips, menu, 40.0);
            assert_eq!(panels.tip, Rect::new(73.0, 467.0, 710.0, 258.0));
            assert_eq!(panels.menu, Rect::new(951.0, 275.0, 176.0, 450.0));
        }
    }
}
