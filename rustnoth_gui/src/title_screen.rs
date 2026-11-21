//! Native Wesnoth 1.18.8 title artwork and local game entry points.
use std::{error::Error, fs, path::{Path, PathBuf}};

use engine_lite::wml;
use macroquad::prelude::*;

mod preferences;
mod hotkeys;
mod help;
pub(crate) mod locale;
mod languages;
mod load_game;
mod credits;
mod community;
mod achievements;
pub(crate) use achievements::icons::pixels as icon_pixels;
mod addons;
mod profile;
pub(crate) mod panel_blur;
mod panel_grid;
pub(crate) mod button_canvas;
use crate::artwork::scaled as scaled_art;
mod tip_pages;
#[cfg(windows)]
pub(crate) mod tip_pango;
mod button_labels;
use tip_pages::TipPages;
mod version;

use crate::{campaign_browser::Browser, desktop, dialog, read_file, setup::Setup};

pub(super) enum Games {
    Prepared(desktop::GameData),
    Installed,
}

pub(super) struct TitleScreen {
    pub(super) games: Games,
    pub(super) root: PathBuf,
    pub(super) seed: u32,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum Command { Campaigns, Multiplayer, Load, Addons, Editor, Preferences, Achievements, Community, Credits, Quit, Help, Previous, Next, Language, About }

impl Command {
    fn label(self) -> &'static str {
        match self {
            Self::Campaigns => "Campaigns",
            Self::Multiplayer => "Multiplayer",
            Self::Load => "Load",
            Self::Addons => "Add-ons",
            Self::Editor => "Map Editor",
            Self::Preferences => "Preferences",
            Self::Achievements => "Achievements",
            Self::Credits => "Credits",
            Self::Community => "Community",
            Self::Quit => "Quit",
            Self::Language => "Language",
            Self::About => "Display the game version and build information",
            Self::Help => "Help",
            Self::Previous => "Previous",
            Self::Next => "Next",
        }
    }
}

fn menu(campaign: bool) -> &'static [Command] {
    if campaign { &[Command::Campaigns, Command::Multiplayer, Command::Load, Command::Addons, Command::Editor, Command::Preferences, Command::Achievements, Command::Community, Command::Credits, Command::Quit] }
    else { &[Command::Multiplayer, Command::Load, Command::Addons, Command::Editor, Command::Preferences, Command::Achievements, Command::Community, Command::Credits, Command::Quit] }
}

fn keyboard_focus(current: Option<usize>, count: usize, backward: bool) -> usize {
    match current {
        Some(index) if backward => (index + count - 1) % count,
        Some(index) => (index + 1) % count,
        None if backward => count - 1,
        None => 0,
    }
}

impl TitleScreen {
    pub(super) async fn run(self, mut fullscreen: bool) -> Result<(), Box<dyn Error>> {
        let mut bindings = hotkeys::Bindings::load(&self.root)?;
        let selected = crate::display_preferences::config_directory().and_then(|dir| locale::saved(&dir)).unwrap_or_default();
        let mut art = Art::load(&self.root, locale::Locale::load(&self.root, selected)?)?;
        let tip_source = fs::read_to_string(self.root.join("data/tips.cfg"))?;
        let mut pages = TipPages::new(tips(&tip_source, &art.locale)?);
        let actions = menu(match &self.games {
            Games::Prepared(data) => data.campaign.is_some(),
            Games::Installed => desktop::campaign_available(&self.root),
        });
        let mut tip = self.seed as usize % pages.entries.len();
        let mut focus = None;
        let mut pressed = None;
        let mut last_pointer = None;
        let mut windowed_size = vec2(1200.0, 820.0);
        let mut error: Option<String> = None;
        loop {
            let scale = (screen_width() / 660.0).min(screen_height() / 460.0).clamp(0.01, 1.0);
            let w = screen_width() / scale;
            let h = screen_height() / scale;
            let (mx, my) = mouse_position();
            let pointer = vec2(mx / scale, my / scale);
            set_camera(&dialog::setup_camera(w, h));
            art.draw_background(w, h);
            let (tip_body, attribution) = &pages.entries[tip];
            let buttons = art.draw_panels(w, h, actions, error.as_deref().unwrap_or(tip_body),
                if error.is_none() { attribution } else { "" }, &pages);
            let hovered = buttons.iter().position(|(_, rect)| rect.contains(pointer));
            if last_pointer != Some((mx, my)) {
                focus = hovered;
                last_pointer = Some((mx, my));
            }
            let backwards = is_key_pressed(KeyCode::Up) || (is_key_pressed(KeyCode::Tab)
                && (is_key_down(KeyCode::LeftShift) || is_key_down(KeyCode::RightShift)));
            if backwards || is_key_pressed(KeyCode::Tab) || is_key_pressed(KeyCode::Down) {
                focus = Some(keyboard_focus(focus, buttons.len(), backwards));
            }
            if is_mouse_button_pressed(MouseButton::Left) { pressed = hovered; }
            let click = if is_mouse_button_released(MouseButton::Left) {
                pressed.take().filter(|index| Some(*index) == hovered)
            } else { None };
            for (index, (command, rect)) in buttons.iter().enumerate() {
                art.command(*rect, *command, Some(index) == focus, pressed == Some(index) && hovered == pressed);
            }
            set_default_camera();

            let command = if is_key_pressed(KeyCode::Escape) { Some(Command::Quit) }
                else if is_key_pressed(KeyCode::Enter) {
                    Some(buttons[focus.unwrap_or(0)].0)
                } else {
                    hotkeys::pressed().and_then(|key| bindings.shortcut(&key, actions))
                        .or_else(|| click.map(|index| buttons[index].0))
                };
            // A shortcut opens a modal without moving focus to its title button.
            let title_focus = focus.map(|index| buttons[index].0);
            let draw_title = |w, h| {
                art.draw_background(w, h);
                let (body, source) = &pages.entries[tip];
                for (cmd, rect) in art.draw_panels(w, h, actions, body, source, &pages) {
                    art.command(rect, cmd, Some(cmd) == title_focus, false);
                }
            };
            match command {
                Some(Command::Quit) => return Ok(()),
                Some(Command::About) => {
                    error = version::show(&self.root, &art).await.err().map(|e| e.to_string());
                }
                Some(Command::Previous) => { error = None; tip = (tip + pages.entries.len() - 1) % pages.entries.len(); }
                Some(Command::Next) => { error = None; tip = (tip + 1) % pages.entries.len(); }
                Some(Command::Community) => {
                    next_frame().await;
                    community::show(&art).await;
                }
                Some(Command::Language) => {
                    next_frame().await;
                    let result = languages::show(&self.root, &art, draw_title).await;
                    match result {
                        Ok(Some(id)) => {
                            let change = (|| -> Result<(),Box<dyn Error>> {
                                let locale = locale::Locale::load(&self.root,id.clone())?;
                                let translated = tips(&tip_source, &locale)?;
                                let map = localized_map(&self.root, &locale)?;
                                let logo = load_texture(&self.root, &locale.logo(&self.root))?;
                                locale::save(&crate::display_preferences::config_directory()?, &id)?;
                                art.set_locale(&self.root,locale,logo,map);
                                pages = TipPages::new(translated);
                                Ok(())
                            })();
                            error = change.err().map(|e|e.to_string());
                        }
                        Ok(None) => {}
                        Err(e) => error = Some(e.to_string()),
                    }
                }
                Some(Command::Addons) => {
                    next_frame().await;
                    error = addons::show(&self.root, &art).await.err().map(|cause| cause.to_string());
                }
                Some(Command::Load) => {
                    next_frame().await;
                    error = load_game::show(&self.root, &art, draw_title).await.err().map(|cause| cause.to_string());
                }
                Some(Command::Achievements) => {
                    next_frame().await;
                    error = achievements::show(&self.root, &art, draw_title).await.err().map(|cause| cause.to_string());
                }
                Some(Command::Preferences) => {
                    next_frame().await;
                    let result = preferences::show(&self.root, &art, &mut fullscreen, &mut windowed_size, &mut bindings, draw_title).await;
                    error = result.err().map(|cause| cause.to_string());
                }
                Some(command @ (Command::Campaigns | Command::Multiplayer | Command::Help | Command::Editor | Command::Credits)) => {
                    next_frame().await;
                    error = self.open(command, &art).await.err().map(|cause| cause.to_string());
                }
                None => {}
            }
            next_frame().await;
        }
    }

    async fn open(&self, command: Command, art: &Art) -> Result<(), Box<dyn Error>> {
        let installed;
        let (data, modifications) = match &self.games {
            Games::Prepared(data) => (data, Default::default()),
            Games::Installed => {
                let root = self.root.clone();
                let worker = std::thread::spawn(move || -> std::io::Result<_> {
                    let data = desktop::prepare(&root, matches!(command,Command::Campaigns | Command::Credits))?;
                    let mut extra = engine_lite::wml::Node::default();
                    if matches!(command, Command::Campaigns) && let Some(userdata) = profile::native_data_directory() {
                        crate::campaign_browser::modifications::load_installed(&mut extra, &root, &userdata)?;
                    }
                    Ok((data, extra))
                });
                while !worker.is_finished() {
                    art.draw_background(screen_width(), screen_height());
                    dialog::text(art.font.as_ref(), "Loading game data…", 24.0, screen_height() - 32.0, 20, WHITE);
                    next_frame().await;
                }
                let (data, extra) = worker.join().map_err(|_| "Game data preparation stopped")??;
                installed = data;
                (&installed, extra)
            }
        };
        match command {
            Command::Campaigns => {
                let (file, maps) = data.campaign.as_ref().ok_or("No campaign supplied")?;
                let browser = if matches!(&self.games, Games::Installed) {
                    Browser::installed(file, maps, data.core.clone(), self.root.clone(), self.seed, modifications)?
                } else {
                    Browser::load(file, maps.clone(), data.core.clone(), self.root.clone(), self.seed)?
                };
                browser.run().await
            }
            Command::Credits => {
                let catalog = data.campaign.as_ref().map(|(file,_)|read_file(file)).transpose()?;
                credits::show(&self.root,&data.core,catalog.as_deref(),art).await
            }
            Command::Multiplayer => Setup::load(self.root.clone(), data.core.clone(), self.seed)?.run().await,
            Command::Help => help::show(&self.root, &data.core, art).await,
            Command::Editor => crate::editor::run(&self.root, &data.core, art.locale.game("Lato")).await,
            _ => unreachable!("only game entries open a child screen"),
        }
    }
}

fn tips(source: &str, locale: &locale::Locale) -> Result<Vec<(String, String)>, Box<dyn Error>> {
    // The final, retired tip in the native file is enclosed in this preprocessor block.
    let active = source.split_once("#ifdef __UNUSED__").map_or(source, |(active, _)| active);
    let translated = |text: &str| locale.game(text).to_owned();
    let entries: Vec<_> = wml::parse(active)?.children.iter().filter(|(tag, _)| tag == "tip")
        .filter_map(|(_, config)| Some((translated(config.attribute("text")?), translated(config.attribute("source")?))))
        .collect();
    if entries.is_empty() { return Err("no title screen tips in data/tips.cfg".into()); }
    Ok(entries)
}


struct ButtonBacking {
    pixels: ::image::RgbaImage,
    sizes: std::cell::RefCell<std::collections::HashMap<(u16, u16), Texture2D>>,
}

impl ButtonBacking {
    fn draw(&self, bounds: Rect, state: button_canvas::State) {
        let width = (bounds.w - 2.).round().max(1.) as u16;
        let height = (bounds.h - 2.).round().max(1.) as u16;
        let mut sizes = self.sizes.borrow_mut();
        let texture = sizes.entry((width, height)).or_insert_with(|| {
            // At exact half-texels the native nearest sampler takes the higher
            // texel. Interpolated GPU coordinates can fall just below that tie.
            let center = |p: u16, src: u32, dst: u16| ((2 * u32::from(p) + 1) * src) / (2 * u32::from(dst));
            let mut bytes = Vec::with_capacity(usize::from(width) * usize::from(height) * 4);
            for y in 0..height {
                let row = center(y, self.pixels.height(), height);
                for x in 0..width {
                    bytes.extend_from_slice(&self.pixels.get_pixel(center(x, self.pixels.width(), width), row).0);
                }
            }
            let tex = Texture2D::from_rgba8(width, height, &bytes);
            tex.set_filter(FilterMode::Nearest);
            tex
        });
        button_canvas::draw(bounds, texture, state);
    }
}

struct Art {
    #[cfg(windows)]
    text_renderer: Option<tip_pango::Text>,
    #[cfg(windows)]
    button_labels: button_labels::Cache,
    blurred_panels: std::cell::RefCell<Option<panel_blur::Panels>>,
    about: version::Footer,
    locale: locale::Locale,
    language_icons: [Texture2D; 3],
    languages: Vec<locale::Language>,
    background: scaled_art::Artwork,
    map: scaled_art::Artwork,
    logo_shadow: Texture2D,
    logo: Texture2D,
    buttons: [ButtonBacking; 3],
    font: Option<Font>,
    italic: Option<Font>,
}

fn image(rect: Rect, texture: &Texture2D) {
    draw_texture_ex(texture, rect.x, rect.y, WHITE, DrawTextureParams {
        dest_size: Some(vec2(rect.w, rect.h)), ..Default::default()
    });
}

fn centered(view: Vec2, size: Vec2) -> Rect {
    Rect::new(((view.x - size.x) * 0.5).round(), ((view.y - size.y) * 0.5).round(), size.x, size.y)
}

fn fit_map(view: Vec2, size: Vec2) -> Rect {
    // title_screen.cfg divides in WFL's three-decimal fixed point, then rounds
    // each scaled image dimension. Its centering expression divides integers.
    let view = view.round().as_ivec2();
    let size = size.as_ivec2();
    let ratios = (view * 1000 + size / 2) / size;
    let scaled = (size * ratios.min_element() + IVec2::splat(500)) / 1000;
    let origin = ((view - scaled) / 2).max(IVec2::ZERO);
    Rect::new(origin.x as f32, origin.y as f32, scaled.x as f32, scaled.y as f32)
}

fn logo_bounds(view_width: f32, size: Vec2) -> Rect {
    Rect::new(((view_width - size.x) / 2.0).floor(), 30.0, size.x, size.y)
}

fn panel(rect: Rect) {
    draw_rectangle(rect.x + 3., rect.y + 3., rect.w - 6., rect.h - 6., Color::from_rgba(0, 0, 0, 137));
    for (inset, thickness, rgba) in [(0., 3., [16,22,35,255]), (1., 1., [114,79,46,255])] {
        let [r,g,b,a] = rgba;
        let color = Color::from_rgba(r,g,b,a);
        for band in panel_rim(rect, inset, thickness) {
            draw_rectangle(band.x, band.y, band.w, band.h, color);
        }
    }
}

// SDL draws each successive GUI2 rectangle inside the preceding one.
fn panel_rim(rect: Rect, inset: f32, thickness: f32) -> [Rect; 4] {
    let x = rect.x + inset;
    let y = rect.y + inset;
    let w = rect.w - 2. * inset;
    let h = rect.h - 2. * inset;
    [
        Rect::new(x, y, w, thickness),
        Rect::new(x, y + h - thickness, w, thickness),
        Rect::new(x, y + thickness, thickness, h - 2. * thickness),
        Rect::new(x + w - thickness, y + thickness, thickness, h - 2. * thickness),
    ]
}

fn load_texture(root: &Path, name: &str) -> Result<Texture2D, Box<dyn Error>> {
    let data = read_file(&root.join(name))?;
    let image = crate::artwork::image(&data).map_err(|cause| format!("{name}: {cause}"))?;
    let texture = Texture2D::from_image(&image);
    texture.set_filter(FilterMode::Linear);
    Ok(texture)
}

fn localized_map(root: &Path, locale: &locale::Locale) -> Result<scaled_art::Artwork, Box<dyn Error>> {
    let (base, overlay) = locale.title_map(root);
    let mut pixels = crate::artwork::rgba(&read_file(&root.join(base))?)?;
    if let Some(overlay) = overlay {
        let names = crate::artwork::rgba(&read_file(&root.join(overlay))?)?;
        ::image::imageops::overlay(&mut pixels, &names, 0, 0);
    }
    Ok(scaled_art::Artwork::new(pixels))
}

fn load_font(root:&Path,name:&str)->Option<Font> {
    fs::read(root.join("fonts").join(name)).ok().and_then(|bytes|load_ttf_font_from_bytes(&bytes).ok())
}

fn language_button_bounds(w: f32, h: f32, text_width: f32) -> Rect {
    let big = w >= 1200.0 && h >= 900.0;
    let width = (text_width.ceil() + if big { 46.0 } else { 43.0 }).max(80.0);
    let height = if big { 32.0 } else { 30.0 };
    Rect::new(w - 10.0 - width, h - 10.0 - height, width, height)
}

fn title_fonts(root:&Path, id:&str)->(Option<Font>,Option<Font>) {
    let name = locale::font_name(id);
    let font = load_font(root,name).or_else(||load_font(root,"DroidSansFallbackFull.ttf"));
    let italic = if name=="Lato-Regular.ttf" { load_font(root,"Lato-Italic.ttf") } else { font.clone() };
    (font,italic)
}

impl Art {
    fn set_locale(&mut self,root:&Path,locale:locale::Locale,logo:Texture2D,map:scaled_art::Artwork) {
        *self.blurred_panels.get_mut() = None;
        self.map = map;
        self.logo = logo;
        if locale::font_name(&self.locale.effective)!=locale::font_name(&locale.effective) {
            (self.font,self.italic)=title_fonts(root,&locale.effective);
        }
        #[cfg(windows)]
        {
            self.text_renderer = tip_pango::Text::load(root, locale.game("Lato"));
            self.button_labels = Default::default();
        }
        self.locale=locale;
    }

    fn command(&self, rect:Rect, cmd:Command, focused:bool, pressed:bool) {
        if cmd == Command::About {
            self.about.draw(self, rect, focused, pressed);
            return;
        }
        self.button_frame(rect, focused, pressed);
        if cmd==Command::Language {
            let icon=&self.language_icons[if pressed {2} else {usize::from(focused)}];
            let size = icon.size().min(rect.size());
            image(Rect::new(rect.x+5.0, rect.y+((rect.h-size.y)*0.5).floor(), size.x, size.y), icon);
        }
        self.draw_command_label(rect, cmd);
    }
    fn load(root: &Path, locale: locale::Locale) -> Result<Self, Box<dyn Error>> {
        let texture = |name: &str| load_texture(root, name);
        let button = |name: &str| -> Result<ButtonBacking, Box<dyn Error>> {
            Ok(ButtonBacking {
                pixels: crate::artwork::rgba(&read_file(&root.join(name))?)?,
                sizes: Default::default(),
            })
        };
        let (font, italic) = title_fonts(root, &locale.effective);
        let logo = texture(&locale.logo(root))?;
        let map = localized_map(root, &locale)?;
        Ok(Self {
            #[cfg(windows)]
            text_renderer: tip_pango::Text::load(root, locale.game("Lato")),
            #[cfg(windows)]
            button_labels: Default::default(),
            blurred_panels: Default::default(),
            about: version::Footer::load(root)?,
            // Partial artwork installations can still show the effective locale identifier.
            languages: locale::list(root).unwrap_or_default(),
            locale, language_icons: [texture("images/icons/action/language_25.png")?,
                texture("images/icons/action/language_25-active.png")?,texture("images/icons/action/language_25-pressed.png")?],
            background: scaled_art::Artwork::new(crate::artwork::rgba(
                &read_file(&root.join("data/core/images/maps/background.webp"))?)?),
            map,
            logo_shadow: texture("images/misc/logo-bg.png")?, logo,
            buttons: [button("images/buttons/button_normal/background.png")?,
                button("images/buttons/button_normal/background-active.png")?,
                button("images/buttons/button_normal/background-pressed.png")?],
            font, italic,
        })
    }

    fn draw_background(&self, w: f32, h: f32) {
        clear_background(BLACK);
        let view = vec2(w, h);
        self.background.draw(Rect::new(0.0, 0.0, w, h), view);
        self.map.draw(fit_map(view, self.map.size()), view);
        for texture in [&self.logo_shadow, &self.logo] {
            image(logo_bounds(w, texture.size()), texture);
        }
    }

    fn draw_panels(&self, w: f32, h: f32, actions: &[Command], body: &str, source: &str, pages: &TipPages) -> Vec<(Command, Rect)> {
        let padding = if w >= 850.0 && h >= 650.0 { 25.0 } else { 10.0 };
        let big = w >= 1200.0 && h >= 900.0;
        let (wide, bh, plain, plain_h, extra) = if big {
            (136.0, 32.0, 128.0, 30.0, 16.0)
        } else {
            (116.0, 30.0, 112.0, 26.0, 13.0)
        };
        let font = self.font.as_ref();
        let italic = self.italic.as_ref().or(font);
        let label_width = |cmd: Command| self.command_label_width(cmd) + extra;
        let bw = actions.iter().copied().map(label_width).fold(wide, f32::max);
        let inset = padding + 5.0;
        let menu_w = bw + inset * 2.0;
        let menu_h = actions.len() as f32 * (bh + 10.0) + 2.0 * padding;
        let tip_text_w = (w - 34.0 - menu_w - 2.0 * inset).clamp(1.0, 650.0).floor();
        let width = tip_text_w as u16;
        let page = pages.page(body, source, width, self);
        let page_height = pages.height(width, self).max(page.height());
        let tip_h = 2.0 * padding + page_height + plain_h + 30.0;
        let tip_w = tip_text_w + inset * 2.0;
        let footer_height = if big { 42.0 } else { 40.0 };
        let panel_grid::Panels { tip, menu } = panel_grid::place(
            vec2(w, h), vec2(tip_w, tip_h), vec2(menu_w, menu_h), footer_height,
        );
        panel_blur::draw(&self.blurred_panels, [tip, menu], vec2(w, h));
        panel(tip);
        panel(menu);
        let controls_y = tip.bottom() - inset - plain_h;
        page.draw(tip.x + inset, tip.y + inset, controls_y - 10.0, font, italic);
        let mut buttons: Vec<_> = actions.iter().enumerate().map(|(i, action)| (*action,
            Rect::new(menu.x + inset, menu.y + inset + i as f32 * (bh + 10.0), bw, bh))).collect();
        let [help_w, prev_w, next_w] = [Command::Help, Command::Previous, Command::Next]
            .map(|cmd| label_width(cmd).max(plain));
        let next = Rect::new(tip.right() - inset - next_w, controls_y, next_w, plain_h);
        buttons.push((Command::Help, Rect::new(tip.x + inset, controls_y, help_w, plain_h)));
        buttons.push((Command::Previous, Rect::new(next.x - 10.0 - prev_w, controls_y, prev_w, plain_h)));
        buttons.push((Command::Next, next));
        buttons.push((Command::About, version::footer_bounds(h)));
        let text_width = self.command_label_width(Command::Language);
        buttons.push((Command::Language, language_button_bounds(w, h, text_width)));
        buttons
    }

    fn button_frame(&self, rect: Rect, focused: bool, pressed: bool) {
        use button_canvas::State;
        let state = if pressed { State::Pressed } else if focused { State::Focused } else { State::Enabled };
        self.buttons[state as usize].draw(rect, state);
    }

    fn button(&self, rect: Rect, label: &str, focused: bool, pressed: bool) {
        self.button_frame(rect,focused,pressed);
        let Rect{x,y,w,h}=rect;
        let font = self.font.as_ref();
        let metrics = measure_text(label, font, 16, 1.0);
        let shift = if pressed { 1.0 } else { 0.0 };
        dialog::text(font, label, (x + (w - metrics.width) * 0.5).round() + shift,
            (y + (h - metrics.height) * 0.5 + metrics.offset_y).round() + shift,
            16, Color::from_rgba(186, 172, 125, 255));
    }
}

pub(crate) fn wrap(text: &str, font: Option<&Font>, width: f32) -> Vec<String> {
    wrap_at(text, font, width, 16)
}

fn wrap_at(text: &str, font: Option<&Font>, width: f32, size: u16) -> Vec<String> {
    let mut lines = Vec::new();
    let space = measure_text(" ", font, size, 1.0).width;
    for paragraph in text.lines() {
        let mut line = String::new();
        let mut used = 0.0;
        for word in paragraph.split_whitespace() {
            let advance = measure_text(word, font, size, 1.0).width;
            if !line.is_empty() && used + space + advance > width {
                lines.push(std::mem::take(&mut line));
                used = 0.0;
            }
            if !line.is_empty() { line.push(' '); used += space; }
            if advance <= width { line.push_str(word); used += advance; }
            else {
                for glyph in word.chars() {
                    let mut utf8 = [0u8;4];
                    let next = measure_text(glyph.encode_utf8(&mut utf8),font,size,1.0).width;
                    if used+next>width && !line.is_empty() {
                        lines.push(std::mem::take(&mut line));
                        used=0.0;
                    }
                    line.push(glyph);
                    used+=next;
                }
            }
        }
        lines.push(line);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Options, Source};

    #[test]
    fn title_keyboard_navigation_can_enter_the_menu_from_an_unhighlighted_map() {
        assert_eq!(keyboard_focus(None, 10, false), 0);
        assert_eq!(keyboard_focus(None, 10, true), 9);
        assert_eq!(keyboard_focus(Some(9), 10, false), 0);
        assert_eq!(keyboard_focus(Some(0), 10, true), 9);
    }

    #[test]
    fn original_title_hotkeys_open_only_available_entries_without_modifiers() {
        let source = [("c", "title_screen__campaign"), ("m", "title_screen__multiplayer"),
            ("a", "title_screen__addons"), ("e", "title_screen__editor"), ("space", "title_screen__credits"),
            ("l", "changelanguage")].map(|(key,id)| format!("[hotkey]\nkey={key}\ncommand={id}\n[/hotkey]")).join("\n");
        let bindings = hotkeys::Bindings::native(&source, PathBuf::new());
        for (key, action) in [(KeyCode::C, Command::Campaigns), (KeyCode::M, Command::Multiplayer),
            (KeyCode::A, Command::Addons), (KeyCode::E, Command::Editor),
            (KeyCode::Space, Command::Credits), (KeyCode::L, Command::Language)] {
            let key = hotkeys::key_name(key).unwrap();
            assert_eq!(bindings.shortcut(&key, menu(true)), Some(action));
            assert_eq!(bindings.shortcut(&format!("Alt+{key}"), menu(true)), None);
        }
        assert_eq!(bindings.shortcut("C", menu(false)), None);
    }

    #[test]
    fn title_routes_only_to_games_with_their_required_inputs() {
        let options = |args: &[&str]| Options::parse(&args.iter().copied().map(std::ffi::OsString::from).collect::<Vec<_>>()).unwrap();
        assert_eq!(options(&["--title", "campaign", "maps", "core", "42"]).source,
            Source::Title(Some(("campaign".into(), "maps".into()))));
        assert_eq!(options(&["--title", "core", "42"]).source, Source::Title(None));
        assert_eq!(menu(true), &[Command::Campaigns, Command::Multiplayer, Command::Load, Command::Addons, Command::Editor, Command::Preferences, Command::Achievements, Command::Community, Command::Credits, Command::Quit]);
        assert_eq!(menu(false), &[Command::Multiplayer, Command::Load, Command::Addons, Command::Editor, Command::Preferences, Command::Achievements, Command::Community, Command::Credits, Command::Quit]);
    }

    #[test]
    fn title_tip_panel_draws_its_gold_edge_one_full_pixel_inside() {
        let panel = Rect::new(73., 467., 710., 258.);
        let dark = panel_rim(panel, 0., 3.);
        let gold = panel_rim(panel, 1., 1.);
        let hits = |bands: &[Rect; 4], x| bands.iter().any(|r| r.contains(vec2(x, 600.5)));
        assert!(!hits(&dark, 72.5));
        assert!(hits(&dark, 73.5) && hits(&dark, 75.5));
        assert!(!hits(&dark, 76.5));
        assert!(hits(&gold, 74.5) && hits(&gold, 781.5));
        assert!(!hits(&gold, 73.5) && !hits(&gold, 75.5));
    }

    #[test]
    fn title_map_keeps_its_aspect_and_centers_on_both_axes() {
        let map = vec2(1000.0, 700.0);
        assert_eq!(fit_map(vec2(1600.0, 700.0), map), Rect::new(300.0, 0.0, 1000.0, 700.0));
        assert_eq!(fit_map(vec2(500.0, 700.0), map), Rect::new(0.0, 175.0, 500.0, 350.0));
    }

    #[test]
    fn title_art_uses_the_native_fixed_point_scale_and_integer_grid_positions() {
        let map = vec2(1280.0, 960.0);
        assert_eq!(fit_map(vec2(1200.0, 820.0), map), Rect::new(53.0, 0.0, 1093.0, 820.0));
        assert_eq!(fit_map(vec2(1280.0, 725.0), map), Rect::new(157.0, 0.0, 966.0, 725.0));
        assert_eq!(logo_bounds(801.0, vec2(600.0, 200.0)), Rect::new(100.0, 30.0, 600.0, 200.0));
    }

    #[test]
    fn title_shows_the_active_native_tips_and_their_attribution() {
        let tip = "[tip]\ntext=_ \"Use <b>Recruit</b>.\"\nsource=_ \"<i>Konrad</i>\"\n[/tip]\n";
        assert_eq!(tips(&format!("{tip}#ifdef __UNUSED__\n{tip}#endif"), &locale::Locale::default()).unwrap(),
            [("Use <b>Recruit</b>.".to_owned(), "<i>Konrad</i>".to_owned())]);
    }

    #[test]
    fn language_footer_has_the_original_logical_bounds_for_the_active_name() {
        assert_eq!(language_button_bounds(1200.0, 820.0, 66.0), Rect::new(1081.0, 780.0, 109.0, 30.0));
        assert_eq!(language_button_bounds(1280.0, 900.0, 150.0), Rect::new(1074.0, 858.0, 196.0, 32.0));
    }
}
