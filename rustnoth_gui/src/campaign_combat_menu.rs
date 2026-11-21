use super::{Art, EDGE, LABEL, difficulty, image, outline};
use crate::title_screen::button_canvas::State;
use engine_lite::wml::Node;
use macroquad::prelude::*;
use rustnoth_gui::save_replay::set;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Mode { #[default] Default, Predictable, Biased }

impl Mode {
    const ALL: [Self; 3] = [Self::Default, Self::Predictable, Self::Biased];

    fn label(self) -> &'static str {
        match self {
            Self::Default => "Default RNG",
            Self::Predictable => "Predictable RNG",
            Self::Biased => "Biased RNG (experimental)",
        }
    }

    pub(crate) fn classify(self, selected: &mut Node) {
        let value = match self { Self::Default => "", Self::Predictable => "deterministic", Self::Biased => "biased" };
        set(selected, "random_mode", value);
        set(selected, "rustnoth_interactive_rng", "yes");
    }
}

#[derive(Default)]
pub(super) struct Menu {
    selected: usize,
    cursor: Option<usize>,
    held: bool,
}

impl Menu {
    pub fn is_open(&self) -> bool { self.cursor.is_some() }
    pub fn mode(&self) -> Mode { Mode::ALL[self.selected] }

    pub fn draw(&mut self, art: &mut Art, control: Rect, pointer: Vec2, view: Vec2, available: bool) {
        let label = art.locale.ui("Combat:");
        let width = measure_text(label, art.font.as_ref(), 15, 1.).width;
        art.label(label, control.x - width - 10., control.y + 20., 15, LABEL);
        let hot = control.contains(pointer);
        let pressed = is_mouse_button_pressed(MouseButton::Left);
        let clicked = is_mouse_button_released(MouseButton::Left);
        if pressed && self.cursor.is_none() { self.held = hot && available; }
        let state = if self.is_open() || hot && self.held { State::Pressed } else if hot && available { State::Focused } else { State::Enabled };
        let value = art.locale.ui(self.mode().label()).to_owned();
        difficulty::draw_control(art, control, state, &value);
        if !available { return; }
        let Some(mut cursor) = self.cursor else {
            if hot {
                let wheel = mouse_wheel().1;
                if wheel > 0. { self.selected = self.selected.saturating_sub(1); }
                if wheel < 0. { self.selected = (self.selected + 1).min(2); }
                if clicked && self.held { self.cursor = Some(self.selected); }
            }
            if clicked { self.held = false; }
            return;
        };
        let row = 28.;
        let h = row * 3. + 2.;
        let top = if view.y - control.bottom() >= h { control.bottom() } else { control.y - h };
        let popup = Rect::new(control.x, top, control.w, h);
        art.tile("dialogs/menu-background.png", popup);
        outline(popup, EDGE);
        if is_key_pressed(KeyCode::Up) { cursor = cursor.saturating_sub(1); }
        if is_key_pressed(KeyCode::Down) { cursor = (cursor + 1).min(2); }
        if is_key_pressed(KeyCode::Home) { cursor = 0; }
        if is_key_pressed(KeyCode::End) { cursor = 2; }
        for (index, mode) in Mode::ALL.iter().enumerate() {
            let rect = Rect::new(popup.x+1., popup.y+1.+index as f32*row, popup.w-2., row);
            if cursor == index || rect.contains(pointer) {
                if let Some(texture) = art.get("dialogs/selection-background.png") { image(&texture, rect); }
                if cursor == index { outline(rect, Color::from_rgba(185,158,75,255)); }
            }
            art.label(art.locale.ui(mode.label()), rect.x+7., rect.y+19., 15, LABEL);
            if clicked && rect.contains(pointer) { cursor = index; }
        }
        self.cursor = Some(cursor);
        if clicked || is_key_pressed(KeyCode::Enter) || is_key_pressed(KeyCode::Escape) {
            if is_key_pressed(KeyCode::Enter) || (clicked && popup.contains(pointer)) { self.selected = cursor; }
            self.cursor = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn campaign_modes_write_the_native_classification_to_every_replay_source() {
        for (choice, value) in Mode::ALL.into_iter().zip(["", "deterministic", "biased"]) {
            let mut node = Node::default();
            choice.classify(&mut node);
            assert_eq!(node.attribute("random_mode"), Some(value));
            assert_eq!(node.attribute("rustnoth_interactive_rng"), Some("yes"));
        }
    }
}
