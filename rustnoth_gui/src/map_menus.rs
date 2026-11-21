//! Top-level Menu and Actions buttons from the default battlefield theme.
use engine_lite::{fixed_match::Action, hex::Hex, observation::Observation};
use macroquad::prelude::*;
use std::path::Path;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Group {
    Menu,
    Actions,
    Context,
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Command {
    Objectives,
    UnitList,
    Save,
    Quit,
    NextUnit,
    Recruit,
    Recall,
    Undo,
}
impl Command {
    fn label(self) -> &'static str {
        match self {
            Self::UnitList => "Unit List",
            Self::Objectives => "Scenario Objectives",
            Self::Save => "Save Game",
            Self::Quit => "Quit to Main Menu",
            Self::NextUnit => "Next Unit",
            Self::Recruit => "Recruit",
            Self::Recall => "Recall",
            Self::Undo => "Undo",
        }
    }
}
#[derive(Default)]
pub(super) struct State {
    open: Option<Group>,
    cursor: usize,
    context: Vec2,
}
pub(super) struct Response {
    pub handled: bool,
    pub command: Option<Command>,
}

pub(super) struct Art {
    buttons: [[Option<Texture2D>; 3]; 2],
}
impl Art {
    pub fn new(root: &Path) -> Self {
        let folder = root.join("images/buttons/button_menu");
        let buttons = ["menu_button_copper_H20", "menu_button_small_copper_H20"].map(|base| {
            ["", "-active", "-active-pressed"].map(|suffix| {
                crate::render::load_texture(&folder.join(format!("{base}{suffix}.png")), None)
            })
        });
        Self { buttons }
    }
}

fn geometry(w: f32, h: f32) -> ([Rect; 2], f32, bool) {
    let s = (w / 800.0).min(h / 600.0).clamp(0.0, 1.0);
    let compact = w < 1024.0 || h < 768.0;
    let width = if compact { 55.0 * s } else { 100.0 * s };
    (
        [
            Rect::new(0.0, s, width, 22.0 * s),
            Rect::new(width, s, width, 22.0 * s),
        ],
        s,
        compact,
    )
}

pub(super) fn destination(cmd: Command, actions: &[Action], near: Option<Hex>) -> Option<Hex> {
    actions
        .iter()
        .filter_map(|action| match (cmd, action) {
            (Command::Recruit, Action::Recruit { to, .. })
            | (Command::Recall, Action::Recall { to, .. }) => Some(*to),
            _ => None,
        })
        .min_by_key(|hex| (near.map_or(0, |near| near.distance(*hex)), hex.x, hex.y))
}

pub(super) fn next_unit(view: &Observation, selected: Option<Hex>) -> Option<Hex> {
    let mut units = view
        .units
        .iter()
        .filter(|u| u.side == view.side && u.moves_left.unwrap_or(0) > 0)
        .map(|u| u.position);
    let first = units.next()?;
    let Some(current) = selected else {
        return Some(first);
    };
    let mut past = first == current;
    for hex in units {
        if past {
            return Some(hex);
        }
        past = hex == current;
    }
    Some(first)
}

pub(super) fn available(objectives: bool, units: bool, actions: &[Action]) -> Vec<Command> {
    let mut commands = Vec::new();
    if objectives {
        commands.push(Command::Objectives)
    }
    commands.push(Command::Quit);
    if units {
        commands.push(Command::NextUnit)
    }
    for c in [Command::Recruit, Command::Recall] {
        if destination(c, actions, None).is_some() {
            commands.push(c)
        }
    }
    commands
}
fn items(group: Group, commands: &[Command]) -> Vec<Command> {
    commands
        .iter()
        .copied()
        .filter(|c| match group {
            Group::Menu => matches!(c, Command::Objectives | Command::UnitList | Command::Save | Command::Quit),
            Group::Actions => matches!(c, Command::NextUnit | Command::Recruit | Command::Recall | Command::Undo),
            Group::Context => *c == Command::Undo,
        })
        .collect()
}
fn row(button: Rect, i: usize, scale: f32) -> Rect {
    Rect::new(
        button.x,
        button.bottom() + i as f32 * 27.0 * scale,
        192.0 * scale,
        27.0 * scale,
    )
}

impl State {
    fn anchor(&self, group: Group, buttons: [Rect; 2]) -> Rect {
        match group {
            Group::Menu => buttons[0],
            Group::Actions => buttons[1],
            Group::Context => Rect::new(self.context.x, self.context.y, 0.0, 0.0),
        }
    }

    pub fn input(&mut self, commands: &[Command]) -> Response {
        let (bounds, s, _) = geometry(screen_width(), screen_height());
        let pointer = Vec2::from(mouse_position());
        let group_at = |p| {
            bounds
                .iter()
                .position(|r| r.contains(p))
                .map(|i| if i == 0 { Group::Menu } else { Group::Actions })
        };
        let click = is_mouse_button_pressed(MouseButton::Left);
        let prior = self.open;
        let mut command = None;
        if prior.is_none() && commands.contains(&Command::Undo) {
            if is_mouse_button_pressed(MouseButton::Right) && crate::render::board_viewport().contains(pointer.to_array()) {
                self.open = Some(Group::Context);
                self.cursor = 0;
                self.context = vec2(pointer.x.min(screen_width() - 192.0 * s), pointer.y.min(screen_height() - 27.0 * s));
                return Response { handled: true, command: None };
            }
            if is_key_pressed(KeyCode::U) && ![
                KeyCode::LeftAlt, KeyCode::RightAlt, KeyCode::LeftControl, KeyCode::RightControl,
                KeyCode::LeftShift, KeyCode::RightShift, KeyCode::LeftSuper, KeyCode::RightSuper,
            ].into_iter().any(is_key_down) {
                return Response { handled: true, command: Some(Command::Undo) };
            }
        }
        if click && let Some(group) = group_at(pointer) {
            self.open =
                (prior != Some(group) && !items(group, commands).is_empty()).then_some(group);
            self.cursor = 0;
            return Response {
                handled: true,
                command: None,
            };
        }
        if let Some(group) = prior {
            let options = items(group, commands);
            let button = self.anchor(group, bounds);
            if options.is_empty()
                || is_key_pressed(KeyCode::Escape)
                || is_mouse_button_pressed(MouseButton::Right)
            {
                self.open = None;
            } else if click {
                command = options
                    .iter()
                    .enumerate()
                    .find(|(i, _)| row(button, *i, s).contains(pointer))
                    .map(|(_, c)| *c);
                self.open = None;
            } else if is_key_pressed(KeyCode::Enter) {
                command = options.get(self.cursor).copied();
                self.open = None;
            } else if is_key_pressed(KeyCode::Down) {
                self.cursor = (self.cursor + 1) % options.len();
            } else if is_key_pressed(KeyCode::Up) {
                self.cursor = (self.cursor + options.len() - 1) % options.len();
            } else if group != Group::Context && (is_key_pressed(KeyCode::Right) || is_key_pressed(KeyCode::Left)) {
                let other = if group == Group::Menu {
                    Group::Actions
                } else {
                    Group::Menu
                };
                if !items(other, commands).is_empty() {
                    self.open = Some(other);
                    self.cursor = 0;
                }
            }
        }
        Response {
            handled: prior.is_some(),
            command,
        }
    }

    pub fn draw(&self, art: &Art, font: Option<&Font>, commands: &[Command]) {
        let (bounds, s, compact) = geometry(screen_width(), screen_height());
        if s <= 0.0 {
            return;
        }
        let pointer = Vec2::from(mouse_position());
        let text = |caption: &str, r: Rect, centered: bool, color: Color| {
            let size = (15.0 * s).round().max(1.0) as u16;
            let metrics = measure_text(caption, font, size, 1.0);
            let x = if centered {
                r.center().x - metrics.width / 2.0
            } else {
                r.x + 10.0 * s
            };
            draw_text_ex(
                caption,
                x.round(),
                (r.center().y + metrics.offset_y - metrics.height / 2.0).round(),
                TextParams {
                    font,
                    font_size: size,
                    color,
                    ..Default::default()
                },
            );
        };
        for (i, (group, name)) in [(Group::Menu, "Menu"), (Group::Actions, "Actions")]
            .into_iter()
            .enumerate()
        {
            let r = bounds[i];
            let state = if self.open == Some(group) {
                2
            } else {
                usize::from(r.contains(pointer))
            };
            if let Some(tex) = &art.buttons[usize::from(compact)][state] {
                draw_texture_ex(
                    tex,
                    r.x,
                    r.y,
                    WHITE,
                    DrawTextureParams {
                        dest_size: Some(r.size()),
                        ..Default::default()
                    },
                );
            }
            text(name, r, true, Color::from_rgba(239, 224, 184, 255));
        }
        if let Some(group) = self.open {
            let r = self.anchor(group, bounds);
            for (j, cmd) in items(group, commands).iter().enumerate() {
                let r = row(r, j, s);
                let focus = r.contains(pointer) || self.cursor == j;
                let color = if focus {
                    Color::from_rgba(52, 75, 92, 255)
                } else {
                    Color::from_rgba(30, 31, 32, 252)
                };
                draw_rectangle(r.x, r.y, r.w, r.h, color);
                draw_rectangle_lines(r.x, r.y, r.w, r.h, 1.0, Color::from_rgba(100, 82, 52, 255));
                text(cmd.label(), r, false, WHITE);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tutorial_context_menu_offers_only_the_available_undo() {
        let commands = [Command::Quit, Command::NextUnit, Command::Undo];
        assert_eq!(items(Group::Context, &commands), [Command::Undo]);
        assert!(items(Group::Context, &commands[..2]).is_empty());
        let state = State { context: vec2(100., 200.), ..Default::default() };
        let row = row(state.anchor(Group::Context, geometry(1024., 768.).0), 0, 1.);
        assert_eq!(row, Rect::new(100., 200., 192., 27.));
    }
    #[test]
    fn actions_only_offer_recruitment_with_a_legal_castle_destination() {
        let target = Hex { x: 3, y: 4 };
        let actions = [Action::Recruit {
            to: target,
            unit_type: "Mage".into(),
        }];
        assert_eq!(available(false, false, &[]), [Command::Quit]);
        assert_eq!(destination(Command::Recruit, &actions, None), Some(target));
        assert_eq!(
            items(Group::Actions, &available(true, false, &actions)),
            [Command::Recruit]
        );
        assert_eq!(geometry(1024.0, 768.0).0[1].x, 100.0);
        assert_eq!(geometry(800.0, 600.0).0[1].x, 55.0);
    }
}
