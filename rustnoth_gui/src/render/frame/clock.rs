//! In an untimed game Wesnoth's report_countdown displays the local clock.
use super::*;
use std::time::{SystemTime, UNIX_EPOCH};

pub(super) struct Clock {
    icon: Texture,
    twelve_hour: bool,
    has_battery: bool,
    last_minute: Option<(u64, u16)>,
    label: String,
    #[cfg(windows)]
    texture: Texture,
}

fn label(hour: i32, minute: i32, twelve_hour: bool) -> String {
    if twelve_hour {
        let period = if hour < 12 { "AM" } else { "PM" };
        format!("{:02}:{minute:02} {period}", (hour + 11) % 12 + 1)
    } else {
        format!("{hour:02}:{minute:02}")
    }
}

// The 1024×600 theme inherits the 1024×768 top bar. Only 800×600
// moves the clock next to the compact income field and reduces the font.
fn placement(width: f32, has_battery: bool) -> (f32, f32, u16) {
    if width < 1024. {
        (515., 5., 13)
    } else if width < 1152. || !has_battery {
        (729., 1., 15)
    } else {
        (820., 3., 15)
    }
}

impl Clock {
    pub(super) fn new(root: &Path) -> Self {
        let twelve_hour = crate::title_screen::profile::read_profile().ok().is_some_and(|p|
            matches!(p.attribute("use_twelve_hour_clock_format"), Some("true" | "yes" | "1" | "on")));
        Self { icon: load_texture(&root.join("data/core/images/themes/sand-clock.png"), None),
            twelve_hour, has_battery: has_battery(), last_minute: None, label: String::new(),
            #[cfg(windows)] texture: None }
    }

    pub(super) fn draw(&mut self, frame: &BattlefieldFrame, font: Option<&Font>,
        #[cfg(windows)] renderer: Option<&crate::title_screen::tip_pango::Text>) {
        let (x, y, point) = placement(frame.sidebar.right() / frame.scale, self.has_battery);
        let now = SystemTime::now();
        let Ok(elapsed) = now.duration_since(UNIX_EPOCH) else { return };
        let key = (elapsed.as_secs() / 60, point);
        if self.last_minute != Some(key) {
            let Some(local) = crate::desktop::local_date(now) else { return };
            self.label = label(local.tm_hour, local.tm_min, self.twelve_hour);
            #[cfg(windows)]
            {
                self.texture = renderer.map(|r| {
                    let texture = Texture2D::from_image(&r.button(&self.label, point, [221; 3]));
                    texture.set_filter(FilterMode::Nearest);
                    texture
                });
            }
            self.last_minute = Some(key);
        }
        let scale = frame.scale;
        let mut at = (vec2(x, y) * scale).floor();
        if let Some(icon) = &self.icon {
            let size = icon.size() * scale;
            draw_texture_ex(icon, at.x, at.y, WHITE, DrawTextureParams { dest_size: Some(size), ..Default::default() });
            at.x += size.x;
        }
        #[cfg(windows)]
        if let Some(texture) = &self.texture {
            draw_texture_ex(texture, at.x, at.y, WHITE, DrawTextureParams {
                dest_size: Some(texture.size() * scale), ..Default::default()
            });
            return;
        }
        crate::dialog::text(font, &self.label, at.x, at.y + f32::from(point) * scale,
            (f32::from(point) * scale).round().max(1.) as u16, Color::from_rgba(221, 221, 221, 255));
    }
}

#[cfg(windows)]
fn has_battery() -> bool {
    #[repr(C)]
    #[derive(Default)]
    struct PowerStatus {
        ac: u8, battery_flag: u8, percent: u8, system_flag: u8, remaining: u32, full: u32,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" { fn GetSystemPowerStatus(status: *mut PowerStatus) -> i32; }
    let mut status = PowerStatus::default();
    // Wesnoth's Windows theme uses the SYSTEM_POWER_STATUS "no system battery" bit.
    unsafe { GetSystemPowerStatus(&mut status) != 0 && status.battery_flag & 128 == 0 }
}

#[cfg(target_os = "macos")]
fn has_battery() -> bool {
    std::process::Command::new("/usr/bin/pmset").args(["-g", "batt"]).output().ok()
        .is_some_and(|info| info.status.success() && String::from_utf8_lossy(&info.stdout).contains("InternalBattery"))
}

#[cfg(not(any(windows, target_os = "macos")))]
fn has_battery() -> bool {
    std::fs::read_dir("/sys/class/power_supply").into_iter().flatten().flatten().any(|entry| {
        std::fs::read_to_string(entry.path().join("type")).is_ok_and(|s| s.trim() == "Battery")
            && !std::fs::read_to_string(entry.path().join("scope")).is_ok_and(|s| s.trim() == "Device")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn battlefield_clock_follows_the_native_top_bar_and_local_hour_format() {
        assert_eq!(placement(1200., true), (820., 3., 15));
        assert_eq!(placement(1200., false), (729., 1., 15));
        assert_eq!(placement(1024., true), placement(1200., false));
        assert_eq!(placement(800., true), (515., 5., 13));
        assert_eq!(label(0, 1, true), "12:01 AM");
        assert_eq!(label(12, 0, true), "12:00 PM");
        assert_eq!(label(14, 32, true), "02:32 PM");
        assert_eq!(label(4, 7, false), "04:07");
    }
}
