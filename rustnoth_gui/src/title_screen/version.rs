use super::{Art, dialog, image, load_texture, locale::Locale};
use macroquad::prelude::*;
use std::{env, error::Error, path::{Path, PathBuf}};

const WESNOTH_VERSION: &str = "1.18.8";

pub(super) fn footer_bounds(height: f32) -> Rect {
    // The 25px button is centered in the 31px footer row above a 10px margin.
    Rect::new(10., height - 38., 25., 25.)
}

fn version_label(locale: &Locale, value: &str) -> String {
    locale.ui("Version $version").replace("$version|", value).replace("$version", value)
}

pub(super) struct Footer {
    frames: [Texture2D; 3],
    icons: [Texture2D; 3],
}

impl Footer {
    pub fn load(root: &Path) -> Result<Self, Box<dyn Error>> {
        let states = |stem: &str| -> Result<_, Box<dyn Error>> {
            Ok([
                load_texture(root, &format!("{stem}.png"))?,
                load_texture(root, &format!("{stem}-active.png"))?,
                load_texture(root, &format!("{stem}-pressed.png"))?,
            ])
        };
        Ok(Self {
            frames: states("images/buttons/button_square/button_square_25")?,
            icons: states("images/icons/action/about_25")?,
        })
    }

    pub fn draw(&self, art: &Art, rect: Rect, focused: bool, pressed: bool) {
        let state = if pressed { 2 } else { usize::from(focused) };
        image(rect, &self.frames[state]);
        image(rect, &self.icons[state]);
        let label = version_label(&art.locale, WESNOTH_VERSION);
        dialog::text(art.font.as_ref(), &label, rect.right() + 5., rect.y + 18., 16, WHITE);
    }
}

fn game_paths(root: &Path, config: Option<PathBuf>, cache: Option<PathBuf>) -> Vec<(&'static str, PathBuf)> {
    let mut rows = vec![("Game data:", root.to_owned())];
    if let Some(dir) = config {
        rows.push(("Configuration:", dir.clone()));
        rows.push(("Saved games:", dir.join("saves")));
    }
    if let Some(dir) = cache { rows.push(("Cache:", dir)); }
    rows
}

// Cargo.lock identifies the versions statically linked into this executable.
fn linked_libraries() -> Vec<(&'static str, &'static str)> {
    include_str!("../../Cargo.lock").split("[[package]]").filter_map(|pkg| {
        let field = |key: &str| pkg.lines().find_map(|line| line.strip_prefix(key)?.strip_suffix('"'));
        let name = field("name = \"")?;
        if matches!(name, "engine_lite" | "image" | "macroquad" | "miniquad") {
            Some((name, field("version = \"")?))
        } else { None }
    }).collect()
}

fn caption<'a>(locale: &'a Locale, key: &'a str) -> &'a str {
    let label = locale.ui(key);
    label.split_once('^').map_or(label, |(_, text)| text)
}

fn path_open(dir: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    let mut opener = std::process::Command::new("explorer.exe");
    #[cfg(target_os="macos")]
    let mut opener = std::process::Command::new("open");
    #[cfg(not(any(windows, target_os="macos")))]
    let mut opener = std::process::Command::new("xdg-open");
    opener.arg(dir).spawn().map(|_| ())
}

pub(super) async fn show(root: &Path, art: &Art) -> Result<(), Box<dyn Error>> {
    let backdrop = Texture2D::from_image(&get_screen_data());
    let icon = load_texture(root, "data/core/images/wesnoth-icon.png")?;
    let skin = dialog::Skin::load(root);
    let paths = game_paths(root, crate::display_preferences::config_directory().ok(), crate::desktop::cache_directory().ok());
    let libs = linked_libraries();
    let available: Vec<_> = paths.iter().map(|(_, dir)| dir.is_dir()).collect();
    let platforms = format!("{} {}", env::consts::OS, env::consts::ARCH);
    let platform = art.locale.ui("Running on $os").replace("$os|", &platforms).replace("$os", &platforms);
    let revision = format!("{}   ·   Rustnoth {}", version_label(&art.locale, WESNOTH_VERSION), env!("CARGO_PKG_VERSION"));
    let tabs = ["Paths", "Libraries", "Features"];
    let features = ["Map Editor", "Multiplayer", "Campaigns"];
    let mut report = format!("{revision}\n{platform}\n");
    for (key, dir) in &paths {
        report.push_str(&format!("{} {}\n", art.locale.ui(key), dir.display()));
    }
    for (name, version) in &libs { report.push_str(&format!("{name} {version}\n")); }
    for feature in features { report.push_str(&format!("{}\n", art.locale.ui(feature))); }
    let mut tab = 0;
    next_frame().await;
    loop {
        let (r, mouse) = skin.frame("", 650., 425., &backdrop);
        let x = r.x + 15.;
        let y = r.y + 15.;
        let text = |s: &str, x, y, size, color| dialog::text(art.font.as_ref(), s, x, y, size, color);
        let button = |bounds: Rect, key: &str, selected: bool| {
            let hover = bounds.contains(mouse);
            art.button(bounds, caption(&art.locale, key), hover || selected, hover && is_mouse_button_down(MouseButton::Left));
            hover && is_mouse_button_pressed(MouseButton::Left)
        };
        image(Rect::new(x, y, icon.width(), icon.height()), &icon);
        let heading_x = x + icon.width() + 10.;
        text(art.locale.ui("The Battle for Wesnoth"), heading_x, y+25., 26, dialog::GOLD);
        text(&revision, heading_x, y+54., 18, WHITE);
        text(&platform, heading_x, y+79., 17, WHITE);
        if is_key_pressed(KeyCode::Right) || is_key_pressed(KeyCode::Tab) {
            tab = (tab + 1) % tabs.len();
        } else if is_key_pressed(KeyCode::Left) { tab = (tab + tabs.len() - 1) % tabs.len(); }
        for (i,key) in tabs.iter().enumerate() {
            if button(Rect::new(heading_x + i as f32 * 135., y + 99., 132., 30.), key, i == tab) { tab = i; }
        }
        let row = |index: usize| y + 143. + index as f32 * 41.;
        match tab {
            0 => for (i, (key, path)) in paths.iter().enumerate() {
                let y = row(i);
                text(art.locale.ui(key), x, y+23., 16, WHITE);
                draw_rectangle(x+115., y, 310., 30., Color::from_rgba(19,22,26,255));
                let value = path.to_string_lossy();
                let label = dialog::fit(art.font.as_ref(), value.to_string(), 300., 16);
                text(&label, x+120., y+22., 16, WHITE);
                if button(Rect::new(x+430., y, 83., 30.), "filesystem^Copy", false) {
                    macroquad::miniquad::window::clipboard_set(&value);
                }
                let browse = Rect::new(x+519., y, 99., 30.);
                if available[i] {
                    if button(browse, "filesystem^Browse", false) { path_open(path)?; }
                } else {
                    skin.button(browse, mouse, caption(&art.locale, "filesystem^Browse"), false);
                }
            },
            1 => {
                text(caption(&art.locale, "library^Name"), x+8., row(0)+21., 17, dialog::GOLD);
                text(caption(&art.locale, "library^Build version"), x+290., row(0)+21., 17, dialog::GOLD);
                for (i, (name, version)) in libs.iter().enumerate() {
                    text(name, x+8., row(i+1)+16., 17, WHITE);
                    text(version, x+290., row(i+1)+16., 17, WHITE);
                }
            },
            _ => for (i, key) in features.iter().enumerate() {
                text(art.locale.ui(key), x+8., row(i)+21., 17, WHITE);
                text(art.locale.ui("Yes"), x+290., row(i)+21., 17, WHITE);
            },
        }
        if button(Rect::new(x, r.bottom()-45., 100., 30.), "page^Copy", false) {
            macroquad::miniquad::window::clipboard_set(&report);
        }
        let close = button(Rect::new(r.right()-115., r.bottom()-45., 100., 30.), "Close", false);
        set_default_camera();
        if close || is_key_pressed(KeyCode::Escape) || is_key_pressed(KeyCode::Enter) { return Ok(()); }
        next_frame().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn about_uses_the_title_footer_and_reports_the_active_game_directories() {
        assert_eq!(footer_bounds(820.), Rect::new(10., 782., 25., 25.));
        assert_eq!(version_label(&Locale::default(), WESNOTH_VERSION), "Version 1.18.8");
        let rows = game_paths(Path::new("assets"), Some("profile".into()), Some("cache".into()));
        assert_eq!(rows, [
            ("Game data:", PathBuf::from("assets")), ("Configuration:", PathBuf::from("profile")),
            ("Saved games:", Path::new("profile").join("saves")), ("Cache:", PathBuf::from("cache")),
        ]);
        assert!(linked_libraries().contains(&("macroquad","0.4.16")));
    }
}
