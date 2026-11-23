use engine_lite::scenario_sides::ScenarioObjectives;
use crate::title_screen::{self, locale::Locale};
use super::*;

const TITLE: [u8; 3] = [186, 172, 125];
const BODY: [u8; 3] = [215; 3];

pub fn hotkey() -> bool {
    is_key_pressed(KeyCode::J) && (is_key_down(KeyCode::LeftControl) || is_key_down(KeyCode::RightControl))
}

fn objective_text(objectives: &ScenarioObjectives, locale: &Locale) -> String {
    if objectives.is_empty() { return locale.game("No objectives available").to_owned(); }
    let mut sections = Vec::new();
    if !objectives.summary.is_empty() {
        // objectives.lua enlarges only the first line, preserving markup on later lines.
        let mut lines = objectives.summary.split('\n');
        let mut summary = format!("<big>{}</big>", lines.next().unwrap_or_default());
        for line in lines {
            summary.push('\n');
            summary.push_str(line);
        }
        sections.push(summary);
    }
    for (heading, entries, counters, color) in [
        ("Victory:", &objectives.victory, &objectives.victory_turns, "#00ff00"),
        ("Defeat:", &objectives.defeat, &objectives.defeat_turns, "#ff0000"),
    ] {
        if entries.is_empty() { continue; }
        let mut lines = vec![format!("<big>{}</big>", locale.game(heading))];
        for (index, entry) in entries.iter().enumerate() {
            let counter = counters.get(&index).map(|&count| {
                let translated = locale.game_plural("(this turn left)", "($remaining_turns turns left)", count);
                let value = count.to_string();
                let text = translated.replace("$remaining_turns|", &value).replace("$remaining_turns", &value);
                format!("<span foreground='white'><small> {text}</small></span>")
            }).unwrap_or_default();
            lines.push(format!("<span foreground='{color}'>• {entry}{counter}</span>"));
        }
        sections.push(lines.join("\n"));
    }
    sections.join("\n\n")
}

fn panel(view: Vec2, title: Vec2, body: Vec2) -> Rect {
    // The default message window has 10/13px borders. Each label has a
    // five-pixel margin; the unused image column is invisible.
    let size = vec2(title.x.max(body.x) + 33., title.y + body.y + 43.).min(view);
    let origin = ((view - size) * 0.5).floor();
    Rect::new(origin.x, origin.y, size.x, size.y)
}

pub async fn show(objectives: &ScenarioObjectives, title: &str, locale: &Locale, art: &mut BoardArt, root: &Path) {
    let frame = get_screen_data();
    let board = Texture2D::from_image(&frame);
    board.set_filter(FilterMode::Nearest);
    let tile = get_texture(&mut art.paths, root.join("images/dialogs/translucent65-background.png"));
    let body_text = objective_text(objectives, locale);
    #[cfg(windows)]
    let renderer = title_screen::tip_pango::Text::load(root, "Lato");
    let font = art.font.as_ref();
    let mut opening = true;
    let mut click_armed = false;
    let mut scroll = 0f32;
    let mut previous = Vec2::ZERO;
    let mut labels = None;
    loop {
        let view = vec2(screen_width(), screen_height());
        let scale = (view.x / 350.).min(view.y / 180.).clamp(0.01, 1.);
        let canvas = view / scale;
        if canvas != previous || labels.is_none() {
            let limit = (canvas.x.min(800.) - 33.).max(1.);
            let title = Label::new(title, false, 22, TITLE, limit, font, #[cfg(windows)] renderer.as_ref());
            let body = Label::new(&body_text, true, 17, BODY, limit, font, #[cfg(windows)] renderer.as_ref());
            let bounds = panel(canvas, title.size, body.size);
            let pixels = title_screen::panel_blur::backdrop(&frame, canvas, bounds, 5);
            let blur = Texture2D::from_image(&pixels);
            blur.set_filter(FilterMode::Nearest);
            labels = Some((title, body, bounds, blur));
            previous = canvas;
        }
        let (title, body, r, blur) = labels.as_ref().unwrap();
        set_camera(&crate::dialog::setup_camera(canvas.x, canvas.y));
        draw_texture_ex(&board, 0., 0., WHITE, DrawTextureParams {
            dest_size: Some(canvas), flip_y: true, ..Default::default()
        });
        draw_texture(blur, r.x, r.y, WHITE);
        draw_rectangle_lines(r.x + 0.5, r.y + 0.5, r.w - 1., r.h - 1., 1., Color::from_rgba(16, 22, 35, 255));
        draw_rectangle_lines(r.x + 1.5, r.y + 1.5, r.w - 3., r.h - 3., 1., Color::from_rgba(114, 79, 46, 255));
        let inset = Rect::new(r.x + 2., r.y + 2., r.w - 4., r.h - 4.);
        if let Some(tile) = &tile { title_screen::tiled(inset, tile); }
        else { draw_rectangle(inset.x, inset.y, inset.w, inset.h, Color::from_rgba(0, 0, 0, 166)); }

        let title_pos = r.point() + vec2(15., 15.);
        let body_pos = title_pos + vec2(0., title.size.y + 10.);
        let visible = (r.bottom() - 18. - body_pos.y).max(0.);
        let delta = if is_key_pressed(KeyCode::PageDown) { visible }
            else if is_key_pressed(KeyCode::PageUp) { -visible }
            else if is_key_pressed(KeyCode::Down) { 22. }
            else if is_key_pressed(KeyCode::Up) { -22. }
            else { -mouse_wheel().1 * 22. };
        scroll = (scroll + delta).round().clamp(0., (body.size.y - visible).max(0.));
        title.draw(title_pos, 0., (r.h - 33.).max(0.), font);
        body.draw(body_pos, scroll, visible, font);
        set_default_camera();
        if !opening && is_mouse_button_pressed(MouseButton::Left) { click_armed = true; }
        let dismissed = !opening && (hotkey() || is_key_pressed(KeyCode::Escape)
            || is_key_pressed(KeyCode::Enter) || is_key_pressed(KeyCode::Space)
            || click_armed && is_mouse_button_released(MouseButton::Left));
        opening = false;
        next_frame().await;
        if dismissed { return; }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_transient_wraps_the_scenario_text_inside_a_centered_message() {
        let objectives = ScenarioObjectives { victory: vec!["Hold the bridge".into()], defeat: vec!["Death of Deoran".into()], ..Default::default() };
        let message = objective_text(&objectives, &Locale::default());
        assert_eq!(message, "<big>Victory:</big>\n<span foreground='#00ff00'>• Hold the bridge</span>\n\n<big>Defeat:</big>\n<span foreground='#ff0000'>• Death of Deoran</span>");
        assert_eq!(panel(vec2(1200., 820.), vec2(213., 27.), vec2(270., 130.)), Rect::new(448., 310., 303., 200.));
    }

    #[test]
    fn tutorial_summary_keeps_the_list_size_and_precedes_the_victory_heading() {
        let objectives = ScenarioObjectives {
            summary: "You will learn the basics of:\n• <small>Movement</small>".into(),
            victory: vec!["Destroy a fierce enemy".into()],
            ..Default::default()
        };
        assert_eq!(objective_text(&objectives, &Locale::default()),
            "<big>You will learn the basics of:</big>\n• <small>Movement</small>\n\n<big>Victory:</big>\n<span foreground='#00ff00'>• Destroy a fierce enemy</span>");
    }

    #[test]
    fn limited_objectives_append_the_native_white_countdown_to_the_marked_entry() {
        let mut goals = ScenarioObjectives {
            defeat: vec!["Death of Konrad".into(), "Turns run out".into()],
            defeat_turns: [(1, 2)].into(),
            ..Default::default()
        };
        let text = |suffix: &str| format!("<big>Defeat:</big>\n<span foreground='#ff0000'>• Death of Konrad</span>\n<span foreground='#ff0000'>• Turns run out<span foreground='white'><small> {suffix}</small></span></span>");
        assert_eq!(objective_text(&goals, &Locale::default()), text("(2 turns left)"));
        goals.defeat_turns.insert(1, 1);
        assert_eq!(objective_text(&goals, &Locale::default()), text("(this turn left)"));
    }
}
