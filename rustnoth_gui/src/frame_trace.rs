//! Opt-in frame and input-availability timestamps for the Windows desktop acceptance run.
use std::{cell::{Cell, RefCell}, fs::{File, OpenOptions}, io::{self, BufWriter, Write}, time::Instant};

thread_local! {
    static TRACE: RefCell<Option<Trace>> = const { RefCell::new(None) };
    static ENABLED: Cell<bool> = const { Cell::new(false) };
}

struct Trace {
    file: BufWriter<File>,
    clock: Instant,
    waiting_for_input: bool,
    frames: u32,
    size: (i64, i64),
    costs: std::collections::BTreeMap<&'static str, (u64, u32)>,
}

pub(crate) fn enabled() -> bool { ENABLED.get() }

pub(crate) fn visible_tiles(view: &engine_lite::observation::Observation) -> std::collections::BTreeSet<engine_lite::hex::Hex> {
    view.tiles.iter().filter(|t| t.visible).map(|t| t.position).collect()
}

pub(crate) fn start(uncapped: bool) -> io::Result<()> {
    let Some(path) = std::env::var_os("RUSTNOTH_FRAME_TRACE") else { return Ok(()) };
    let file = OpenOptions::new().write(true).create_new(true).open(path)?;
    let mut file = BufWriter::new(file);
    writeln!(file, "elapsed_us\tevent\tvalue")?;
    let size = window_size();
    TRACE.with_borrow_mut(|slot| *slot = Some(Trace {
        file, clock: Instant::now(), waiting_for_input: false, frames: 0, size, costs: Default::default(),
    }));
    ENABLED.set(true);
    mark(if uncapped { "uncapped" } else { "default_pacing" });
    record("process_id", i64::from(std::process::id()));
    record("window_width", size.0);
    record("window_height", size.1);
    #[cfg(windows)]
    record("display_hz", display_hz());
    macroquad::experimental::coroutines::start_coroutine(async {
        loop {
            macroquad::window::next_frame().await;
            TRACE.with_borrow_mut(|slot| {
                if let Some(trace) = slot {
                    trace.row("frame", 0);
                    let size = window_size();
                    if size != trace.size {
                        trace.row("window_width", size.0);
                        trace.row("window_height", size.1);
                        trace.size = size;
                    }
                    trace.frames += 1;
                    if trace.frames % 240 == 0 { trace.flush(); }
                }
            });
        }
    });
    Ok(())
}

fn window_size() -> (i64, i64) {
    (macroquad::window::screen_width() as i64, macroquad::window::screen_height() as i64)
}

#[cfg(windows)]
fn display_hz() -> i64 {
    #[link(name = "opengl32")]
    unsafe extern "system" { fn wglGetCurrentDC() -> *mut std::ffi::c_void; }
    #[link(name = "gdi32")]
    unsafe extern "system" { fn GetDeviceCaps(dc: *mut std::ffi::c_void, index: i32) -> i32; }
    i64::from(unsafe { GetDeviceCaps(wglGetCurrentDC(), 116) })
}

impl Trace {
    fn row(&mut self, event: &str, value: i64) {
        writeln!(self.file, "{}\t{event}\t{value}", self.clock.elapsed().as_micros()).expect("write desktop frame trace");
    }
    fn flush(&mut self) { self.file.flush().expect("flush desktop frame trace"); }
}

pub(crate) fn record(name: &str, value: i64) {
    if enabled() { TRACE.with_borrow_mut(|slot| { if let Some(trace) = slot { trace.row(name, value); } }); }
}

pub(crate) fn mark(name: &str) { record(name, 0); }

pub(crate) fn time_asset<T>(stage: &'static str, work: impl FnOnce() -> T) -> T {
    if !enabled() { return work(); }
    let start = Instant::now();
    let result = work();
    let us = start.elapsed().as_micros() as u64;
    TRACE.with_borrow_mut(|slot| {
        if let Some(trace) = slot {
            let cost = trace.costs.entry(stage).or_default();
            cost.0 += us;
            cost.1 += 1;
        }
    });
    result
}

pub(crate) fn finish_assets() {
    if !enabled() { return; }
    TRACE.with_borrow_mut(|slot| {
        if let Some(trace) = slot {
            for (stage, (us, calls)) in std::mem::take(&mut trace.costs) {
                trace.row(stage, us as i64);
                trace.row(&format!("{stage}_calls"), calls.into());
            }
        }
    });
}

pub(crate) fn input(name: &str) {
    if enabled() {
        TRACE.with_borrow_mut(|slot| {
            if let Some(trace) = slot {
                #[cfg(windows)]
                trace.row("display_hz", display_hz());
                trace.row(name, 0);
                trace.waiting_for_input = true;
                trace.flush();
            }
        });
    }
}

pub(crate) fn ready(name: &str) {
    if enabled() {
        TRACE.with_borrow_mut(|slot| {
            if let Some(trace) = slot && std::mem::take(&mut trace.waiting_for_input) {
                trace.row(name, 0);
                trace.flush();
            }
        });
    }
}
