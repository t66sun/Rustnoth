use std::{fs, path::Path};

use macroquad::prelude::*;

use super::{Field, Setup};
use rustnoth_gui::session::Players;

const BACKDROP: Color = Color::from_rgba(23, 27, 25, 255);
const PANEL: Color = Color::from_rgba(25, 31, 28, 245);
const ROW: Color = Color::from_rgba(43, 52, 45, 255);
const HOVER: Color = Color::from_rgba(61, 74, 60, 255);
const GOLD: Color = Color::from_rgba(235, 200, 130, 255);
const PALE: Color = Color::from_rgba(241, 236, 220, 255);
const MUTED: Color = Color::from_rgba(194, 197, 177, 255);
const WARNING: Color = Color::from_rgba(252, 185, 143, 255);

pub(super) enum Input {
    Choose(Field, usize),
    Start(Players),
    Exit,
    Idle,
}

struct Picker { field: Field, cursor: usize, first: usize }

fn setup_camera(width: f32, height: f32) -> Camera2D {
    Camera2D {
        target: vec2(width * 0.5, height * 0.5),
        zoom: vec2(2.0 / width, 2.0 / height),
        ..Default::default()
    }
}

#[test]
fn setup_camera_places_the_top_left_control_at_the_top_left_of_the_screen() {
    let projection = setup_camera(440.0, 520.0).matrix();
    let top_left = projection.transform_point3(vec3(0.0, 0.0, 0.0));
    let bottom_right = projection.transform_point3(vec3(440.0, 520.0, 0.0));
    assert!(top_left.abs_diff_eq(vec3(-1.0, 1.0, 0.0), 0.00001));
    assert!(bottom_right.abs_diff_eq(vec3(1.0, -1.0, 0.0), 0.00001));
}


pub(super) struct SetupView {
    font: Option<Font>,
    background: Option<Texture2D>,
    focus: usize,
    picker: Option<Picker>,
}

impl SetupView {
    pub(super) fn new(root: &Path) -> Self {
        let font = fs::read(root.join("fonts/Lato-Regular.ttf")).ok()
            .and_then(|bytes| load_ttf_font_from_bytes(&bytes).ok());
        let background = fs::read(root.join("images/dialogs/multi_create/background.png")).ok()
            .and_then(|bytes| Image::from_file_with_format(&bytes, None).ok())
            .map(|image| Texture2D::from_image(&image));
        Self { font, background, focus: 0, picker: None }
    }

    pub(super) fn frame(&mut self, setup: &Setup, error: Option<&str>) -> Input {
        // Keep every field reachable in short windows while preserving the layout aspect ratio.
        let scale = (screen_width() / 440.0).min(screen_height() / 577.0).clamp(0.01, 1.0);
        let w = screen_width() / scale;
        let h = screen_height() / scale;
        let (mx, my) = mouse_position();
        let mouse = vec2(mx / scale, my / scale);
        let clicked = is_mouse_button_pressed(MouseButton::Left);
        let confirm = is_key_pressed(KeyCode::Enter) || is_key_pressed(KeyCode::Space);
        set_camera(&setup_camera(w, h));
        clear_background(BACKDROP);
        if let Some(image) = &self.background {
            let cover = (w / image.width()).max(h / image.height());
            let size = vec2(image.width(), image.height()) * cover;
            draw_texture_ex(image, (w - size.x) * 0.5, (h - size.y) * 0.5, WHITE,
                DrawTextureParams { dest_size: Some(size), ..Default::default() });
        }
        let width = (w - 36.0).min(720.0);
        let height = (h - 28.0).min(636.0);
        let x = (w - width) * 0.5;
        let y = (h - height) * 0.5;
        draw_rectangle(x, y, width, height, PANEL);
        draw_rectangle_lines(x, y, width, height, 2.0, GOLD);
        let left = x + 22.0;
        let inner = width - 44.0;
        let font = self.font.as_ref();
        let mut result = Input::Idle;

        if let Some(picker) = &mut self.picker {
            text(font, &format!("Choose {}", picker.field.title().to_lowercase()), left, y + 53.0, 32, PALE);
            let count = setup.count(picker.field);
            let top = y + 78.0;
            let visible = (((height - 152.0) / 40.0) as usize).min(count).max(1);
            let last_page = count.saturating_sub(visible);
            let wheel = mouse_wheel().1;
            if wheel != 0.0 {
                picker.first = picker.first.saturating_add_signed(if wheel > 0.0 { -3 } else { 3 }).min(last_page);
                picker.cursor = picker.cursor.clamp(picker.first, (picker.first + visible - 1).min(count - 1));
            }
            let step = if is_key_pressed(KeyCode::PageDown) { visible as isize }
                else if is_key_pressed(KeyCode::PageUp) { -(visible as isize) }
                else if is_key_pressed(KeyCode::Down) { 1 } else if is_key_pressed(KeyCode::Up) { -1 } else { 0 };
            if step != 0 { picker.cursor = picker.cursor.saturating_add_signed(step).min(count - 1); }
            if is_key_pressed(KeyCode::Home) { picker.cursor = 0; }
            if is_key_pressed(KeyCode::End) { picker.cursor = count - 1; }
            picker.first = picker.first.clamp(picker.cursor.saturating_sub(visible - 1), picker.cursor).min(last_page);
            if confirm { result = Input::Choose(picker.field, picker.cursor); }
            for (row, index) in (picker.first..count).take(visible).enumerate() {
                let rect = Rect::new(left, top + row as f32 * 40.0, inner, 37.0);
                button(rect, rect.contains(mouse), picker.cursor == index);
                let name = setup.label(picker.field, index);
                let name = fit(font, name, rect.w - 105.0, 18);
                text(font, &name, rect.x + 12.0, rect.y + 25.0, 18, PALE);
                if index == setup.selected(picker.field) {
                    text(font, "Selected", rect.x + rect.w - 82.0, rect.y + 24.0, 15, GOLD);
                }
                if clicked && rect.contains(mouse) { result = Input::Choose(picker.field, index); }
            }
            let foot = y + height - 59.0;
            let back = Rect::new(left, foot, 92.0, 40.0);
            button(back, back.contains(mouse), false);
            text(font, "Back", back.x + 18.0, back.y + 26.0, 17, PALE);
            let mut page = |dx: f32, label: &str, forward: bool| {
                let rect = Rect::new(left + inner - dx, foot, 40.0, 40.0);
                button(rect, rect.contains(mouse), false);
                text(font, label, rect.x + 14.0, rect.y + 27.0, 19, PALE);
                if clicked && rect.contains(mouse) {
                    picker.first = if forward { (picker.first + visible).min(last_page) }
                        else { picker.first.saturating_sub(visible) };
                    picker.cursor = picker.first;
                }
            };
            page(88.0, "‹", false);
            page(40.0, "›", true);
            text(font, &format!("{}–{} / {count}", picker.first + 1, (picker.first + visible).min(count)),
                left + 109.0, foot + 26.0, 15, MUTED);
            if is_key_pressed(KeyCode::Escape) || (clicked && back.contains(mouse)) || matches!(result, Input::Choose(_, _)) {
                self.picker = None;
            }
        } else {
            text(font, "Local skirmish", left, y + 56.0, 36, PALE);
            text(font, "Default era   ·   30 rounds", left, y + 85.0, 17, MUTED);
            let start_index = Field::ALL.len();
            let reverse_tab = is_key_down(KeyCode::LeftShift) || is_key_down(KeyCode::RightShift);
            let move_up = is_key_pressed(KeyCode::Up) || (is_key_pressed(KeyCode::Tab) && reverse_tab);
            let move_down = is_key_pressed(KeyCode::Down) || (is_key_pressed(KeyCode::Tab) && !reverse_tab);
            let focus_count = start_index + 2;
            if move_up { self.focus = (self.focus + focus_count - 1) % focus_count; }
            if move_down { self.focus = (self.focus + 1) % focus_count; }
            for (n, field) in Field::ALL.into_iter().enumerate() {
                let rect = Rect::new(left, y + 105.0 + n as f32 * 57.0, inner, 49.0);
                let enabled = setup.count(field) > 0;
                button(rect, enabled && rect.contains(mouse), self.focus == n);
                text(font, field.title(), rect.x + 13.0, rect.y + 30.0, 16, MUTED);
                if enabled {
                    let index = setup.selected(field);
                    let value = fit(font, setup.label(field, index), rect.w - 135.0, 19);
                    text(font, &value, rect.x + 95.0, rect.y + 31.0, 19, PALE);
                    text(font, "›", rect.x + rect.w - 24.0, rect.y + 31.0, 21, GOLD);
                    if (clicked && rect.contains(mouse)) || (confirm && self.focus == n) {
                        self.focus = n;
                        self.picker = Some(Picker { field, cursor: index, first: index.saturating_sub(3) });
                    }
                }
            }
            let map_error = setup.maps[setup.map].board.as_ref().err().map(String::as_str);
            if let Some(message) = error.or(map_error) {
                wrap(font, message, left, y + 105.0 + Field::ALL.len() as f32 * 57.0 + 27.0, inner);
            }
            let button_width = (inner - 8.0) / 2.0;
            let modes = [(Players::HotSeat, "Play hot seat"), (Players::VersusComputer, "Play vs computer")];
            for (index, (players, title)) in modes.into_iter().enumerate() {
                let rect = Rect::new(left + index as f32 * (button_width + 8.0), y + height - 71.0, button_width, 48.0);
                let focused = self.focus == start_index + index;
                let enabled = !setup.sides.is_empty();
                let start = enabled && ((clicked && rect.contains(mouse)) || (confirm && focused));
                button(rect, enabled && (rect.contains(mouse) || start), focused);
                text(font, if start { "Starting…" } else { title }, rect.x + 14.0, rect.y + 31.0,
                    18, if enabled { GOLD } else { MUTED });
                if start { result = Input::Start(players); }
            }
            if is_key_pressed(KeyCode::Escape) { result = Input::Exit; }
        }
        set_default_camera();
        result
    }
}

fn text(font: Option<&Font>, message: &str, x: f32, y: f32, size: u16, color: Color) {
    draw_text_ex(message, x, y, TextParams { font, font_size: size, color, ..Default::default() });
}

fn fit(font: Option<&Font>, mut label: String, width: f32, size: u16) -> String {
    if measure_text(&label, font, size, 1.0).width > width {
        let available = width - measure_text("…", font, size, 1.0).width;
        while !label.is_empty() && measure_text(&label, font, size, 1.0).width > available {
            label.pop();
        }
        label.push('…');
    }
    label
}

fn button(rect: Rect, hover: bool, focused: bool) {
    draw_rectangle(rect.x, rect.y, rect.w, rect.h, if hover { HOVER } else { ROW });
    if focused { draw_rectangle_lines(rect.x, rect.y, rect.w, rect.h, 2.0, GOLD); }
}

fn wrap(font: Option<&Font>, message: &str, x: f32, mut y: f32, width: f32) {
    let mut line = String::new();
    for word in message.split_whitespace() {
        let mut next = line.clone();
        if !next.is_empty() { next.push(' '); }
        next.push_str(word);
        if !line.is_empty() && measure_text(&next, font, 15, 1.0).width > width {
            text(font, &line, x, y, 15, WARNING);
            y += 18.0;
            line = word.to_string();
        } else { line = next; }
    }
    text(font, &line, x, y, 15, WARNING);
}
