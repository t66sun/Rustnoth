use std::{
    cell::Cell,
    io,
    sync::{Arc, atomic::{AtomicBool, Ordering}},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use macroquad::{miniquad, prelude::*};

const FRAME_MESSAGE: u32 = 0x8000 + 42;
const WM_NCDESTROY: u32 = 0x0082;
const WM_MOUSEMOVE: u32 = 0x0200;
const DRAG_BUTTONS: usize = 0x0001 | 0x0010;
const VREFRESH: i32 = 116;
const DRAG_PERIOD: Duration = Duration::from_millis(3);

type Subclass = unsafe extern "system" fn(isize, u32, usize, isize, usize, usize) -> isize;

#[link(name = "opengl32")]
unsafe extern "system" {
    fn wglGetCurrentDC() -> isize;
}

#[link(name = "user32")]
unsafe extern "system" {
    fn WindowFromDC(dc: isize) -> isize;
    fn PostMessageW(window: isize, message: u32, wparam: usize, lparam: isize) -> i32;
}

#[link(name = "gdi32")]
unsafe extern "system" {
    fn GetDeviceCaps(dc: isize, index: i32) -> i32;
}

#[link(name = "comctl32")]
unsafe extern "system" {
    fn SetWindowSubclass(window: isize, callback: Subclass, id: usize, data: usize) -> i32;
    fn RemoveWindowSubclass(window: isize, callback: Subclass, id: usize) -> i32;
    fn DefSubclassProc(window: isize, message: u32, wparam: usize, lparam: isize) -> isize;
}

struct State {
    running: AtomicBool,
    posted: AtomicBool,
}

struct WindowEvents {
    state: Arc<State>,
    latest_drag_wake: Cell<Option<Instant>>,
}

pub(crate) struct FrameTicker {
    window: isize,
    events: Box<WindowEvents>,
    timer: Option<JoinHandle<()>>,
}

pub(crate) fn adjust(config: &mut macroquad::conf::Conf) {
    config.miniquad_conf.platform.blocking_event_loop = true;
    config.miniquad_conf.platform.swap_interval = Some(0);
    config.update_on = Some(macroquad::conf::UpdateTrigger {
        key_down: true,
        mouse_down: true,
        mouse_up: true,
        mouse_wheel: true,
        touch: true,
        ..Default::default()
    });
}

impl FrameTicker {
    pub(crate) fn start() -> io::Result<Self> {
        let dc = unsafe { wglGetCurrentDC() };
        let window = unsafe { WindowFromDC(dc) };
        let refresh = unsafe { GetDeviceCaps(dc, VREFRESH) };
        let state = Arc::new(State {
            running: AtomicBool::new(true),
            posted: AtomicBool::new(false),
        });
        let events = Box::new(WindowEvents { state, latest_drag_wake: Cell::new(None) });
        let data = &*events as *const WindowEvents as usize;
        if window == 0 || unsafe { SetWindowSubclass(window, notify, 0, data) } == 0 {
            return Err(io::Error::other("could not attach frame scheduling to the Rustnoth window"));
        }
        let mut ticker = Self { window, events, timer: None };
        let clock_state = Arc::clone(&ticker.events.state);
        ticker.timer = Some(thread::Builder::new().name("frame timer".into()).spawn(move || {
            let mut deadline = Instant::now();
            let period = frame_period(refresh);
            while clock_state.running.load(Ordering::Acquire) {
                deadline = (deadline + period).max(Instant::now());
                thread::sleep(deadline.saturating_duration_since(Instant::now()));
                if !clock_state.running.load(Ordering::Acquire) {
                    break;
                }
                // At most one wake is pending during a slow frame or a window resize.
                if !clock_state.posted.swap(true, Ordering::AcqRel)
                    && unsafe { PostMessageW(window, FRAME_MESSAGE, 0, 0) } == 0
                {
                    break;
                }
            }
        })?);
        let display = Arc::clone(&ticker.events.state);
        macroquad::experimental::coroutines::start_coroutine(async move {
            while display.running.load(Ordering::Acquire) {
                // Finish this back buffer before handing it to the compositor.
                // With driver swap waiting disabled, an unfinished frame can
                // otherwise leave the desktop with a stale or cleared image.
                unsafe {
                    get_internal_gl().flush();
                    miniquad::gl::glFinish();
                }
                next_frame().await;
            }
        });
        Ok(ticker)
    }
}

impl Drop for FrameTicker {
    fn drop(&mut self) {
        self.events.state.running.store(false, Ordering::Release);
        if let Some(timer) = self.timer.take() {
            let _ = timer.join();
        }
        unsafe { RemoveWindowSubclass(self.window, notify, 0); }
    }
}

fn frame_period(refresh: i32) -> Duration {
    let refresh = if refresh > 1 { refresh } else { 60 };
    Duration::from_secs_f64(1.0 / f64::from(refresh))
}

fn drag_wake(latest: &Cell<Option<Instant>>, buttons: usize, now: Instant) -> bool {
    if buttons & DRAG_BUTTONS == 0 || latest.get().is_some_and(|last| now - last < DRAG_PERIOD) {
        return false;
    }
    latest.set(Some(now));
    true
}

unsafe extern "system" fn notify(window: isize, message: u32, wparam: usize, lparam: isize, _: usize, data: usize) -> isize {
    // The GUI owns this stable allocation until the callback is removed.
    let events = unsafe { &*(data as *const WindowEvents) };
    if message == FRAME_MESSAGE {
        events.state.posted.store(false, Ordering::Release);
        miniquad::window::schedule_update();
        return 0;
    }
    if message == WM_MOUSEMOVE && drag_wake(&events.latest_drag_wake, wparam, Instant::now()) {
        miniquad::window::schedule_update();
    }
    if message == WM_NCDESTROY {
        events.state.running.store(false, Ordering::Release);
        unsafe { RemoveWindowSubclass(window, notify, 0); }
    }
    unsafe { DefSubclassProc(window, message, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mouse_drag_wakes_immediately_and_coalesces_a_burst_under_three_milliseconds() {
        let latest = Cell::new(None);
        let start = Instant::now();
        assert!(!drag_wake(&latest, 0, start));
        assert!(drag_wake(&latest, 0x10, start));
        assert!(!drag_wake(&latest, 0x10, start + Duration::from_millis(2)));
        assert!(drag_wake(&latest, 0x01, start + Duration::from_millis(3)));
        assert_eq!(frame_period(90), Duration::from_secs_f64(1.0 / 90.0));
    }
}
