use super::{Art, AssetCatalog, Command};
use crate::dialog;
use macroquad::prelude::*;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Menu { File, Map, Window, Addon, Areas, Side }
use Menu::*;

const HEADERS: [(Menu, &str); 6] =
    [(File, "File"), (Map, "Map"), (Window, "Window"), (Addon, "Add-on"), (Areas, "Areas"), (Side, "Side")];

impl Menu {
    fn enabled(self) -> bool { matches!(self, File | Map | Window) }

    fn entries(self) -> &'static [Command] {
        match self {
            File => &[Command::New, Command::Open, Command::Revert, Command::Save, Command::SaveAs, Command::SaveAll, Command::Close, Command::Exit],
            Map => &[Command::Refresh, Command::ToggleGrid, Command::FillSelection],
            _ => &[],
        }
    }

    fn bounds(self) -> Rect {
        let index = HEADERS.iter().position(|&(menu, _)| menu == self).unwrap();
        Rect::new((index * 100) as f32, 1., 100., 20.)
    }
}

pub(super) struct Menubar {
    #[cfg(windows)]
    labels: std::collections::HashMap<&'static str, Texture2D>,
}

impl Menubar {
    pub fn new(root: &Path) -> Self {
        #[cfg(windows)]
        {
            let mut labels = std::collections::HashMap::new();
            if let Some(renderer) = crate::title_screen::tip_pango::Text::load(root, "Lato") {
                for (menu, text) in HEADERS {
                    let rgb = if menu.enabled() { [186, 172, 125] } else { [136; 3] };
                    let image = renderer.button(text, 14, rgb);
                    let tex = Texture2D::from_image(&image);
                    tex.set_filter(FilterMode::Nearest);
                    labels.insert(text, tex);
                }
            }
            Self { labels }
        }
        #[cfg(not(windows))]
        { let _ = root; Self {} }
    }

    pub fn draw(&self, open: &mut Option<Menu>, point: Vec2, assets: &AssetCatalog,
            art: &mut Art, font: Option<&Font>, names: &[String], can_revert: bool) -> Option<Command> {
        let click = is_mouse_button_pressed(MouseButton::Left);
        let mut triggered = false;
        for (menu, label) in HEADERS {
            let rect = menu.bounds();
            let hot = menu.enabled() && rect.contains(point);
            let suffix = if *open == Some(menu) { "-pressed" } else if hot { "-active" } else { "" };
            let gray = if menu.enabled() { "" } else { "~GS()" };
            art.image(assets, &format!("buttons/button_menu/menu_button_copper_H20{suffix}.png{gray}"), rect);
            #[cfg(windows)]
            let texture = self.labels.get(label);
            #[cfg(not(windows))]
            let texture: Option<&Texture2D> = None;
            if let Some(tex) = texture {
                let x = rect.center().x - (tex.width() * 0.5).floor();
                let y = rect.center().y - (tex.height() * 0.5).floor();
                draw_texture(tex, x, y, WHITE);
            } else {
                let metrics = measure_text(label, font, 14, 1.);
                let color = if menu.enabled() { dialog::GOLD } else { GRAY };
                dialog::text(font, label, rect.center().x - metrics.width * 0.5,
                    rect.center().y + metrics.height * 0.5, 14, color);
            }
            if hot && click {
                *open = if *open == Some(menu) { None } else { Some(menu) };
                triggered = true;
            }
        }
        let menu = (*open)?;
        let mut result = None;
        let entries = menu.entries();
        let count = if menu == Window { names.len() } else { entries.len() };
        for row in 0..count {
            let area = Rect::new(menu.bounds().x, 22. + row as f32 * 27., 192., 27.);
            let hot = area.contains(point);
            let color = if hot { Color::from_rgba(44,65,71,255) } else { Color::from_rgba(31,32,35,255) };
            draw_rectangle(area.x, area.y, area.w, area.h, color);
            let (cmd, label) = if menu == Window {
                (Command::Switch(row), names[row].as_str())
            } else {
                (entries[row], entries[row].label())
            };
            let enabled = cmd != Command::Revert || can_revert;
            dialog::text(font, label, area.x + 9., area.y + 19., 16, if enabled { WHITE } else { GRAY });
            if enabled && hot && click && !triggered { result = Some(cmd); }
        }
        if click && !triggered { *open = None; }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn classic_map_document_menu_uses_the_original_slots_and_routes_map_actions() {
        assert_eq!(Window.bounds(), Rect::new(200.,1.,100.,20.));
        assert_eq!(Side.bounds(), Rect::new(500.,1.,100.,20.));
        assert!(Map.entries().contains(&Command::ToggleGrid));
        assert!(File.entries().contains(&Command::Open));
        assert!(Window.enabled() && !Areas.enabled() && !Side.enabled());
    }
}
