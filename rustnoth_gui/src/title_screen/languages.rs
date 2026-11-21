//! The title screen language picker, matching language_selection.cfg's list and information panel.
use super::{Art, dialog, load_font, load_texture, locale};
mod scrollbar;
mod view;
use crate::list_scroll::Scroll;
use scrollbar::BarArt;
use view::{GOLD, Information, Label, Layout, WHITE_TEXT};
use macroquad::prelude::*;
use std::{collections::HashMap, error::Error, path::Path};

const SHOW_ALL: &str = "Show in-progress or abandoned translations";
const INFORMATION: [&str; 3] = [
    "<i>The Battle for Wesnoth</i> relies on volunteers for development, including translation. Translations may drift behind or even become abandoned.",
    "<b>Note:</b> The translation percentages shown apply to the core game interface and in-game help only. More complete stats, including details for each campaign, are available at the following page:",
    "If you are interested in translating the game yourself, please follow the link below for more information:",
];
const LINKS: [&str; 2] = [
    "https://gettext.wesnoth.org/",
    "https://wiki.wesnoth.org/WesnothTranslations",
];

fn open_link(url: &str) -> std::io::Result<()> {
    #[cfg(windows)]
    let mut launch = {
        use std::os::windows::process::CommandExt;
        let mut command = std::process::Command::new("rundll32.exe");
        command
            .creation_flags(0x08000000)
            .arg("url.dll,FileProtocolHandler");
        command
    };
    #[cfg(target_os = "macos")]
    let mut launch = std::process::Command::new("open");
    #[cfg(not(any(windows, target_os = "macos")))]
    let mut launch = std::process::Command::new("xdg-open");
    launch.arg(url).spawn().map(|_| ())
}

struct Row {
    name: Label,
    percent: Label,
    rtl: bool,
}

impl Row {
    fn height(&self) -> f32 {
        self.name.size.y.max(self.percent.size.y) + 20.
    }

    fn draw(&self, r: Rect, percent_width: f32) {
        let x = if self.rtl {
            (r.right() - 30. - percent_width - self.name.size.x).max(r.x + 10.)
        } else {
            r.x + 10.
        };
        self.name.draw_cell(vec2(x, (r.center().y - self.name.size.y / 2.).floor()),
            r.right() - 30. - percent_width - x);
        self.percent.draw(vec2(r.right() - 10. - self.percent.size.x,
            (r.center().y - self.percent.size.y / 2.).floor()));
    }
}

fn colors(root: &Path) -> Vec<[u8; 3]> {
    let config = std::fs::read_to_string(root.join("data/game_config.cfg")).unwrap_or_default();
    config.lines().filter_map(|line| line.split_once('='))
        .find(|(key, _)| key.trim() == "red_green_scale_text")
        .map(|(_, colors)| colors.trim().trim_matches('"').split(',')
            .filter_map(|s| u32::from_str_radix(s.trim(), 16).ok())
            .map(|rgb| [(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8]).collect())
        .unwrap_or_default()
}

fn make_rows(root: &Path, art: &Art, languages: &[locale::Language]) -> Vec<Row> {
    let palette = colors(root);
    let mut fonts = HashMap::new();
    let font = art.font.as_ref();
    languages.iter().map(|language| {
        let font_name = locale::font_name(&language.id);
        let use_fallback = !language.id.is_empty() && font_name != locale::font_name(&art.locale.effective);
        let row_font = if use_fallback {
            fonts.entry(font_name).or_insert_with(|| load_font(root, font_name)).as_ref()
        } else { font };
        let name = if language.id.is_empty() { art.locale.game(&language.name) } else { &language.name };
        let index = (f64::from(language.percent.min(100)) * palette.len().saturating_sub(1) as f64 / 100.)
            .round_ties_even() as usize;
        let rgb = palette.get(index).copied().unwrap_or(WHITE_TEXT);
        Row {
            name: Label::new(art, row_font, name, 17, WHITE_TEXT),
            percent: Label::new(art, font, &format!("{}%", language.percent), 17, rgb),
            rtl: matches!(language.id.split(['_', '@']).next(), Some("ar" | "he" | "fa")),
        }
    }).collect()
}

pub(super) async fn show(
    root: &Path, art: &Art, behind: impl Fn(f32, f32),
) -> Result<Option<String>, Box<dyn Error>> {
    let languages = locale::list(root)?;
    let original = languages.iter().position(|l| l.id == art.locale.selected).unwrap_or(0);
    let mut selected = original;
    let mut all = languages[selected].percent < 80;
    let mut reveal = true;
    let mut last_click = None;
    let mut scroll = Scroll::default();
    let mut side_scroll = Scroll::default();
    let backdrop = load_texture(root, "images/dialogs/opaque-background.png")?;
    let highlight = load_texture(root, "images/dialogs/selection-background.png")?;
    let checkbox = [
        load_texture(root, "images/buttons/checkbox.png")?,
        load_texture(root, "images/buttons/checkbox-active.png")?,
        load_texture(root, "images/buttons/checkbox-pressed.png")?,
        load_texture(root, "images/buttons/checkbox-active-pressed.png")?,
    ];
    let bar = BarArt::load(root)?;
    let rows = make_rows(root, art, &languages);
    let gold = |text, size| Label::new(art, art.font.as_ref(), art.locale.ui(text), size, GOLD);
    let title = gold("Language", 22);
    let toggle_label = gold(SHOW_ALL, 15);
    let select_label = gold("Select", 16);
    let cancel_label = gold("Cancel", 16);
    let mut information: Option<(Vec2, Information)> = None;

    loop {
        let scale = (screen_width() / 700.).min(screen_height() / 480.).clamp(0.01, 1.);
        let view = vec2(screen_width(), screen_height()) / scale;
        let pointer = Vec2::from(mouse_position()) / scale;
        let click = is_mouse_button_pressed(MouseButton::Left);
        let down = is_mouse_button_down(MouseButton::Left);
        let wheel = mouse_wheel().1;
        let visible: Vec<_> = (0..rows.len()).filter(|&i| all || i == original || languages[i].percent >= 80).collect();
        let mut tops = vec![0.];
        for &i in &visible {
            tops.push(tops.last().unwrap() + rows[i].height());
        }
        let total = *tops.last().unwrap();
        let name_width = visible.iter().map(|&i| rows[i].name.size.x).fold(0., f32::max);
        let percent_width = visible.iter().map(|&i| rows[i].percent.size.x).fold(0., f32::max);
        let layout = Layout::new(view, vec2(name_width, percent_width), total, title.size.y, toggle_label.size.x + 25.);
        let viewport = layout.list;
        let body = Rect::new(viewport.x, viewport.y, viewport.w - if total > viewport.h { 15. } else { 0. }, viewport.h);
        let mut cursor = visible.iter().position(|&i| i == selected).unwrap_or(0);
        let next = if is_key_pressed(KeyCode::Down) { Some((cursor + 1).min(visible.len() - 1)) }
            else if is_key_pressed(KeyCode::Up) { Some(cursor.saturating_sub(1)) }
            else if is_key_pressed(KeyCode::Home) { Some(0) }
            else if is_key_pressed(KeyCode::End) { Some(visible.len() - 1) }
            else if is_key_pressed(KeyCode::PageDown) || is_key_pressed(KeyCode::PageUp) {
                let step = if is_key_pressed(KeyCode::PageDown) { viewport.h } else { -viewport.h };
                let top = tops[cursor] + step;
                Some(tops.partition_point(|&y| y <= top).saturating_sub(1).min(visible.len()-1))
            } else { None };
        if let Some(next) = next { cursor = next; selected = visible[cursor]; }
        if next.is_some() || reveal {
            scroll.reveal(tops[cursor], rows[selected].height(), viewport.h);
            reveal = false;
        }

        set_camera(&dialog::setup_camera(view.x, view.y));
        behind(view.x, view.y);
        view::window(layout.bounds, &backdrop);
        title.draw(vec2(layout.bounds.x + 15., layout.bounds.y + 15.));
        bar.update(&mut scroll, viewport, total, pointer, wheel, scale);
        view::clip(Some(body), scale);
        let mut confirmed = false;
        for (position, &index) in visible.iter().enumerate() {
            let row = &rows[index];
            let rect = Rect::new(body.x, body.y + tops[position] - scroll.offset, body.w, row.height());
            if rect.bottom() <= body.y || rect.y >= body.bottom() { continue; }
            let hover = body.contains(pointer) && rect.contains(pointer);
            if hover && click {
                selected = index;
                let now = get_time();
                confirmed = last_click.is_some_and(|(last, time)| last == index && now - time < 0.4);
                last_click = Some((index, now));
            }
            if selected == index || hover {
                view::highlight(rect, selected == index, &highlight);
            } else {
                view::fill(rect, view::SHADE);
            }
            row.draw(rect, percent_width);
        }
        view::clip(None, scale);

        if information.as_ref().is_none_or(|(size, _)| *size != layout.side.size()) {
            let mut info = Information::new(art, layout.side.w);
            if info.height() > layout.side.h { info = Information::new(art, layout.side.w - 15.); }
            information = Some((layout.side.size(), info));
        }
        let info = &information.as_ref().unwrap().1;
        bar.update(&mut side_scroll, layout.side, info.height(), pointer, wheel, scale);
        let side_content = Rect::new(layout.side.x, layout.side.y, info.width, layout.side.h);
        view::clip(Some(side_content), scale);
        info.draw(side_content, side_scroll.offset, pointer, click)?;
        view::clip(None, scale);
        let toggle = layout.toggle;
        let hover = toggle.contains(pointer);
        draw_texture(&checkbox[2 * usize::from(all) + usize::from(hover)], toggle.x, toggle.y, WHITE);
        toggle_label.draw_cell(vec2(toggle.x + 25., (toggle.center().y - toggle_label.size.y / 2.).floor()), toggle.w - 25.);
        for (button, label) in [(layout.choose, &select_label), (layout.cancel, &cancel_label)] {
            let hover = button.contains(pointer);
            art.button_frame(button, hover, hover && down);
            let pos = (button.center() - label.size / 2.).floor();
            label.draw(pos + vec2(0., -1.));
        }
        set_default_camera();
        if is_key_pressed(KeyCode::Escape) || click && layout.cancel.contains(pointer) { return Ok(None); }
        if is_key_pressed(KeyCode::Enter) || confirmed || click && layout.choose.contains(pointer) {
            return Ok(Some(languages[selected].id.clone()));
        }
        if click && hover || is_key_pressed(KeyCode::Space) {
            all = !all;
            if !all && languages[selected].percent < 80 && selected != original { selected = original; }
            reveal = true;
            last_click = None;
        }
        next_frame().await;
    }
}
