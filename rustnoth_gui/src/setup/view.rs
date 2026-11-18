use std::path::Path;
use crate::dialog::{Dialog, ListAction, ListPosition, button, fit, text, wrap, GOLD, MUTED, PALE};

use macroquad::prelude::*;

use super::{Field, Setup};
use rustnoth_gui::session::Players;

pub(super) enum Input {
    Choose(Field, usize),
    Start(Players),
    Exit,
    Idle,
}

pub(super) struct SetupView {
    dialog: Dialog,
    focus: usize,
    picker: Option<(Field, ListPosition)>,
}

impl SetupView {
    pub(super) fn new(root: &Path) -> Self {
        Self { dialog: Dialog::new(root), focus: 0, picker: None }
    }

    pub(super) fn frame(&mut self, setup: &Setup, error: Option<&str>) -> Input {
        let frame = self.dialog.begin();
        let Rect { x, y, w: width, h: height } = frame.panel;
        let (mouse, clicked, confirm) = (frame.mouse, frame.clicked, frame.confirm);
        let left = x + 22.0;
        let inner = width - 44.0;
        let font = self.dialog.font.as_ref();
        let mut result = Input::Idle;

        if let Some((field, position)) = &mut self.picker {
            let field = *field;
            let action = self.dialog.choose(&frame, position, setup.count(field), Some(setup.selected(field)),
                &format!("Choose {}", field.title().to_lowercase()), None, |index| (setup.label(field, index), None));
            match action {
                ListAction::Choose(index) => { result = Input::Choose(field, index); self.picker = None; }
                ListAction::Back => self.picker = None,
                ListAction::Idle => {}
            }
        } else {
            text(font, "Local skirmish", left, y + 56.0, 36, PALE);
            text(font, "Default era", left, y + 85.0, 17, MUTED);
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
                        self.picker = Some((field, ListPosition { cursor: index, first: index.saturating_sub(3) }));
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
