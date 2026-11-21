use super::{Art, AssetCatalog, Command, EditableMap, History};
use super::clipboard::SelectionState;
use macroquad::prelude::*;

#[derive(Default)]
pub(super) struct Toolbar {
    pressed: Option<Command>,
}

struct Button {
    command: Command,
    image: &'static str,
    area: Rect,
    enabled: bool,
}

fn buttons(width: f32, doc: &EditableMap, history: &History) -> impl Iterator<Item = Button> {
    use Command::*;
    // The 800-wide theme removes Close. The undo group follows a ten-pixel separator.
    let compact = width < 1024.0;
    let undo_x = if compact { 106.0 } else { 137.0 };
    [
        (Open, "editor-map-load", 2.0, true),
        (Save, "editor-map-save", 33.0, doc.dirty()),
        (Close, "editor-close-map", 64.0, true),
        (Undo, "undo", undo_x, !history.undo.is_empty()),
        (Redo, "redo", undo_x + 31.0, !history.redo.is_empty()),
        (ToggleGrid, "togglegrid", width - 214.0, true),
    ]
    .into_iter()
    .filter(move |(cmd, ..)| *cmd != Close || !compact)
    .map(|(command, image, x, enabled)| Button {
        command, image, area: Rect::new(x, 31.0, 30.0, 30.0), enabled,
    })
}

impl Toolbar {
    fn activate(&mut self, hover: Option<Command>, down: bool, up: bool) -> Option<Command> {
        if down {
            self.pressed = hover;
        }
        if up && self.pressed.take() == hover { hover } else { None }
    }

    pub(super) fn draw(
        &mut self, width: f32, point: Vec2, modal: bool, show_grid: bool, doc: &EditableMap, history: &History,
        assets: &AssetCatalog, art: &mut Art, tip: &mut Option<&'static str>, selection: SelectionState,
    ) -> Option<Command> {
        let controls = || buttons(width, doc, history).chain(selection_buttons(width, selection));
        let hover = controls()
            .find(|button| button.area.contains(point) && button.enabled && !modal)
            .map(|button| button.command);
        let action = self.activate(hover, is_mouse_button_pressed(MouseButton::Left),
            is_mouse_button_released(MouseButton::Left));
        for button in controls() {
            let hot = hover == Some(button.command);
            let selected = button.command == Command::ToggleGrid && show_grid || selection.selected(button.command);
            let suffix = if selected || hot && self.pressed == hover { "-pressed" }
                else if hot { "-active" } else { "" };
            let frame_state = if hot && selected { "-active-pressed" } else { suffix };
            let effect = if button.enabled { "" } else { "~GS()" };
            let frame = format!("buttons/button_square/button_square_30{frame_state}.png{effect}");
            let icon = format!("icons/action/{}_30{suffix}.png{effect}", button.image);
            art.image(assets, &frame, button.area);
            art.image(assets, &icon, button.area);
            if !modal && button.area.contains(point) {
                *tip = Some(button.command.label());
            }
        }
        action
    }
}

fn selection_buttons(width: f32, state: SelectionState) -> impl Iterator<Item=Button> {
    use Command::*;
    let first = if width < 1024. { 179. } else { 210. };
    [
        (SelectAll, "editor-select-all", 0.),
        (SelectNone, "editor-select-none", 31.),
        (SelectInverse, "editor-select-inverse", 62.),
        (FillSelection, "editor-selection-fill", 93.),
        (Shuffle, "editor-selection-randomize", 124.),
        (Cut, "editor-cut", 166.),
        (Copy, "editor-copy", 197.),
        (Paste, "editor-paste", 228.),
    ].into_iter().map(move |(command,image,offset)| Button {
        command, image, area: Rect::new(first + offset,31.,30.,30.), enabled: state.enabled(command),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_lite::hex::Hex;

    #[test]
    fn classic_file_and_history_actions_use_the_native_slots_and_document_state() {
        let mut doc = super::super::new_map();
        let mut history = History::default();
        let slots = |width, doc: &EditableMap, history: &History|
            buttons(width, doc, history).map(|b| (b.command, b.area.x as i32, b.enabled)).collect::<Vec<_>>();
        assert_eq!(slots(1200., &doc, &history), [
            (Command::Open, 2, true), (Command::Save, 33, false), (Command::Close, 64, true),
            (Command::Undo, 137, false), (Command::Redo, 168, false), (Command::ToggleGrid, 986, true),
        ]);
        history.paint(&mut doc, Hex { x: 0, y: 0 }, "Ww");
        history.finish();
        assert_eq!(slots(800., &doc, &history), [
            (Command::Open, 2, true), (Command::Save, 33, true),
            (Command::Undo, 106, true), (Command::Redo, 137, false), (Command::ToggleGrid, 586, true),
        ]);
        history.apply(&mut doc, true);
        assert!(buttons(1200., &doc, &history).any(|b| b.command == Command::Redo && b.enabled));
        let mut toolbar = Toolbar::default();
        assert_eq!(toolbar.activate(Some(Command::Open), true, false), None);
        assert_eq!(toolbar.activate(Some(Command::Close), false, true), None);
        toolbar.activate(Some(Command::Close), true, false);
        assert_eq!(toolbar.activate(Some(Command::Close), false, true), Some(Command::Close));
    }
}
