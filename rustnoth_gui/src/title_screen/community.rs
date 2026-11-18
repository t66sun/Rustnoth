use super::{Art, panel};
use crate::dialog;
use macroquad::prelude::*;
use std::{io, process::Command};

const LINKS: [(&str, &str); 6] = [
    ("Forums", "https://forums.wesnoth.org/"),
    ("Discord", "https://discord.gg/battleforwesnoth"),
    ("IRC", "https://web.libera.chat/#wesnoth"),
    (
        "Steam",
        "https://steamcommunity.com/app/599390/discussions/",
    ),
    ("Reddit", "https://www.reddit.com/r/wesnoth/"),
    ("Donate", "https://www.spi-inc.org/projects/wesnoth/"),
];

fn browse(link: &'static str) -> io::Result<()> {
    #[cfg(windows)]
    let mut app = {
        use std::os::windows::process::CommandExt;
        let mut cmd = Command::new("rundll32.exe");
        cmd.arg("url.dll,FileProtocolHandler")
            .creation_flags(0x08000000);
        cmd
    };
    #[cfg(target_os = "macos")]
    let mut app = Command::new("open");
    #[cfg(not(any(windows, target_os = "macos")))]
    let mut app = Command::new("xdg-open");
    app.arg(link).spawn().map(|_| ())
}

pub(super) async fn show(art: &Art) {
    let mut focus = 0;
    let mut error = None;
    loop {
        let scale = (screen_width() / 360.0)
            .min(screen_height() / 470.0)
            .clamp(0.01, 1.0);
        let (w, h) = (screen_width() / scale, screen_height() / scale);
        let p = Vec2::from(mouse_position()) / scale;
        set_camera(&dialog::setup_camera(w, h));
        art.draw_background(w, h);
        let frame = Rect::new((w - 310.0) * 0.5, (h - 445.0) * 0.5, 310.0, 445.0);
        panel(frame);
        dialog::text(
            art.font.as_ref(),
            art.locale.ui("Community"),
            frame.x + 15.0,
            frame.y + 37.0,
            25,
            dialog::GOLD,
        );
        if is_key_pressed(KeyCode::Down) || is_key_pressed(KeyCode::Tab) {
            focus = (focus + 1) % (LINKS.len() + 1);
        }
        if is_key_pressed(KeyCode::Up) {
            focus = (focus + LINKS.len()) % (LINKS.len() + 1);
        }
        let mut activated = None;
        for (i, name) in LINKS
            .iter()
            .map(|(s, _)| *s)
            .chain(std::iter::once("Close"))
            .enumerate()
        {
            let r = Rect::new(
                frame.center().x - 89.0,
                frame.y + 57.0 + i as f32 * 50.0,
                178.0,
                32.0,
            );
            let over = r.contains(p);
            art.button(
                r,
                art.locale.ui(name),
                over || i == focus,
                over && is_mouse_button_down(MouseButton::Left),
            );
            if over && is_mouse_button_pressed(MouseButton::Left) {
                activated = Some(i);
                focus = i;
            }
        }
        if is_key_pressed(KeyCode::Enter) || is_key_pressed(KeyCode::Space) {
            activated = Some(focus);
        }
        if let Some(msg) = &error {
            let msg = dialog::fit(art.font.as_ref(), format!("{msg}"), frame.w - 20.0, 13);
            dialog::text(
                art.font.as_ref(),
                &msg,
                frame.x + 10.0,
                frame.bottom() - 8.0,
                13,
                dialog::WARNING,
            );
        }
        set_default_camera();
        if activated == Some(LINKS.len()) || is_key_pressed(KeyCode::Escape) {
            return;
        }
        if let Some(i) = activated {
            error = browse(LINKS[i].1).err().map(|e| e.to_string());
        }
        next_frame().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn each_stock_community_button_has_its_distinct_native_destination() {
        let captions: Vec<_> = LINKS.iter().map(|(name, _)| *name).collect();
        assert_eq!(
            captions,
            ["Forums", "Discord", "IRC", "Steam", "Reddit", "Donate"]
        );
        assert_eq!(LINKS[2].1, "https://web.libera.chat/#wesnoth");
        assert!(LINKS.iter().all(|(_, url)| url.starts_with("https://")));
    }
}
