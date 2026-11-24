use std::cell::Cell;

use macroquad::experimental::coroutines::start_coroutine;
use macroquad::input::{is_mouse_button_down, utils};
use macroquad::miniquad::{self, EventHandler, MouseButton, TouchPhase};
use macroquad::window::next_frame;

use super::BoardCamera;

#[derive(Clone, Copy)]
pub(crate) enum PointerEvent {
    Press(MouseButton, [f32; 2]),
    Move([f32; 2]),
    Release(MouseButton, [f32; 2]),
    Cancel,
}

#[cfg(windows)]
mod windows_capture;
#[cfg(windows)]
pub(super) use windows_capture::Capture;

thread_local! {
    static SUBSCRIPTION: Cell<Option<usize>> = const { Cell::new(None) };
    static FRAME: Cell<usize> = const { Cell::new(0) };
}

pub(crate) fn start_drag_input() {
    #[cfg(windows)]
    windows_capture::attach();
    let subscription = utils::register_input_subscriber();
    SUBSCRIPTION.set(Some(subscription));
    start_coroutine(async move {
        loop {
            // Macroquad polls coroutines after the scene future. Discard events
            // on frames where the active scene has no map camera.
            utils::repeat_all_miniquad_input(&mut DragReceiver::new(None, 1.0), subscription);
            #[cfg(windows)]
            windows_capture::release_unused(FRAME.get());
            FRAME.set(FRAME.get().wrapping_add(1));
            next_frame().await;
        }
    });
}

impl BoardCamera {
    pub(crate) fn drag_input(&mut self, allowed: bool, ui_scale: f32) {
        self.pointer_input(allowed, ui_scale, |_, _| false);
    }

    pub(crate) fn pointer_input(
        &mut self,
        allowed: bool,
        ui_scale: f32,
        mut route: impl FnMut(&mut BoardCamera, PointerEvent) -> bool,
    ) {
        let frame = FRAME.get();
        #[cfg(windows)]
        let allowed = {
            let has_drag_input = self.dragging() || macroquad::input::is_mouse_button_pressed(MouseButton::Middle);
            allowed && (!has_drag_input || windows_capture::has_focus())
        };
        #[cfg(windows)]
        let capture_continues = self.drag_capture.as_ref().is_none_or(Capture::is_current);
        #[cfg(not(windows))]
        let capture_continues = true;
        self.begin_drag_frame(frame, allowed, capture_continues);
        let mut receiver = DragReceiver::new(
            allowed.then_some(&mut *self),
            miniquad::window::dpi_scale() * ui_scale,
        );
        receiver.route = Some(&mut route);
        let subscription = SUBSCRIPTION.get().expect("desktop drag input is registered at launch");
        utils::repeat_all_miniquad_input(&mut receiver, subscription);
        if self.dragging() && !is_mouse_button_down(MouseButton::Middle) {
            self.cancel_drag();
        }
        #[cfg(windows)]
        if self.dragging() {
            if let Some(capture) = &self.drag_capture {
                capture.use_on(frame);
            } else if windows_capture::attached() {
                self.drag_capture = Capture::begin(frame);
                if self.drag_capture.is_none() {
                    self.cancel_drag();
                }
            }
        }
    }

    fn begin_drag_frame(&mut self, frame: usize, allowed: bool, capture_continues: bool) {
        // A modal or another camera can own intervening frames. Require a new
        // press on return so their discarded events cannot continue this drag.
        let continued = self.last_drag_frame
            .is_some_and(|last| last == frame || last == frame.wrapping_sub(1));
        if !allowed || !continued || !capture_continues {
            self.cancel_drag();
        }
        self.last_drag_frame = Some(frame);
    }

    pub(crate) fn cancel_drag(&mut self) {
        self.drag_position = None;
        #[cfg(windows)]
        {
            self.drag_capture = None;
        }
    }

    fn drag_to(&mut self, point: [f32; 2]) {
        if let Some([x, y]) = self.drag_position {
            self.pan([point[0] - x, point[1] - y]);
            self.drag_position = Some(point);
        }
    }
}

type PointerRoute<'a> = dyn FnMut(&mut BoardCamera, PointerEvent) -> bool + 'a;

struct DragReceiver<'a> {
    camera: Option<&'a mut BoardCamera>,
    coordinate_scale: f32,
    route: Option<&'a mut PointerRoute<'a>>,
}

impl<'a> DragReceiver<'a> {
    fn new(camera: Option<&'a mut BoardCamera>, coordinate_scale: f32) -> Self {
        Self { camera, coordinate_scale, route: None }
    }

    fn point(&self, x: f32, y: f32) -> [f32; 2] {
        [x / self.coordinate_scale, y / self.coordinate_scale]
    }

    fn dispatch(&mut self, event: PointerEvent) {
        let Some(camera) = &mut self.camera else { return; };
        if self.route.as_mut().is_some_and(|route| route(camera, event)) {
            camera.cancel_drag();
            return;
        }
        match event {
            PointerEvent::Press(MouseButton::Middle, point) => {
                camera.cancel_drag();
                camera.drag_position = camera.contains(point).then_some(point);
            }
            PointerEvent::Move(point) => camera.drag_to(point),
            PointerEvent::Release(MouseButton::Middle, point) => {
                camera.drag_to(point);
                camera.cancel_drag();
            }
            PointerEvent::Cancel => camera.cancel_drag(),
            _ => {}
        }
    }
}

impl EventHandler for DragReceiver<'_> {
    fn update(&mut self) {}

    fn draw(&mut self) {}

    fn mouse_button_down_event(&mut self, button: MouseButton, x: f32, y: f32) {
        self.dispatch(PointerEvent::Press(button, self.point(x, y)));
    }

    fn mouse_motion_event(&mut self, x: f32, y: f32) {
        self.dispatch(PointerEvent::Move(self.point(x, y)));
    }

    fn mouse_button_up_event(&mut self, button: MouseButton, x: f32, y: f32) {
        self.dispatch(PointerEvent::Release(button, self.point(x, y)));
    }

    fn window_minimized_event(&mut self) {
        self.dispatch(PointerEvent::Cancel);
    }

    // The backend can forward simulated mouse events through its own mouse
    // callbacks. A separate touch event cannot take over an active middle drag.
    fn touch_event(&mut self, _: TouchPhase, _: u64, _: f32, _: f32) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_lite::hex::Hex;
    use rustnoth_gui::layout::Viewport;

    const FIRST: Hex = Hex { x: 0, y: 0 };

    fn zoomed_camera() -> BoardCamera {
        let viewport = Viewport { x: 20.0, y: 68.0, width: 240.0, height: 200.0 };
        let mut camera = BoardCamera::default();
        camera.fit(viewport, 6, 6, false);
        camera.scroll([140.0, 168.0], super::super::WHEEL_STEP * 12.0);
        camera.pan([2000.0, 2000.0]);
        camera
    }

    #[test]
    fn middle_drag_keeps_motion_after_press_and_before_release_in_the_same_frame() {
        let mut camera = zoomed_camera();
        camera.begin_drag_frame(1, true, true);
        let mut input = DragReceiver::new(Some(&mut camera), 2.0);

        input.mouse_button_down_event(MouseButton::Middle, 38.0, 312.0);
        input.mouse_motion_event(176.0, 272.0);
        assert_eq!(input.camera.as_ref().unwrap().layout().unwrap().tile_origin(FIRST), [74.0, 140.0]);
        input.mouse_button_up_event(MouseButton::Middle, 176.0, 272.0);

        input.mouse_button_down_event(MouseButton::Middle, 216.0, 312.0);
        input.mouse_motion_event(176.0, 272.0);
        assert_eq!(input.camera.as_ref().unwrap().layout().unwrap().tile_origin(FIRST), [54.0, 120.0]);
        assert!(input.camera.as_ref().unwrap().dragging());

        input.mouse_button_up_event(MouseButton::Middle, 156.0, 252.0);
        input.mouse_motion_event(136.0, 232.0);
        assert_eq!(camera.layout().unwrap().tile_origin(FIRST), [44.0, 110.0]);
        assert!(!camera.dragging());
    }

    #[test]
    fn middle_drag_stops_after_an_inactive_frame_or_focus_loss() {
        let mut camera = zoomed_camera();
        camera.begin_drag_frame(1, true, true);
        let mut input = DragReceiver::new(Some(&mut camera), 1.0);
        input.mouse_button_down_event(MouseButton::Middle, 108.0, 156.0);
        assert!(input.camera.as_ref().unwrap().dragging());
        input.window_minimized_event();
        assert!(!input.camera.as_ref().unwrap().dragging());
        input.mouse_button_down_event(MouseButton::Middle, 108.0, 156.0);

        camera.begin_drag_frame(3, true, true);
        camera.drag_to([88.0, 136.0]);
        assert_eq!(camera.layout().unwrap().tile_origin(FIRST), [74.0, 140.0]);
        assert!(!camera.dragging());
    }

    #[test]
    fn motion_after_capture_loss_requires_a_new_press() {
        let mut camera = zoomed_camera();
        camera.begin_drag_frame(1, true, true);
        let mut input = DragReceiver::new(Some(&mut camera), 1.0);
        input.mouse_button_down_event(MouseButton::Middle, 108.0, 156.0);
        assert!(camera.dragging());

        camera.begin_drag_frame(2, true, false);
        camera.drag_to([88.0, 136.0]);
        assert_eq!(camera.layout().unwrap().tile_origin(FIRST), [74.0, 140.0]);
        assert!(!camera.dragging());
    }
}
