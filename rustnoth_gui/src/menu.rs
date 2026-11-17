use engine_lite::{fixed_match::Action, hex::Hex};
use macroquad::prelude::{Rect, Vec2};
use rustnoth_gui::choices::ActionOption;

const ROW_HEIGHT: f32 = 44.0;
const ROW_STEP: f32 = 49.0;

pub struct Menu {
    pub target: Hex,
    options: Vec<ActionOption>,
    focused: usize,
}

pub struct Row<'a> {
    pub bounds: Rect,
    pub action: &'a Action,
    pub label: &'a str,
    pub focused: bool,
}

impl Menu {
    pub fn new(target: Hex, options: Vec<ActionOption>) -> Option<Self> {
        (!options.is_empty()).then_some(Self { target, options, focused: 0 })
    }

    pub fn title(&self) -> &str {
        match &self.options[0].action {
            Action::Attack(_) => "Attack",
            Action::Recruit { .. } => "Recruit",
            _ => unreachable!("context choices contain only attack and recruit actions"),
        }
    }

    pub fn cycle(&mut self) {
        self.focused = (self.focused + 1) % self.options.len();
    }

    pub fn focused_action(&self) -> &Action {
        &self.options[self.focused].action
    }

    pub fn action_at(&self, pointer: Vec2, area: Rect) -> Option<&Action> {
        self.rows(area).find(|row| row.bounds.contains(pointer)).map(|row| row.action)
    }

    pub fn rows(&self, area: Rect) -> impl Iterator<Item = Row<'_>> {
        let visible = ((area.h + ROW_STEP - ROW_HEIGHT) / ROW_STEP).floor() as usize;
        let first = (self.focused + 1).saturating_sub(visible);
        self.options
            .iter()
            .enumerate()
            .skip(first)
            .take(visible)
            .map(move |(index, option)| Row {
                bounds: Rect::new(area.x, area.y + (index - first) as f32 * ROW_STEP, area.w, ROW_HEIGHT),
                action: &option.action,
                label: &option.label,
                focused: index == self.focused,
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use macroquad::prelude::vec2;

    #[test]
    fn the_pointed_and_keyboard_focused_rows_return_their_matching_actions() {
        let target = Hex { x: 2, y: 1 };
        let recruit = |kind: &str| ActionOption {
            label: kind.to_owned(),
            action: Action::Recruit { unit_type: kind.to_owned(), to: target },
        };
        assert!(Menu::new(target, Vec::new()).is_none());
        let options = [recruit("Bowman"), recruit("Cavalryman"), recruit("Fencer")];
        let first = options[0].action.clone();
        let second = options[1].action.clone();
        let third = options[2].action.clone();
        let area = Rect::new(10.0, 20.0, 130.0, 98.0);
        let mut menu = Menu::new(target, options.into()).unwrap();
        assert_eq!(menu.focused_action(), &first);
        assert_eq!(menu.action_at(vec2(25.0, 70.0), area), Some(&second));
        assert_eq!(menu.action_at(vec2(25.0, 66.0), area), None);
        menu.cycle();
        menu.cycle();
        assert_eq!(menu.focused_action(), &third);
        assert_eq!(menu.action_at(vec2(25.0, 70.0), area), Some(&third));
        menu.cycle();
        assert_eq!(menu.focused_action(), &first);
    }
}
