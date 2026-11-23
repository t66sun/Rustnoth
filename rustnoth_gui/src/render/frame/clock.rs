//! The battery and local clock share the end of the native battlefield's top bar.
use super::*;
use std::time::{SystemTime, UNIX_EPOCH};

pub(super) struct Clock {
    time: Report,
    battery: Report,
    twelve_hour: bool,
    charge: Option<u8>,
    last_minute: Option<(u64, u16)>,
}

struct Report {
    icon: Texture,
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
        let themes = root.join("data/core/images/themes");
        Self { time: Report::new(&themes.join("sand-clock.png")), battery: Report::new(&themes.join("battery.png")),
            twelve_hour, charge: None, last_minute: None }
    }

    pub(super) fn draw(&mut self, frame: &BattlefieldFrame, font: Option<&Font>,
        #[cfg(windows)] renderer: Option<&crate::title_screen::tip_pango::Text>) {
        let width = frame.sidebar.right() / frame.scale;
        let now = SystemTime::now();
        let Ok(elapsed) = now.duration_since(UNIX_EPOCH) else { return };
        let minute = elapsed.as_secs() / 60;
        if self.last_minute.is_none_or(|(old, _)| old != minute) {
            self.charge = battery_percentage();
        }
        let (x, y, point) = placement(width, self.charge.is_some());
        let key = (minute, point);
        if self.last_minute != Some(key) {
            let Some(local) = crate::desktop::local_date(now) else { return };
            self.time.set_text(label(local.tm_hour, local.tm_min, self.twelve_hour), point, #[cfg(windows)] renderer);
            if let Some(percent) = self.charge {
                let percent = format!("{percent} %");
                if self.battery.label != percent {
                    self.battery.set_text(percent, 15, #[cfg(windows)] renderer);
                }
            }
            self.last_minute = Some(key);
        }
        if let Some(position) = battery_position(width, self.charge) {
            self.battery.draw(position, 15, frame.scale, font);
        }
        self.time.draw(vec2(x, y), point, frame.scale, font);
    }
}

fn battery_position(width: f32, charge: Option<u8>) -> Option<Vec2> {
    (width >= 1152. && charge.is_some()).then(|| vec2(729., 3.))
}

impl Report {
    fn new(path: &Path) -> Self {
        Self { icon: load_texture(path, None), label: String::new(), #[cfg(windows)] texture: None }
    }

    fn set_text(&mut self, label: String, _point: u16,
        #[cfg(windows)] renderer: Option<&crate::title_screen::tip_pango::Text>) {
        #[cfg(windows)]
        {
            self.texture = renderer.map(|r| {
                let texture = Texture2D::from_image(&r.battlefield_report(&label, _point));
                texture.set_filter(FilterMode::Nearest);
                texture
            });
        }
        self.label = label;
    }

    fn draw(&self, position: Vec2, point: u16, scale: f32, font: Option<&Font>) {
        let mut at = (position * scale).floor();
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
fn battery_percentage() -> Option<u8> {
    #[repr(C)]
    #[derive(Default)]
    struct PowerStatus {
        ac: u8, battery_flag: u8, percent: u8, system_flag: u8, remaining: u32, full: u32,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" { fn GetSystemPowerStatus(status: *mut PowerStatus) -> i32; }
    let mut status = PowerStatus::default();
    // Wesnoth's Windows theme uses the SYSTEM_POWER_STATUS "no system battery" bit.
    let present = unsafe { GetSystemPowerStatus(&mut status) != 0 } && status.battery_flag & 128 == 0;
    present.then_some(status.percent)
}

#[cfg(target_os = "macos")]
fn battery_percentage() -> Option<u8> {
    let info = std::process::Command::new("/usr/bin/pmset").args(["-g", "batt"]).output().ok()?;
    if !info.status.success() { return None; }
    let text = String::from_utf8_lossy(&info.stdout);
    let battery = text.lines().find(|line| line.contains("InternalBattery"))?;
    battery.split_once('%')?.0.split_whitespace().next_back()?.parse().ok()
}

#[cfg(not(any(windows, target_os = "macos")))]
fn battery_percentage() -> Option<u8> {
    std::fs::read_dir("/sys/class/power_supply").ok()?.flatten().find_map(|entry| {
        let dir = entry.path();
        if std::fs::read_to_string(dir.join("type")).ok()?.trim() != "Battery"
            || std::fs::read_to_string(dir.join("scope")).is_ok_and(|s| s.trim() == "Device") {
            return None;
        }
        std::fs::read_to_string(dir.join("capacity")).ok()?.trim().parse().ok()
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

    #[test]
    fn the_battery_report_uses_the_native_wide_laptop_slot() {
        assert_eq!(battery_position(1152., Some(0)), Some(vec2(729., 3.)));
        assert_eq!(battery_position(1200., Some(100)), Some(vec2(729., 3.)));
        assert_eq!(battery_position(1151., Some(73)), None);
        assert_eq!(battery_position(1200., None), None);
    }
}
