//! The title screen language picker, matching language_selection.cfg's list and information panel.
use super::{Art, centered, dialog, load_font, load_texture, locale, preferences::tiled, wrap};
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

fn plain(text: &str) -> String {
    text.replace("<b>", "")
        .replace("</b>", "")
        .replace("<i>", "")
        .replace("</i>", "")
}

pub(super) async fn show(
    root: &Path,
    art: &Art,
    behind: impl Fn(f32, f32),
) -> Result<Option<String>, Box<dyn Error>> {
    let languages = locale::list(root)?;
    let original = languages
        .iter()
        .position(|l| l.id == art.locale.selected)
        .unwrap_or(0);
    let mut selected = original;
    let mut all = languages[selected].percent < 80;
    let mut first = 0;
    let mut first_frame = true;
    let mut last_click = None;
    let backdrop = load_texture(root, "images/dialogs/opaque-background.png")?;
    let checkbox = [
        load_texture(root, "images/buttons/checkbox.png")?,
        load_texture(root, "images/buttons/checkbox-pressed.png")?,
    ];
    let mut fonts = HashMap::new();
    let font = art.font.as_ref();
    let gold = Color::from_rgba(186, 172, 125, 255);
    let text_color = Color::from_rgba(218, 218, 218, 255);
    loop {
        let scale = (screen_width() / 700.0)
            .min(screen_height() / 480.0)
            .clamp(0.01, 1.0);
        let (w, h) = (screen_width() / scale, screen_height() / scale);
        let (mx, my) = mouse_position();
        let pointer = vec2(mx / scale, my / scale);
        set_camera(&dialog::setup_camera(w, h));
        behind(w, h);
        let bounds = centered(
            vec2(w, h),
            vec2((w - 24.0).min(860.0), (h - 28.0).min(635.0)),
        );
        tiled(bounds, &backdrop);
        draw_rectangle_lines(
            bounds.x,
            bounds.y,
            bounds.w,
            bounds.h,
            2.0,
            Color::from_rgba(114, 79, 46, 255),
        );
        dialog::text(
            font,
            art.locale.ui("Language"),
            bounds.x + 16.0,
            bounds.y + 38.0,
            28,
            gold,
        );
        let list = Rect::new(
            bounds.x + 12.0,
            bounds.y + 52.0,
            bounds.w * 0.49,
            bounds.h - 122.0,
        );
        draw_rectangle(
            list.x,
            list.y,
            list.w,
            list.h,
            Color::from_rgba(10, 16, 24, 255),
        );
        draw_rectangle_lines(
            list.x,
            list.y,
            list.w,
            list.h,
            1.5,
            Color::from_rgba(72, 76, 80, 255),
        );
        let button_y = bounds.bottom() - 47.0;
        let cancel = Rect::new(bounds.right() - 106.0, button_y, 93.0, 32.0);
        let choose = Rect::new(cancel.x - 101.0, button_y, 93.0, 32.0);
        let toggle = Rect::new(bounds.x + 14.0, button_y, choose.x - bounds.x - 22.0, 32.0);
        let rows: Vec<_> = languages
            .iter()
            .enumerate()
            .filter(|(i, l)| all || *i == original || l.percent >= 80)
            .collect();
        let row_h = 36.0;
        let visible = ((list.h - 4.0) / row_h).floor().max(1.0) as usize;
        let mut cursor = rows.iter().position(|(i, _)| *i == selected).unwrap_or(0);
        let step = if is_key_pressed(KeyCode::Down) {
            1
        } else if is_key_pressed(KeyCode::Up) {
            -1
        } else if is_key_pressed(KeyCode::PageDown) {
            visible as isize
        } else if is_key_pressed(KeyCode::PageUp) {
            -(visible as isize)
        } else {
            0
        };
        if step != 0 || first_frame || is_key_pressed(KeyCode::Home) || is_key_pressed(KeyCode::End)
        {
            cursor = cursor.saturating_add_signed(step).min(rows.len() - 1);
            if is_key_pressed(KeyCode::Home) {
                cursor = 0;
            }
            if is_key_pressed(KeyCode::End) {
                cursor = rows.len() - 1;
            }
            selected = rows[cursor].0;
            first = first.min(cursor).max((cursor + 1).saturating_sub(visible));
            first_frame = false;
        }
        let (_, wheel) = mouse_wheel();
        if list.contains(pointer) && wheel != 0.0 {
            first = first.saturating_add_signed(if wheel > 0.0 { -3 } else { 3 });
        }
        first = first.min(rows.len().saturating_sub(visible));
        let click = is_mouse_button_pressed(MouseButton::Left);
        let mut confirmed = false;
        for (pos, &(index, language)) in rows.iter().skip(first).take(visible).enumerate() {
            let row = Rect::new(
                list.x + 2.0,
                list.y + 2.0 + pos as f32 * row_h,
                list.w - 5.0,
                row_h,
            );
            if row.contains(pointer) && click {
                selected = index;
                let now = get_time();
                confirmed =
                    last_click.is_some_and(|(last, time)| last == index && now - time < 0.4);
                last_click = Some((index, now));
            }
            if selected == index || row.contains(pointer) {
                let color = if selected == index {
                    Color::from_rgba(47, 66, 82, 255)
                } else {
                    Color::from_rgba(31, 42, 55, 255)
                };
                draw_rectangle(row.x, row.y, row.w, row.h - 1.0, color);
            }
            let (label, row_font) = if language.id.is_empty() {
                (art.locale.game(&language.name), font)
            } else {
                let name = locale::font_name(&language.id);
                let row_font = if name == locale::font_name(&art.locale.effective) {
                    font
                } else {
                    fonts
                        .entry(name)
                        .or_insert_with(|| load_font(root, name))
                        .as_ref()
                };
                (language.name.as_str(), row_font)
            };
            let label = dialog::fit(row_font, label.into(), row.w - 73.0, 17);
            dialog::text(row_font, &label, row.x + 10.0, row.y + 24.0, 17, text_color);
            let pct = format!("{}%", language.percent);
            let percent = language.percent as f32 / 100.0;
            let color = Color::new(
                (1.5 - percent).clamp(0.0, 1.0),
                (0.35 + percent).min(1.0),
                0.35,
                1.0,
            );
            let width = measure_text(&pct, font, 16, 1.0).width;
            dialog::text(
                font,
                &pct,
                row.right() - 12.0 - width,
                row.y + 24.0,
                16,
                color,
            );
        }
        if rows.len() > visible {
            let step = (list.h - 4.0) / rows.len() as f32;
            draw_rectangle(
                list.right() - 4.0,
                list.y + 2.0 + first as f32 * step,
                3.0,
                step * visible as f32,
                gold,
            );
        }
        let right = list.right() + 17.0;
        let width = bounds.right() - right - 14.0;
        let mut y = list.y + 23.0;
        for (index, msg) in INFORMATION.iter().enumerate() {
            for line in wrap(&plain(art.locale.ui(msg)), font, width) {
                if y + 4.0 > list.bottom() {
                    break;
                }
                dialog::text(font, &line, right, y, 16, text_color);
                y += 20.0;
            }
            if index > 0 && y + 4.0 < list.bottom() {
                let url = LINKS[index - 1];
                let label = dialog::fit(font, url.into(), width, 15);
                let hit = Rect::new(
                    right,
                    y - 14.0,
                    measure_text(&label, font, 15, 1.0).width,
                    19.0,
                );
                dialog::text(
                    font,
                    &label,
                    right,
                    y,
                    15,
                    Color::from_rgba(118, 180, 222, 255),
                );
                if click && hit.contains(pointer) {
                    open_link(url)?;
                }
                y += 20.0;
            }
            y += 13.0;
        }

        let icon = &checkbox[usize::from(all)];
        super::image(Rect::new(toggle.x, toggle.y + 6.0, 20.0, 20.0), icon);
        let label = dialog::fit(font, art.locale.ui(SHOW_ALL).into(), toggle.w - 26.0, 15);
        dialog::text(
            font,
            &label,
            toggle.x + 26.0,
            toggle.y + 21.0,
            15,
            text_color,
        );
        art.button(
            choose,
            art.locale.ui("Select"),
            choose.contains(pointer),
            choose.contains(pointer) && is_mouse_button_down(MouseButton::Left),
        );
        art.button(
            cancel,
            art.locale.ui("Cancel"),
            cancel.contains(pointer),
            cancel.contains(pointer) && is_mouse_button_down(MouseButton::Left),
        );
        set_default_camera();
        if is_key_pressed(KeyCode::Escape) || click && cancel.contains(pointer) {
            return Ok(None);
        }
        if is_key_pressed(KeyCode::Enter) || confirmed || click && choose.contains(pointer) {
            return Ok(Some(languages[selected].id.clone()));
        }
        if click && toggle.contains(pointer) || is_key_pressed(KeyCode::Space) {
            all = !all;
            if !all && languages[selected].percent < 80 && selected != original {
                selected = original;
            }
            first_frame = true;
        }
        next_frame().await;
    }
}
