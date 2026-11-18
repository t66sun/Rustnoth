use engine_lite::{fixed_match::Action, hex::Hex};
use macroquad::prelude::{Rect, Vec2};
use rustnoth_gui::choices::ActionOption;

const ROW_HEIGHT: f32 = 44.0;
const ROW_STEP: f32 = 49.0;

pub struct Menu {
    pub target: Hex,
    options: Vec<ActionOption>,
    focused: usize,
    advancements: Vec<String>,
    advancement: Option<usize>,
}

pub struct Row<'a> {
    pub bounds: Rect,
    pub action: &'a Action,
    pub label: &'a str,
    pub focused: bool,
}

impl Menu {
    pub fn new(target: Hex, options: Vec<ActionOption>, choices: impl FnOnce(Hex) -> Vec<String>) -> Option<Self> {
        let advancements = match &options.first()?.action {
            Action::Attack(attack) => choices(attack.from),
            _ => Vec::new(),
        };
        Some(Self { target, options, focused: 0, advancements, advancement: None })
    }

    pub fn preferred_advancement(&self) -> Option<&str> {
        self.advancement.map(|index| self.advancements[index].as_str())
    }

    pub fn cycle_advancement(&mut self) {
        if self.advancements.len() > 1 {
            let next = self.advancement.map_or(0, |index| index + 1);
            self.advancement = (next < self.advancements.len()).then_some(next);
        }
    }

    pub fn advancement_button(&self, area: Rect) -> Option<Rect> {
        (self.advancements.len() > 1 && area.h >= ROW_HEIGHT)
            .then_some(Rect::new(area.x, area.y, area.w, ROW_HEIGHT))
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

    pub fn rows(&self, mut area: Rect) -> impl Iterator<Item = Row<'_>> {
        if self.advancement_button(area).is_some() {
            area.y += ROW_STEP;
            area.h -= ROW_STEP;
        }
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
    fn the_attack_menu_can_choose_its_own_promotion_and_keeps_the_default_for_the_defender() {
        use engine_lite::{board::AttackChoice, scenario_bundle::Bundle};
        use rustnoth_gui::session::Session;

        let bundle = Bundle::parse(br#"
            [units]
                [unit_type]
                    id=Fighter
                    level=1
                    hitpoints=20
                    experience=5
                    advances_to=Veteran,Missing,Champion
                    [attack]
                        range=melee
                        damage=1
                        number=1
                    [/attack]
                [/unit_type]
                [unit_type]
                    id=Veteran
                    hitpoints=25
                [/unit_type]
                [unit_type]
                    id=Champion
                    hitpoints=30
                [/unit_type]
            [/units]
            [scenario]
                map_data="Xv,Xv,Xv,Xv
Xv,1 Gg,2 Gg,Xv
Xv,Xv,Xv,Xv"
                [side]
                    side=1
                    type=Fighter
                    experience=4
                [/side]
                [side]
                    side=2
                    type=Fighter
                    experience=4
                [/side]
            [/scenario]
        "#).unwrap();
        let from = Hex { x: 0, y: 0 };
        let to = Hex { x: 1, y: 0 };
        let action = Action::Attack(AttackChoice { from, to, weapon: 0 });
        for (cycles, expected) in [(0, "Veteran"), (2, "Champion"), (3, "Veteran")] {
            let mut session = Session::new(bundle.start(5489, |_| unreachable!()).unwrap());
            assert_eq!(session.advancement_choices(from), ["Veteran", "Champion"]);
            assert!(session.advancement_choices(to).is_empty());
            let options = vec![ActionOption { action: action.clone(), label: "Sword".into() }];
            let mut menu = Menu::new(to, options, |position| session.advancement_choices(position)).unwrap();
            let area = Rect::new(10.0, 20.0, 190.0, 150.0);
            let button = menu.advancement_button(area).unwrap();
            assert!(button.contains(vec2(20.0, 35.0)));
            assert!(menu.action_at(vec2(20.0, 35.0), area).is_none());
            assert_eq!(menu.action_at(vec2(20.0, 85.0), area), Some(&action));
            for _ in 0..cycles {
                menu.cycle_advancement();
            }
            session.apply_with_advancement(menu.focused_action().clone(), menu.preferred_advancement()).unwrap();
            let view = session.observation().unwrap();
            let type_at = |hex| view.units.iter().find(|unit| unit.position == hex).unwrap().type_id.as_str();
            assert_eq!(type_at(from), expected);
            assert_eq!(type_at(to), "Veteran");
        }
    }

    #[test]
    fn the_pointed_and_keyboard_focused_rows_return_their_matching_actions() {
        let target = Hex { x: 2, y: 1 };
        let recruit = |kind: &str| ActionOption {
            label: kind.to_owned(),
            action: Action::Recruit { unit_type: kind.to_owned(), to: target },
        };
        assert!(Menu::new(target, Vec::new(), |_| unreachable!()).is_none());
        let options = [recruit("Bowman"), recruit("Cavalryman"), recruit("Fencer")];
        let first = options[0].action.clone();
        let second = options[1].action.clone();
        let third = options[2].action.clone();
        let area = Rect::new(10.0, 20.0, 130.0, 98.0);
        let mut menu = Menu::new(target, options.into(), |_| unreachable!()).unwrap();
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
