use engine_lite::hex::Hex;
use rustnoth_gui::layout::{BoardLayout, Viewport};

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
    drag_position: Option<[f32; 2]>,
}

impl BoardCamera {
    pub fn update(&mut self, viewport: Viewport, view: &engine_lite::observation::Observation, reset: bool) {
        let opening = self.layout.is_none();
        self.fit(viewport, view.width, view.height, reset);
        if opening && !reset && let Some(layout) = self.layout.as_mut() {
            let leader = view.units.iter().find(|unit| unit.side == view.side && unit.can_recruit);
            *layout = layout.native_scale(leader.map(|unit| unit.position));
        }
    }

    fn fit(&mut self, viewport: Viewport, width: i32, height: i32, reset: bool) {
        let fit = (viewport, width, height);
        if !reset && self.last_fit == Some(fit) {
            return;
        }
        let same_map = self.last_fit.is_some_and(|(_, w, h)| (w, h) == (width, height));
        let layout = match self.layout.as_ref() {
            Some(current) if same_map && !reset => current.resized(viewport),
            _ => BoardLayout::battlefield(width, height, viewport),
        };
        // A minimized window has no drawable map area. Keep its previous scroll position.
        let Some(layout) = layout else { return };
        self.last_fit = Some(fit);
        self.drag_position = None;
        self.layout = Some(layout);
    }

    pub fn scroll(&mut self, point: [f32; 2], delta: f32) {
        if delta != 0.0 && self.contains(point)
            && let Some(layout) = self.layout.as_mut()
        {
            *layout = layout.zoomed_at(point, ZOOM_PER_STEP.powf(delta / WHEEL_STEP));
        }
    }

    pub fn pan(&mut self, delta: [f32; 2]) {
        if delta != [0.0, 0.0]
            && let Some(layout) = self.layout.as_mut()
        {
            *layout = layout.panned(delta);
        }
    }

    pub fn checkpoint(&self) -> Option<[f32; 3]> {
        let (viewport, _, _) = self.last_fit?;
        let layout = self.layout.as_ref()?;
        let [left, top] = layout.tile_origin(Hex {x:0,y:0});
        let size = layout.tile_size();
        Some([(viewport.x + viewport.width/2.0-left)/size,
              (viewport.y + viewport.height/2.0-top)/size, size])
    }

    pub fn restore(&mut self, [x,y,size]: [f32;3]) {
        if let Some(layout)=self.layout.as_mut() {
            *layout = layout.zoomed_at([0.0,0.0],size/layout.tile_size());
        }
        self.center_world([x,y]);
    }

    pub fn show_movement_hex(&mut self, at: Hex) {
        if let (Some(layout), Some((area, _, _))) = (self.layout.as_ref(), self.last_fit) {
            let [x, y] = layout.tile_origin(at);
            let size = layout.tile_size();
            if x < area.x || y < area.y || x + size > area.x + area.width || y + size > area.y + area.height {
                self.center_world([at.x as f32 * 0.75 + 0.5, at.y as f32 + at.x.rem_euclid(2) as f32 * 0.5 + 0.5]);
            }
        }
    }

    /// Center the viewport on a point in map space, expressed in tile widths and heights.
    pub fn center_world(&mut self, [x, y]: [f32; 2]) {
        if let (Some((viewport, _, _)), Some(layout)) = (self.last_fit, self.layout.as_mut()) {
            let [left, top] = layout.tile_origin(Hex { x: 0, y: 0 });
            let size = layout.tile_size();
            *layout = layout.panned([
                viewport.x + viewport.width / 2.0 - left - size * x,
                viewport.y + viewport.height / 2.0 - top - size * y,
            ]);
        }
    }

    pub fn drag(&mut self, point: [f32; 2], pressed: bool, held: bool) {
        if !held {
            self.drag_position = None;
        } else if pressed {
            self.drag_position = self.contains(point).then_some(point);
        } else if let Some([x, y]) = self.drag_position {
            self.pan([point[0] - x, point[1] - y]);
            self.drag_position = Some(point);
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
        self.last_fit.is_some_and(|(viewport, _, _)| viewport.contains([x, y]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_anchored_zoom_across_resize_and_rejects_hud_input() {
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
        camera.pan([-70.0, -50.0]);
        let before = *camera.layout().unwrap();
        let origin = before.tile_origin(anchor_hex);
        camera.fit(Viewport { width: 0.0, ..viewport }, 6, 6, false);
        let resized = Viewport { width: 210.0, y: 77.0, ..viewport };
        camera.fit(resized, 6, 6, false);
        assert_eq!(camera.layout().unwrap().tile_size(), before.tile_size());
        assert_eq!(camera.layout().unwrap().tile_origin(anchor_hex), [origin[0], origin[1] + 9.0]);
        camera.fit(resized, 6, 6, true);
        assert_eq!(camera.layout().unwrap().tile_size(), fit.tile_size());
    }

    #[test]
    fn pans_hit_testing_with_the_board_and_stops_at_the_native_half_hex_border() {
        let viewport = Viewport { x: 20.0, y: 68.0, width: 240.0, height: 200.0 };
        let mut camera = BoardCamera::default();
        let first = Hex { x: 0, y: 0 };
        let last = Hex { x: 5, y: 5 };
        camera.fit(viewport, 6, 6, false);
        let fitted_origin = camera.layout().unwrap().tile_origin(first);
        camera.pan([80.0, -80.0]);
        assert_eq!(camera.layout().unwrap().tile_origin(first), fitted_origin);

        camera.scroll([140.0, 168.0], 12.0 * WHEEL_STEP);
        camera.pan([2_000.0, 2_000.0]);
        assert_eq!(camera.layout().unwrap().tile_origin(first), [74.0, 140.0]);

        camera.drag([19.0, 156.0], true, true);
        camera.drag([88.0, 136.0], false, true);
        assert_eq!(camera.layout().unwrap().tile_origin(first), [74.0, 140.0]);
        camera.drag([108.0, 156.0], true, true);
        camera.drag([88.0, 136.0], false, true);
        assert_eq!(camera.layout().unwrap().tile_origin(first), [54.0, 120.0]);
        assert_eq!(camera.hex_at([88.0, 136.0]), Some(first));
        camera.drag([88.0, 136.0], false, false);
        camera.drag([68.0, 116.0], false, true);
        assert_eq!(camera.layout().unwrap().tile_origin(first), [54.0, 120.0]);

        camera.pan([-2_000.0, -2_000.0]);
        let [x, y] = camera.layout().unwrap().tile_origin(last);
        assert_eq!([x + 144.0, y + 144.0], [206.0, 196.0]);
    }

    #[test]
    fn centering_a_minimap_point_keeps_the_zoom_and_places_its_hex_under_the_camera_center() {
        let viewport = Viewport { x: 0.0, y: 20.0, width: 300.0, height: 240.0 };
        let mut camera = BoardCamera::default();
        camera.fit(viewport, 20, 20, false);
        let center = [150.0, 140.0];
        camera.scroll(center, WHEEL_STEP * 8.0);
        let zoom = camera.layout().unwrap().tile_size();
        camera.center_world([9.5, 7.5]);
        assert_eq!(camera.hex_at(center), Some(Hex { x: 12, y: 7 }));
        assert_eq!(camera.layout().unwrap().tile_size(), zoom);
    }

    #[test]
    fn centers_a_fitting_axis_while_panning_the_overflowing_axis() {
        let viewport = Viewport { x: 0.0, y: 68.0, width: 400.0, height: 240.0 };
        let mut camera = BoardCamera::default();
        let first = Hex { x: 0, y: 0 };
        camera.fit(viewport, 2, 4, false);
        camera.scroll([190.0, 180.0], 12.0 * WHEEL_STEP);
        camera.pan([25.0, 2_000.0]);

        assert_eq!(camera.layout().unwrap().tile_origin(first), [74.0, 140.0]);
        camera.pan([-25.0, -20.0]);
        assert_eq!(camera.layout().unwrap().tile_origin(first), [74.0, 120.0]);
    }
}
