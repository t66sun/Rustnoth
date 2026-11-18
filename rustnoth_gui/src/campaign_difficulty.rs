use super::{Art, EDGE, GOLD, LABEL, contain, fill, image, outline, plain};
use crate::dialog::setup_camera;
use engine_lite::wml::Node;
use macroquad::prelude::*;

struct Choice<'a> {
    define: &'a str,
    label: String,
    description: String,
    image: Option<&'a str>,
}

fn options<'a>(cfg: &'a Node, locale: &mut super::Locale, root: &std::path::Path) -> (Vec<Choice<'a>>, usize) {
    let mut choices = Vec::new();
    let mut cursor = 0;
    for (_, item) in cfg.children.iter().filter(|(tag, _)| tag == "difficulty") {
        let Some(define) = item.attribute("define") else {
            continue;
        };
        let label = plain(&locale.attribute(root, item, "label", define));
        let desc = plain(&locale.attribute(root, item, "description", ""));
        let description = if desc.is_empty()
            || matches!(item.attribute("auto_markup"), Some("no" | "false"))
            || matches!(item.attribute("old_markup"), Some("yes" | "true"))
        {
            desc
        } else {
            format!("({desc})")
        };
        if matches!(item.attribute("default"), Some("yes" | "true" | "1")) {
            cursor = choices.len();
        }
        choices.push(Choice {
            define,
            label,
            description,
            image: item.attribute("image"),
        });
    }
    (choices, cursor)
}

pub(super) async fn choose(art: &mut Art, cfg: &Node) -> Option<String> {
    let (choices, mut cursor) = options(cfg, &mut art.locale, &art.roots[0]);
    if choices.len() <= 1 {
        return Some(choices.first().map_or("NORMAL", |c| c.define).to_owned());
    }
    let back = Texture2D::from_image(&get_screen_data());
    next_frame().await;
    let mut first = 0usize;
    let mut last_click = None;
    loop {
        let scale = (screen_width() / 620.0)
            .min(screen_height() / 420.0)
            .clamp(0.01, 1.0);
        let (w, h) = (screen_width() / scale, screen_height() / scale);
        let (mx, my) = mouse_position();
        let pointer = vec2(mx / scale, my / scale);
        set_camera(&setup_camera(w, h));
        draw_texture_ex(&back, 0.0, 0.0, WHITE, DrawTextureParams {
            dest_size: Some(vec2(w, h)),
            flip_y: true,
            ..Default::default()
        });
        fill(Rect::new(0.0, 0.0, w, h), Color::from_rgba(0, 0, 0, 80));
        let row_height = 78.0;
        let visible = (((h - 136.0) / row_height).floor() as usize)
            .max(1)
            .min(choices.len());
        let height = visible as f32 * row_height + 128.0;
        let width = (w - 32.0).min(580.0);
        let bounds = Rect::new((w - width) * 0.5, (h - height) * 0.5, width, height);
        art.window(bounds);
        art.label(art.locale.ui("Difficulty"), bounds.x + 13.0, bounds.y + 31.0, 25, GOLD);
        art.label(
            art.locale.ui("Select difficulty:"),
            bounds.x + 13.0,
            bounds.y + 58.0,
            17,
            LABEL,
        );
        let list = Rect::new(
            bounds.x + 12.0,
            bounds.y + 67.0,
            width - 24.0,
            visible as f32 * row_height,
        );
        fill(list, Color::from_rgba(13, 19, 29, 236));
        outline(list, EDGE);
        let wheel = mouse_wheel().1;
        if is_key_pressed(KeyCode::Down) || wheel < 0.0 && list.contains(pointer) {
            cursor = (cursor + 1).min(choices.len() - 1);
        }
        if is_key_pressed(KeyCode::Up) || wheel > 0.0 && list.contains(pointer) {
            cursor = cursor.saturating_sub(1);
        }
        if is_key_pressed(KeyCode::Home) {
            cursor = 0;
        }
        if is_key_pressed(KeyCode::End) {
            cursor = choices.len() - 1;
        }
        first = first
            .clamp(cursor.saturating_sub(visible - 1), cursor)
            .min(choices.len() - visible);
        let click = is_mouse_button_pressed(MouseButton::Left);
        let mut accepted = is_key_pressed(KeyCode::Enter);
        for (row, choice) in choices.iter().enumerate().skip(first).take(visible) {
            let r = Rect::new(
                list.x + 2.0,
                list.y + (row - first) as f32 * row_height + 1.0,
                list.w - 4.0,
                row_height - 2.0,
            );
            if click && r.contains(pointer) {
                let now = get_time();
                accepted |= last_click.is_some_and(|(i, t)| i == row && now - t <= 0.4);
                last_click = Some((row, now));
                cursor = row;
            }
            if row == cursor || r.contains(pointer) {
                if let Some(bg) = art.get("dialogs/selection-background-fancy.png") {
                    image(&bg, r);
                }
                if row == cursor {
                    outline(r, EDGE);
                }
            }
            if let Some(unit) = choice.image.and_then(|s| art.get(s)) {
                contain(&unit, Rect::new(r.x + 5.0, r.y + 2.0, 72.0, 72.0), false);
            }
            art.label(&choice.label, r.x + 88.0, r.y + 32.0, 19, LABEL);
            art.label(&choice.description, r.x + 88.0, r.y + 55.0, 16, GRAY);
        }
        let cancel = Rect::new(bounds.right() - 118.0, bounds.bottom() - 46.0, 103.0, 33.0);
        let play = Rect::new(cancel.x - 111.0, cancel.y, 103.0, 33.0);
        art.button(play, "game^Play", pointer);
        art.button(cancel, "Cancel", pointer);
        set_default_camera();
        if is_key_pressed(KeyCode::Escape) || click && cancel.contains(pointer) {
            next_frame().await;
            return None;
        }
        if accepted || click && play.contains(pointer) {
            next_frame().await;
            return Some(choices[cursor].define.to_owned());
        }
        next_frame().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_labels_order_and_default_survive_selection_without_rewriting_the_macro() {
        let cfg=engine_lite::wml::parse("[difficulty]\ndefine=BEGINNER\nlabel=Scout\ndescription=Easy\nimage=units/scout.png\n[/difficulty]\n[difficulty]\ndefine=HARD\ndefault=yes\nlabel=Captain\ndescription=Veteran\nold_markup=yes\n[/difficulty]").unwrap();
        let (list, index) = options(&cfg, &mut super::super::Locale::default(), std::path::Path::new(""));
        assert_eq!(
            (
                list[0].label.as_str(),
                list[0].description.as_str(),
                list[0].image
            ),
            ("Scout", "(Easy)", Some("units/scout.png"))
        );
        assert_eq!(
            (index, list[index].define, list[index].description.as_str()),
            (1, "HARD", "Veteran")
        );
    }
}
