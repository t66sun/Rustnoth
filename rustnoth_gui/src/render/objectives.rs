use engine_lite::scenario_sides::ScenarioObjectives;

use super::*;

pub(super) fn draw_button(art: &BoardArt, bounds: Rect, caption: &str) {
    let (mx, my) = mouse_position();
    let hovered = bounds.contains(vec2(mx, my));
    draw_rectangle(bounds.x, bounds.y, bounds.w, bounds.h, if hovered { BUTTON_HIGHLIGHTED } else { BUTTON });
    draw_rectangle_lines(bounds.x, bounds.y, bounds.w, bounds.h, 1.0, BORDER);
    let text = measure_text(caption, art.font.as_ref(), 17, 1.0);
    label(&art.font, caption, bounds.x + (bounds.w - text.width) * 0.5, bounds.y + bounds.h * 0.5 + 6.0, 17, TEXT);
}

pub fn hotkey() -> bool {
    is_key_pressed(KeyCode::J) && (is_key_down(KeyCode::LeftControl) || is_key_down(KeyCode::RightControl))
}

pub fn button(width: f32) -> Option<Rect> {
    let right = width - sidebar_width(width) - 12.0;
    (right >= 102.0).then_some(Rect::new(right - 100.0, 12.0, 100.0, 44.0))
}

fn wrap_entries(objectives: &ScenarioObjectives, font: Option<&Font>, width: f32) -> Vec<(String, bool)> {
    let mut lines = Vec::new();
    for (heading, entries) in [("Victory", &objectives.victory), ("Defeat", &objectives.defeat)] {
        if entries.is_empty() { continue; }
        lines.push((heading.to_owned(), true));
        for entry in entries {
            for paragraph in entry.lines() {
                let mut line = String::new();
                for word in paragraph.split_whitespace() {
                    let next = if line.is_empty() { word.into() } else { format!("{line} {word}") };
                    if !line.is_empty() && measure_text(&next, font, 18, 1.0).width > width {
                        lines.push((line, false));
                        line = word.to_owned();
                    } else {
                        line = next;
                    }
                }
                if !line.is_empty() { lines.push((line, false)); }
            }
        }
    }
    lines
}

pub async fn show(objectives: &ScenarioObjectives, art: &mut BoardArt, root: &Path) {
    let background = get_texture(&mut art.paths, root.join("images/dialogs/opaque-background.png"));
    let mut first = 0usize;
    let mut wrapped_width = 0.0;
    let mut lines = Vec::new();
    let mut opening_frame = true;
    loop {
        clear_background(BACKGROUND);
        let w = (screen_width() - 16.0).clamp(1.0, 660.0);
        let h = (screen_height() - 16.0).clamp(1.0, 540.0);
        let x = (screen_width() - w) * 0.5;
        let y = (screen_height() - h) * 0.5;
        draw_rectangle(x, y, w, h, PANEL);
        if let Some(texture) = &background {
            draw_texture_ex(texture, x, y, WHITE, DrawTextureParams { dest_size: Some(vec2(w, h)), ..Default::default() });
        }
        draw_rectangle_lines(x, y, w, h, 1.0, BORDER);
        label(&art.font, "Scenario objectives", x + 16.0, y + 36.0, 26, TEXT);

        let line_width = (w - 48.0).max(1.0);
        if wrapped_width != line_width {
            lines = wrap_entries(objectives, art.font.as_ref(), line_width);
            wrapped_width = line_width;
        }
        let visible = (((h - 135.0) / 25.0).max(1.0) as usize).min(lines.len());
        let last = lines.len().saturating_sub(visible);
        let delta = if is_key_pressed(KeyCode::PageDown) { visible as isize }
            else if is_key_pressed(KeyCode::PageUp) { -(visible as isize) }
            else if is_key_pressed(KeyCode::Down) { 1 } else if is_key_pressed(KeyCode::Up) { -1 }
            else if mouse_wheel().1 > 0.0 { -1 } else if mouse_wheel().1 < 0.0 { 1 } else { 0 };
        first = first.saturating_add_signed(delta).min(last);
        for (index, (line, heading)) in lines.iter().skip(first).take(visible).enumerate() {
            let (inset, size, color) = if *heading { (16.0, 21, SELECTED) } else { (28.0, 18, TEXT) };
            header_label(&art.font, line, x + inset, y + 72.0 + index as f32 * 25.0, size, color, line_width);
        }
        let footer = y + h - 56.0;
        let (mx, my) = mouse_position();
        let pointer = vec2(mx, my);
        let clicked = !opening_frame && is_mouse_button_pressed(MouseButton::Left);
        if last > 0 {
            for (offset, caption, destination) in [(0.0, "‹", first.saturating_sub(visible)), (52.0, "›", (first + visible).min(last))] {
                let bounds = Rect::new(x + 16.0 + offset, footer, 46.0, 42.0);
                draw_button(art, bounds, caption);
                if clicked && bounds.contains(pointer) { first = destination; }
            }
        }
        let close = Rect::new(x + w - 96.0, footer, 80.0, 42.0);
        draw_button(art, close, "Close");
        if !opening_frame && (hotkey() || is_key_pressed(KeyCode::Escape) || is_key_pressed(KeyCode::Enter)
            || clicked && close.contains(pointer)) { break; }
        opening_frame = false;
        next_frame().await;
    }
}
