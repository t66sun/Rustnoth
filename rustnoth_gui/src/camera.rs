use engine_lite::hex::Hex;
use rustnoth_gui::layout::{BoardLayout, Viewport};

const BOARD_PADDING: f32 = 16.0;
const ZOOM_PER_STEP: f32 = 1.25;
// Miniquad forwards raw Windows wheel deltas and unit steps from X11.
#[cfg(windows)]
const WHEEL_STEP: f32 = 120.0;
#[cfg(not(windows))]
const WHEEL_STEP: f32 = 1.0;

#[derive(Default)]
pub struct BoardCamera {
    last_fit: Option<(Viewport, i32, i32)>,
    layout: Option<BoardLayout>,
}

impl BoardCamera {
    pub fn fit(&mut self, viewport: Viewport, width: i32, height: i32, reset: bool) {
        let fit = (viewport, width, height);
        if !reset && self.last_fit == Some(fit) {
            return;
        }
        self.last_fit = Some(fit);
        self.layout = BoardLayout::fit(
            width,
            height,
            Viewport {
                x: viewport.x + BOARD_PADDING,
                y: viewport.y + BOARD_PADDING,
                width: viewport.width - 2.0 * BOARD_PADDING,
                height: viewport.height - 2.0 * BOARD_PADDING,
            },
        );
    }

    pub fn scroll(&mut self, point: [f32; 2], delta: f32) {
        if delta != 0.0 && self.contains(point)
            && let Some(layout) = self.layout.as_mut()
        {
            *layout = layout.zoomed_at(point, ZOOM_PER_STEP.powf(delta / WHEEL_STEP));
        }
    }

    pub fn layout(&self) -> Option<&BoardLayout> {
        self.layout.as_ref()
    }

    pub fn hex_at(&self, point: [f32; 2]) -> Option<Hex> {
        if !self.contains(point) {
            return None;
        }
        self.layout()?.hex_at(point)
    }

    fn contains(&self, [x, y]: [f32; 2]) -> bool {
        self.last_fit.is_some_and(|(viewport, _, _)| {
            (viewport.x..viewport.x + viewport.width).contains(&x)
                && (viewport.y..viewport.y + viewport.height).contains(&y)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_anchored_zoom_until_reset_or_resize_and_rejects_hud_input() {
        let viewport = Viewport { x: 0.0, y: 68.0, width: 200.0, height: 200.0 };
        let mut camera = BoardCamera::default();
        camera.fit(viewport, 6, 6, false);
        let fit = *camera.layout().unwrap();
        let anchor_hex = Hex { x: 0, y: 0 };
        let [x, y] = fit.tile_origin(anchor_hex);
        let half = fit.tile_size() / 2.0;
        let anchor = [x + half, y + half];
        camera.scroll(anchor, 8.0 * WHEEL_STEP);
        let zoomed = *camera.layout().unwrap();
        assert!(zoomed.tile_size() > fit.tile_size());
        assert_eq!(camera.hex_at(anchor), Some(anchor_hex));

        let hidden_hex = Hex { x: 4, y: 0 };
        let [x, y] = zoomed.tile_origin(hidden_hex);
        let half = zoomed.tile_size() / 2.0;
        let over_sidebar = [x + half, y + half];
        assert!(over_sidebar[0] >= viewport.width);
        assert_eq!(zoomed.hex_at(over_sidebar), Some(hidden_hex));
        assert_eq!(camera.hex_at(over_sidebar), None);
        camera.scroll(over_sidebar, -WHEEL_STEP);
        camera.fit(viewport, 6, 6, false);
        assert_eq!(camera.layout().unwrap().tile_size(), zoomed.tile_size());

        camera.fit(viewport, 6, 6, true);
        assert_eq!(camera.layout().unwrap().tile_origin(anchor_hex), fit.tile_origin(anchor_hex));
        camera.scroll(anchor, 8.0 * WHEEL_STEP);
        let resized = Viewport { width: 210.0, ..viewport };
        camera.fit(resized, 6, 6, false);
        assert_eq!(camera.layout().unwrap().tile_size(), fit.tile_size());
        assert!(camera.layout().unwrap().tile_origin(anchor_hex)[0] > fit.tile_origin(anchor_hex)[0]);
    }
}
