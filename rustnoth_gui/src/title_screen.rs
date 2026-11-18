//! Native Wesnoth 1.18.8 title artwork and local game entry points.
use std::{error::Error, fs, path::{Path, PathBuf}};

use engine_lite::wml;
use macroquad::prelude::*;

mod preferences;
mod help;
pub(crate) mod locale;
mod languages;
mod credits;
mod community;
mod achievements;

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Command { Campaigns, Multiplayer, Load, Editor, Preferences, Achievements, Community, Credits, Quit, Help, Previous, Next, Language }

impl Command {
    fn label(self) -> &'static str {
        match self {
            Self::Campaigns => "Campaigns",
            Self::Multiplayer => "Multiplayer",
            Self::Load => "Load",
            Self::Editor => "Map Editor",
            Self::Preferences => "Preferences",
            Self::Achievements => "Achievements",
            Self::Credits => "Credits",
            Self::Community => "Community",
            Self::Quit => "Quit",
            Self::Language => "Language",
            Self::Help => "Help",
            Self::Previous => "Previous",
            Self::Next => "Next",
        }
    }
}

fn menu(campaign: bool) -> &'static [Command] {
    if campaign { &[Command::Campaigns, Command::Multiplayer, Command::Load, Command::Editor, Command::Preferences, Command::Achievements, Command::Community, Command::Credits, Command::Quit] }
    else { &[Command::Multiplayer, Command::Load, Command::Editor, Command::Preferences, Command::Achievements, Command::Community, Command::Credits, Command::Quit] }
}

impl TitleScreen {
    pub(super) async fn run(self, mut fullscreen: bool) -> Result<(), Box<dyn Error>> {
        let selected = crate::display_preferences::config_directory().and_then(|dir| locale::saved(&dir)).unwrap_or_default();
        let mut art = Art::load(&self.root, locale::Locale::load(&self.root, selected)?)?;
        let tip_source = fs::read_to_string(self.root.join("data/tips.cfg"))?;
        let mut entries = tips(&tip_source, &art.locale)?;
        let actions = menu(match &self.games {
            Games::Prepared(data) => data.campaign.is_some(),
            Games::Installed => desktop::campaign_available(&self.root),
        });
        let mut tip = self.seed as usize % entries.len();
        let mut focus = 0;
        let mut pressed = None;
        let mut last_pointer = mouse_position();
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
            let (tip_body, attribution) = &entries[tip];
            let buttons = art.draw_panels(w, h, actions, error.as_deref().unwrap_or(tip_body),
                if error.is_none() { attribution } else { "" });
            let hovered = buttons.iter().position(|(_, rect)| rect.contains(pointer));
            if last_pointer != (mx, my) {
                if let Some(index) = hovered { focus = index; }
                last_pointer = (mx, my);
            }
            let backwards = is_key_pressed(KeyCode::Up) || (is_key_pressed(KeyCode::Tab)
                && (is_key_down(KeyCode::LeftShift) || is_key_down(KeyCode::RightShift)));
            if backwards { focus = (focus + buttons.len() - 1) % buttons.len(); }
            else if is_key_pressed(KeyCode::Tab) || is_key_pressed(KeyCode::Down) {
                focus = (focus + 1) % buttons.len();
            }
            if is_mouse_button_pressed(MouseButton::Left) { pressed = hovered; }
            let click = if is_mouse_button_released(MouseButton::Left) {
                pressed.take().filter(|index| Some(*index) == hovered)
            } else { None };
            for (index, (command, rect)) in buttons.iter().enumerate() {
                art.command(*rect, *command, index == focus, pressed == Some(index) && hovered == pressed);
            }
            dialog::text(art.font.as_ref(), "Rustnoth  ·  Wesnoth 1.18.8", 10.0, h - 11.0, 14, WHITE);
            set_default_camera();

            let command = if is_key_pressed(KeyCode::Escape) { Some(Command::Quit) }
                else if is_key_pressed(KeyCode::F1) { Some(Command::Help) }
                else if is_key_pressed(KeyCode::Left) { Some(Command::Previous) }
                else if is_key_pressed(KeyCode::Right) { Some(Command::Next) }
                else if is_key_pressed(KeyCode::Enter) || is_key_pressed(KeyCode::Space) {
                    Some(buttons[focus].0)
                } else { click.map(|index| buttons[index].0) };
            match command {
                Some(Command::Quit) => return Ok(()),
                Some(Command::Previous) => { error = None; tip = (tip + entries.len() - 1) % entries.len(); }
                Some(Command::Next) => { error = None; tip = (tip + 1) % entries.len(); }
                Some(Command::Community) => {
                    next_frame().await;
                    community::show(&art).await;
                }
                Some(Command::Language) => {
                    next_frame().await;
                    let result = languages::show(&self.root, &art, |w,h| {
                        art.draw_background(w,h);
                        let (body, source) = &entries[tip];
                        for (command, rect) in art.draw_panels(w,h,actions,body,source) {
                            art.command(rect,command,command==Command::Language,false);
                        }
                    }).await;
                    match result {
                        Ok(Some(id)) => {
                            let change = (|| -> Result<(),Box<dyn Error>> {
                                let locale = locale::Locale::load(&self.root,id.clone())?;
                                let translated = tips(&tip_source, &locale)?;
                                let logo = load_texture(&self.root, &locale.logo(&self.root))?;
                                locale::save(&crate::display_preferences::config_directory()?, &id)?;
                                art.set_locale(&self.root,locale,logo);
                                entries = translated;
                                Ok(())
                            })();
                            error = change.err().map(|e|e.to_string());
                        }
                        Ok(None) => {}
                        Err(e) => error = Some(e.to_string()),
                    }
                }
                Some(Command::Load) => {
                    next_frame().await;
                    error = crate::save_dialog::open(&self.root).await.err().map(|cause| cause.to_string());
                }
                Some(Command::Achievements) => {
                    next_frame().await;
                    error = achievements::show(&self.root, &art, |w, h| {
                        art.draw_background(w, h);
                        let (body, source) = &entries[tip];
                        for (cmd, rect) in art.draw_panels(w, h, actions, body, source) {
                            art.command(rect, cmd, cmd == Command::Achievements, false);
                        }
                    }).await.err().map(|cause| cause.to_string());
                }
                Some(Command::Preferences) => {
                    next_frame().await;
                    let result = preferences::show(&self.root, &art, &mut fullscreen, &mut windowed_size, |w, h| {
                        art.draw_background(w, h);
                        let (body, source) = &entries[tip];
                        for (command, bounds) in art.draw_panels(w, h, actions, body, source) {
                            art.command(bounds, command, command == Command::Preferences, false);
                        }
                        dialog::text(art.font.as_ref(), "Rustnoth  ·  Wesnoth 1.18.8", 10.0, h - 11.0, 14, WHITE);
                    }).await;
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
        let data = match &self.games {
            Games::Prepared(data) => data,
            Games::Installed => {
                let root = self.root.clone();
                let worker = std::thread::spawn(move || desktop::prepare(&root, matches!(command,Command::Campaigns | Command::Credits)));
                while !worker.is_finished() {
                    art.draw_background(screen_width(), screen_height());
                    dialog::text(art.font.as_ref(), "Loading game data…", 24.0, screen_height() - 32.0, 20, WHITE);
                    next_frame().await;
                }
                installed = worker.join().map_err(|_| "Game data preparation stopped")??;
                &installed
            }
        };
        match command {
            Command::Campaigns => {
                let (file, maps) = data.campaign.as_ref().ok_or("No campaign supplied")?;
                let browser = if matches!(&self.games, Games::Installed) {
                    Browser::installed(file, maps, data.core.clone(), self.root.clone(), self.seed)?
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
            Command::Editor => crate::editor::run(&self.root, &data.core).await,
            _ => unreachable!("only game entries open a child screen"),
        }
    }
}

fn tips(source: &str, locale: &locale::Locale) -> Result<Vec<(String, String)>, Box<dyn Error>> {
    // The final, retired tip in the native file is enclosed in this preprocessor block.
    let active = source.split_once("#ifdef __UNUSED__").map_or(source, |(active, _)| active);
    let plain = |text: &str| locale.game(text).replace("<i>", "").replace("</i>", "").replace("<b>", "").replace("</b>", "");
    let entries: Vec<_> = wml::parse(active)?.children.iter().filter(|(tag, _)| tag == "tip")
        .filter_map(|(_, config)| Some((plain(config.attribute("text")?), plain(config.attribute("source")?))))
        .collect();
    if entries.is_empty() { return Err("no title screen tips in data/tips.cfg".into()); }
    Ok(entries)
}

struct Art {
    locale: locale::Locale,
    language_icons: [Texture2D; 3],
    background: Texture2D,
    map: Texture2D,
    logo_shadow: Texture2D,
    logo: Texture2D,
    buttons: [Texture2D; 3],
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
    centered(view, (size * (view.x / size.x).min(view.y / size.y)).round())
}

fn panel(rect: Rect) {
    draw_rectangle(rect.x, rect.y, rect.w, rect.h, Color::from_rgba(0, 0, 0, 137));
    draw_rectangle_lines(rect.x, rect.y, rect.w, rect.h, 3.0, Color::from_rgba(16, 22, 35, 255));
    draw_rectangle_lines(rect.x + 1.0, rect.y + 1.0, rect.w - 2.0, rect.h - 2.0, 1.0,
        Color::from_rgba(114, 79, 46, 255));
}

fn load_texture(root: &Path, name: &str) -> Result<Texture2D, Box<dyn Error>> {
    let data = read_file(&root.join(name))?;
    let image = Image::from_file_with_format(&data, None).map_err(|cause| format!("{name}: {cause}"))?;
    let texture = Texture2D::from_image(&image);
    texture.set_filter(FilterMode::Linear);
    Ok(texture)
}

fn load_font(root:&Path,name:&str)->Option<Font> {
    fs::read(root.join("fonts").join(name)).ok().and_then(|bytes|load_ttf_font_from_bytes(&bytes).ok())
}

fn title_fonts(root:&Path, id:&str)->(Option<Font>,Option<Font>) {
    let name = locale::font_name(id);
    let font = load_font(root,name).or_else(||load_font(root,"DroidSansFallbackFull.ttf"));
    let italic = if name=="Lato-Regular.ttf" { load_font(root,"Lato-Italic.ttf") } else { font.clone() };
    (font,italic)
}

impl Art {
    fn set_locale(&mut self,root:&Path,locale:locale::Locale,logo:Texture2D) {
        self.logo = logo;
        if locale::font_name(&self.locale.effective)!=locale::font_name(&locale.effective) {
            (self.font,self.italic)=title_fonts(root,&locale.effective);
        }
        self.locale=locale;
    }

    fn command(&self, rect:Rect, cmd:Command, focused:bool, pressed:bool) {
        let text=self.locale.ui(cmd.label());
        if cmd==Command::Language {
            self.button_frame(rect, focused, pressed);
            let icon=&self.language_icons[if pressed {2} else {usize::from(focused)}];
            image(Rect::new(rect.x+5.0,rect.y+(rect.h-20.0)/2.0,20.0,20.0),icon);
            dialog::text(self.font.as_ref(),text,rect.x+30.0,rect.y+rect.h/2.0+5.0,14,Color::from_rgba(186,172,125,255));
        } else { self.button(rect,text,focused,pressed); }
    }
    fn load(root: &Path, locale: locale::Locale) -> Result<Self, Box<dyn Error>> {
        let texture = |name: &str| load_texture(root, name);
        let (font, italic) = title_fonts(root, &locale.effective);
        let logo = texture(&locale.logo(root))?;
        Ok(Self {
            locale, language_icons: [texture("images/icons/action/language_25.png")?,
                texture("images/icons/action/language_25-active.png")?,texture("images/icons/action/language_25-pressed.png")?],
            background: texture("data/core/images/maps/background.webp")?,
            map: texture("data/core/images/maps/titlescreen.webp")?,
            logo_shadow: texture("images/misc/logo-bg.png")?, logo,
            buttons: [texture("images/buttons/button_normal/background.png")?,
                texture("images/buttons/button_normal/background-active.png")?,
                texture("images/buttons/button_normal/background-pressed.png")?],
            font, italic,
        })
    }

    fn draw_background(&self, w: f32, h: f32) {
        clear_background(BLACK);
        let view = vec2(w, h);
        image(Rect::new(0.0, 0.0, w, h), &self.background);
        image(fit_map(view, self.map.size()), &self.map);
        let scale = (w / self.logo.width()).min((h * 0.42) / self.logo.height()).min(1.0);
        for texture in [&self.logo_shadow, &self.logo] {
            let mut rect = centered(view, (texture.size() * scale).round());
            rect.y = 30.0;
            image(rect, texture);
        }
    }

    fn draw_panels(&self, w: f32, h: f32, actions: &[Command], body: &str, source: &str) -> Vec<(Command, Rect)> {
        let padding = if w >= 850.0 && h >= 650.0 { 25.0 } else { 10.0 };
        let (bw, bh) = if w >= 1200.0 && h >= 900.0 { (136.0, 32.0) } else { (116.0, 30.0) };
        let inset = padding + 5.0;
        let menu_w = bw + inset * 2.0;
        let menu_h = actions.len() as f32 * (bh + 10.0) + 2.0 * padding;
        let tip_text_w = (w - 54.0 - menu_w - 2.0 * inset).min(650.0);
        let font = self.font.as_ref();
        let italic = self.italic.as_ref().or(font);
        let body_lines = wrap(body, font, tip_text_w);
        let source_lines = wrap(source, italic, tip_text_w);
        let tip_h = 2.0 * inset + (body_lines.len() + source_lines.len()) as f32 * 20.0 + 46.0;
        let tip_w = tip_text_w + inset * 2.0;
        let spacer = ((w - tip_w - menu_w - 34.0) / 5.0).max(0.0);
        let row_h = menu_h.max(tip_h);
        let bottom = row_h + ((h - 35.0 - 20.0 - row_h).max(0.0) * 5.0 / 6.0).round() + 20.0;
        let tip = Rect::new((17.0 + spacer).round(), bottom - tip_h, tip_w, tip_h);
        let menu = Rect::new((w - 17.0 - spacer - menu_w).round(), bottom - menu_h, menu_w, menu_h);
        panel(tip);
        panel(menu);
        let text_color = Color::from_rgba(215, 215, 215, 255);
        let mut y = tip.y + inset + 15.0;
        for line in body_lines {
            dialog::text(font, &line, tip.x + inset, y, 16, text_color);
            y += 20.0;
        }
        y += 5.0;
        for line in source_lines {
            let x = tip.right() - inset - measure_text(&line, italic, 16, 1.0).width;
            dialog::text(italic, &line, x, y, 16, text_color);
            y += 20.0;
        }
        let mut buttons: Vec<_> = actions.iter().enumerate().map(|(i, action)| (*action,
            Rect::new(menu.x + inset, menu.y + inset + i as f32 * (bh + 10.0), bw, bh))).collect();
        let next = Rect::new(tip.right() - inset - 62.0, bottom - inset - 26.0, 62.0, 26.0);
        buttons.push((Command::Help, Rect::new(tip.x + inset, next.y, 62.0, next.h)));
        buttons.push((Command::Previous, Rect::new(next.x - 86.0, next.y, 76.0, next.h)));
        buttons.push((Command::Next, next));
        let language_w = (measure_text(self.locale.ui("Language"), font, 14, 1.0).width + 40.0).max(80.0);
        buttons.push((Command::Language,Rect::new(w-10.0-language_w,h-35.0,language_w,25.0)));
        buttons
    }

    fn button_frame(&self, rect: Rect, focused: bool, pressed: bool) {
        let Rect { x, y, w, h } = rect;
        let state = if pressed { 2 } else { usize::from(focused) };
        let dark = Color::from_rgba(1, 10, 16, 255);
        let light_gold = Color::from_rgba(162, 127, 68, 255);
        let dark_gold = Color::from_rgba(114, 79, 46, 255);
        let highlight = [Color::from_rgba(21, 79, 109, 255), Color::from_rgba(12, 108, 157, 255), dark][state];
        draw_rectangle(x, y + 1.0, w - 1.0, h - 1.0, dark);
        image(Rect::new(x + 2.0, y + 2.0, w - 4.0, h - 4.0), &self.buttons[state]);
        draw_rectangle(x + 2.0, y, w - 3.0, 1.0, light_gold);
        draw_rectangle(x + w - 1.0, y + 1.0, 1.0, h - 3.0, light_gold);
        draw_rectangle(x + 1.0, y + 1.0, 1.0, h - 3.0, dark_gold);
        draw_rectangle(x + 2.0, y + h - 2.0, w - 3.0, 1.0, dark_gold);
        draw_rectangle(x + 3.0, y + 2.0, w - 5.0, 1.0, highlight);
        draw_rectangle(x + w - 3.0, y + 2.0, 1.0, h - 5.0, highlight);
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
    let mut lines = Vec::new();
    let space = measure_text(" ", font, 16, 1.0).width;
    for paragraph in text.lines() {
        let mut line = String::new();
        let mut used = 0.0;
        for word in paragraph.split_whitespace() {
            let advance = measure_text(word, font, 16, 1.0).width;
            if !line.is_empty() && used + space + advance > width {
                lines.push(std::mem::take(&mut line));
                used = 0.0;
            }
            if !line.is_empty() { line.push(' '); used += space; }
            if advance <= width { line.push_str(word); used += advance; }
            else {
                for glyph in word.chars() {
                    let mut utf8 = [0u8;4];
                    let next = measure_text(glyph.encode_utf8(&mut utf8),font,16,1.0).width;
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
    fn title_routes_only_to_games_with_their_required_inputs() {
        let options = |args: &[&str]| Options::parse(&args.iter().copied().map(std::ffi::OsString::from).collect::<Vec<_>>()).unwrap();
        assert_eq!(options(&["--title", "campaign", "maps", "core", "42"]).source,
            Source::Title(Some(("campaign".into(), "maps".into()))));
        assert_eq!(options(&["--title", "core", "42"]).source, Source::Title(None));
        assert_eq!(menu(true), &[Command::Campaigns, Command::Multiplayer, Command::Load, Command::Editor, Command::Preferences, Command::Achievements, Command::Community, Command::Credits, Command::Quit]);
        assert_eq!(menu(false), &[Command::Multiplayer, Command::Load, Command::Editor, Command::Preferences, Command::Achievements, Command::Community, Command::Credits, Command::Quit]);
    }

    #[test]
    fn title_map_keeps_its_aspect_and_centers_on_both_axes() {
        let map = vec2(1000.0, 700.0);
        assert_eq!(fit_map(vec2(1600.0, 700.0), map), Rect::new(300.0, 0.0, 1000.0, 700.0));
        assert_eq!(fit_map(vec2(500.0, 700.0), map), Rect::new(0.0, 175.0, 500.0, 350.0));
    }

    #[test]
    fn title_shows_the_active_native_tips_and_their_attribution() {
        let tip = "[tip]\ntext=_ \"Use <b>Recruit</b>.\"\nsource=_ \"<i>Konrad</i>\"\n[/tip]\n";
        assert_eq!(tips(&format!("{tip}#ifdef __UNUSED__\n{tip}#endif"), &locale::Locale::default()).unwrap(),
            [("Use Recruit.".to_owned(), "Konrad".to_owned())]);
    }
}
