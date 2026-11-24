use std::{cell::Cell, ffi::c_void, ptr::NonNull};

type Window = *mut c_void;
type Subclass = unsafe extern "system" fn(Window, u32, usize, isize, usize, usize) -> isize;

#[link(name = "comctl32")]
unsafe extern "system" {
    fn SetWindowSubclass(window: Window, procedure: Subclass, id: usize, data: usize) -> i32;
    fn RemoveWindowSubclass(window: Window, procedure: Subclass, id: usize) -> i32;
    fn DefSubclassProc(window: Window, message: u32, wparam: usize, lparam: isize) -> isize;
}

#[link(name = "user32")]
unsafe extern "system" {
    fn GetForegroundWindow() -> Window;
    fn GetFocus() -> Window;
    fn GetCapture() -> Window;
    fn GetAsyncKeyState(key: i32) -> i16;
    fn SetCapture(window: Window) -> Window;
    fn ReleaseCapture() -> i32;
}

const SUBCLASS_ID: usize = 1;
const VK_MBUTTON: i32 = 0x04;
const WM_KILLFOCUS: u32 = 0x0008;
const WM_CANCELMODE: u32 = 0x001f;
const WM_NCDESTROY: u32 = 0x0082;
const WM_CAPTURECHANGED: u32 = 0x0215;

#[derive(Clone, Copy)]
struct Active {
    id: usize,
    frame: usize,
}

thread_local! {
    static WINDOW: Cell<Option<NonNull<c_void>>> = const { Cell::new(None) };
    static ACTIVE: Cell<Option<Active>> = const { Cell::new(None) };
    static NEXT_ID: Cell<usize> = const { Cell::new(0) };
}

pub(super) fn attach() {
    if let Some(window) = crate::desktop::window::native_handle()
        && unsafe { SetWindowSubclass(window.as_ptr(), on_message, SUBCLASS_ID, 0) } != 0
    {
        WINDOW.set(Some(window));
    }
}

pub(super) fn attached() -> bool {
    WINDOW.get().is_some()
}

fn focused(window: NonNull<c_void>) -> bool {
    unsafe { GetForegroundWindow() == window.as_ptr() && GetFocus() == window.as_ptr() }
}

pub(super) fn has_focus() -> bool {
    WINDOW.get().is_none_or(focused)
}

pub(in crate::camera) struct Capture(usize);

impl Capture {
    pub(super) fn begin(frame: usize) -> Option<Self> {
        let window = WINDOW.get()?.as_ptr();
        // A short press may already have ended outside the client before replay.
        if !has_focus() || unsafe { !GetCapture().is_null() || GetAsyncKeyState(VK_MBUTTON) >= 0 } {
            return None;
        }
        unsafe { SetCapture(window) };
        if unsafe { GetCapture() } != window {
            return None;
        }
        let id = NEXT_ID.get();
        NEXT_ID.set(id.wrapping_add(1));
        ACTIVE.set(Some(Active { id, frame }));
        Some(Self(id))
    }

    pub(super) fn is_current(&self) -> bool {
        ACTIVE.get().is_some_and(|active| active.id == self.0)
    }

    pub(super) fn use_on(&self, frame: usize) {
        if self.is_current() {
            ACTIVE.set(Some(Active { id: self.0, frame }));
        }
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        if self.is_current() {
            release();
        }
    }
}

pub(super) fn release_unused(frame: usize) {
    if ACTIVE.get().is_some_and(|active| active.frame != frame) {
        release();
    }
}

fn release() {
    // ReleaseCapture can synchronously call our window procedure.
    if ACTIVE.replace(None).is_some()
        && let Some(window) = WINDOW.get()
        && unsafe { GetCapture() } == window.as_ptr()
    {
        unsafe { ReleaseCapture() };
    }
}

unsafe extern "system" fn on_message(
    window: Window, message: u32, wparam: usize, lparam: isize, id: usize, _: usize,
) -> isize {
    match message {
        WM_CAPTURECHANGED => ACTIVE.set(None),
        WM_KILLFOCUS | WM_CANCELMODE => release(),
        WM_NCDESTROY => {
            release();
            WINDOW.set(None);
            unsafe { RemoveWindowSubclass(window, on_message, id) };
        }
        _ => {}
    }
    unsafe { DefSubclassProc(window, message, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_camera_guard_cannot_release_a_newer_capture() {
        let stale = Capture(1);
        ACTIVE.set(Some(Active { id: 2, frame: 4 }));
        drop(stale);
        assert!(ACTIVE.get().is_some_and(|active| active.id == 2));

        release_unused(4);
        assert!(ACTIVE.get().is_some());
        release_unused(5);
        assert!(ACTIVE.get().is_none());
    }
}
