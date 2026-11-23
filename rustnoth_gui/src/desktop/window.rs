//! The GUI's artwork, mouse positions and F12 captures use framebuffer pixels.

#[cfg(not(windows))]
pub(crate) fn initial_size() -> (i32, i32) {
    (1200, 820)
}

#[cfg(windows)]
pub(crate) fn initial_size() -> (i32, i32) {
    use std::ffi::c_void;

    #[repr(C)]
    struct Point { x: i32, y: i32 }

    #[link(name = "shcore")]
    unsafe extern "system" {
        fn SetProcessDpiAwareness(awareness: i32) -> i32;
        fn GetDpiForMonitor(monitor: *mut c_void, kind: i32, x: *mut u32, y: *mut u32) -> i32;
    }
    #[link(name = "user32")]
    unsafe extern "system" {
        fn MonitorFromPoint(point: Point, flags: u32) -> *mut c_void;
        fn GetSystemMetrics(index: i32) -> i32;
    }
    const PROCESS_PER_MONITOR_DPI_AWARE: i32 = 2;
    const MONITOR_DEFAULTTOPRIMARY: u32 = 1;
    const MDT_EFFECTIVE_DPI: i32 = 0;
    const SM_CXFULLSCREEN: i32 = 16;
    const SM_CYFULLSCREEN: i32 = 17;

    // Opt out of Windows bitmap scaling before creating the first window.
    // Miniquad's high_dpi stays false so Macroquad also uses physical pixels
    // for layout and hit testing instead of dividing by the monitor scale.
    unsafe {
        SetProcessDpiAwareness(PROCESS_PER_MONITOR_DPI_AWARE);
        let monitor = MonitorFromPoint(Point { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
        let (mut x, mut y) = (96, 96);
        if GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut x, &mut y) != 0 {
            (x, y) = (96, 96);
        }
        (
            window_extent(1200, x, GetSystemMetrics(SM_CXFULLSCREEN)),
            window_extent(820, y, GetSystemMetrics(SM_CYFULLSCREEN)),
        )
    }
}

#[cfg(any(windows, test))]
fn window_extent(logical: i32, dpi: u32, available: i32) -> i32 {
    let pixels = (logical * dpi as i32 + 48) / 96;
    if available > 0 { pixels.min(available) } else { pixels }
}

#[cfg(test)]
mod tests {
    use super::window_extent;

    #[test]
    fn initial_client_uses_physical_pixels_and_fits_the_work_area() {
        assert_eq!(window_extent(1200, 192, 2880), 2400);
        assert_eq!(window_extent(820, 192, 1710), 1640);
        assert_eq!(window_extent(820, 144, 1000), 1000);
        assert_eq!(window_extent(1200, 120, 1920), 1500);
    }
}
