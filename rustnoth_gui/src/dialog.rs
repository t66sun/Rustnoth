//! Shared artwork, panel layout and scrollable choices for desktop launch menus.
use std::{fs, path::Path};
use macroquad::prelude::*;

const BACKDROP: Color = Color::from_rgba(23, 27, 25, 255);
const PANEL: Color = Color::from_rgba(25, 31, 28, 245);
const ROW: Color = Color::from_rgba(43, 52, 45, 255);
const HOVER: Color = Color::from_rgba(61, 74, 60, 255);
pub const GOLD: Color = Color::from_rgba(235, 200, 130, 255);
pub const PALE: Color = Color::from_rgba(241, 236, 220, 255);
pub const MUTED: Color = Color::from_rgba(194, 197, 177, 255);
pub const WARNING: Color = Color::from_rgba(252, 185, 143, 255);

pub(super) fn setup_camera(width: f32, height: f32) -> Camera2D {
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

pub struct Dialog {
    pub font: Option<Font>,
    background: Option<Texture2D>,
}

pub struct Frame {
    pub panel: Rect,
    pub mouse: Vec2,
    pub clicked: bool,
    pub confirm: bool,
}

pub enum ListAction { Idle, Choose(usize), Back }

#[derive(Default)]
pub struct ListPosition {
    pub cursor: usize,
    pub first: usize,
}

impl Dialog {
    pub fn new(root: &Path) -> Self {
        let font = fs::read(root.join("fonts/Lato-Regular.ttf")).ok()
            .and_then(|bytes| load_ttf_font_from_bytes(&bytes).ok());
        let background = fs::read(root.join("images/dialogs/multi_create/background.png")).ok()
            .and_then(|bytes| Image::from_file_with_format(&bytes, None).ok())
            .map(|image| Texture2D::from_image(&image));
        Self { font, background }
    }

    pub fn begin(&self, form_height: f32) -> Frame {
        // Keep every field reachable in short windows while preserving the layout aspect ratio.
        let scale = (screen_width() / 440.0).min(screen_height() / (form_height + 28.0)).clamp(0.01, 1.0);
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
        let height = (h - 28.0).min(form_height);
        let x = (w - width) * 0.5;
        let y = (h - height) * 0.5;
        draw_rectangle(x, y, width, height, PANEL);
        draw_rectangle_lines(x, y, width, height, 2.0, GOLD);
        Frame { panel: Rect::new(x, y, width, height), mouse, clicked, confirm }
    }

    /// Draw a nonempty list. A second label line is used for campaign scenario IDs.
    pub fn choose(&self, frame: &Frame, picker: &mut ListPosition, count: usize, selected: Option<usize>,
        heading: &str, error: Option<&str>, label: impl Fn(usize) -> (String, Option<String>)) -> ListAction {
        let Frame { panel, mouse, clicked, confirm } = *frame;
        let font = self.font.as_ref();
        let left = panel.x + 22.0;
        let inner = panel.w - 44.0;
        text(font, heading, left, panel.y + 53.0, 32, PALE);
        let top = panel.y + if let Some(message) = error {
            let message = fit(font, message.into(), inner, 16);
            text(font, &message, left, panel.y + 83.0, 16, WARNING);
            99.0
        } else { 78.0 };
        let foot = panel.y + panel.h - 59.0;
        let step_height = if selected.is_some() { 40.0 } else { 54.0 };
        let visible = (((foot - 15.0 - top) / step_height) as usize).min(count).max(1);
        let mut result = ListAction::Idle;
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
        if confirm { result = ListAction::Choose(picker.cursor); }
        for (row, index) in (picker.first..count).take(visible).enumerate() {
            let rect = Rect::new(left, top + row as f32 * step_height, inner, step_height - 3.0);
            button(rect, rect.contains(mouse), picker.cursor == index);
            let (name, subtitle) = label(index);
            let available = rect.w - if selected.is_some() { 105.0 } else { 26.0 };
            text(font, &fit(font, name, available, 18), rect.x + 12.0, rect.y + 24.0, 18, PALE);
            if let Some(id) = subtitle {
                text(font, &fit(font, id, available, 14), rect.x + 12.0, rect.y + 43.0, 14, MUTED);
            }
            if Some(index) == selected {
                text(font, "Selected", rect.x + rect.w - 82.0, rect.y + 24.0, 15, GOLD);
            }
            if clicked && rect.contains(mouse) { result = ListAction::Choose(index); }
        }
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
        if is_key_pressed(KeyCode::Escape) || (clicked && back.contains(mouse)) { ListAction::Back } else { result }
    }
}

pub fn text(font: Option<&Font>, message: &str, x: f32, y: f32, size: u16, color: Color) {
    draw_text_ex(message, x, y, TextParams { font, font_size: size, color, ..Default::default() });
}

pub fn fit(font: Option<&Font>, mut label: String, width: f32, size: u16) -> String {
    if measure_text(&label, font, size, 1.0).width > width {
        let available = width - measure_text("…", font, size, 1.0).width;
        while !label.is_empty() && measure_text(&label, font, size, 1.0).width > available {
            label.pop();
        }
        label.push('…');
    }
    label
}

pub fn button(rect: Rect, hover: bool, focused: bool) {
    draw_rectangle(rect.x, rect.y, rect.w, rect.h, if hover { HOVER } else { ROW });
    if focused { draw_rectangle_lines(rect.x, rect.y, rect.w, rect.h, 2.0, GOLD); }
}

pub fn wrap(font: Option<&Font>, message: &str, x: f32, mut y: f32, width: f32) {
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
