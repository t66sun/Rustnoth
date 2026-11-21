//! Wesnoth's confirm_end_turn preference and in-game Yes/No dialog.
use crate::{dialog, title_screen::{self, locale::Locale}};
use engine_lite::{fixed_match::Action, wml};
use macroquad::prelude::*;
use rustnoth_gui::{save_replay::Event, session::Session};
use std::{fs, io, path::Path};

const NO_MOVES: &str = "You have not started your turn yet. Do you really want to end your turn?";
const GREEN: &str = "Some units have not moved. Do you really want to end your turn?";
const YELLOW: &str = "Some units have movement left. Do you really want to end your turn?";

#[derive(Debug, Eq, PartialEq)]
pub(super) enum Mode { NoMoves, Green, Yellow, Never }

impl Mode {
    fn parse(value: &str) -> Self {
        match value.trim() {
            "" | "no_moves" => Self::NoMoves,
            "green" | "yes" => Self::Green,
            "yellow" => Self::Yellow,
            _ => Self::Never,
        }
    }

    pub(super) fn load(directory: &Path) -> io::Result<Self> {
        match fs::read_to_string(directory.join("confirm_end_turn")) {
            Ok(value) => Ok(Self::parse(&value)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Self::NoMoves),
            Err(e) => Err(e),
        }
    }

    pub(super) fn prompt(&self, session: &Session<'_>, side_overrides: &[bool]) -> Option<&'static str> {
        let view = session.observation()?;
        if side_overrides.get(view.side.checked_sub(1)?) == Some(&true) { return None; }
        match self {
            Self::NoMoves if view.units.iter().any(|unit| unit.side == view.side)
                && !acted_this_turn(session.history()) => Some(NO_MOVES),
            Self::Green if session.has_unused_unit(true) => Some(GREEN),
            Self::Yellow if session.has_unused_unit(false) => Some(YELLOW),
            _ => None,
        }
    }
}

fn acted_this_turn(history: &[Event]) -> bool {
    let last_action = history.iter().rev().find(|event| !matches!(event, Event::Dismiss));
    matches!(last_action, Some(Event::Player(action, ..)) if *action != Action::EndTurn)
}

pub(super) fn side_overrides(config: &wml::Node) -> Vec<bool> {
    config.children.iter().filter(|(tag, _)| tag == "side").map(|(_, side)| {
        matches!(side.attribute("no_turn_confirmation"), Some("yes" | "true" | "1"))
    }).collect()
}

pub(super) fn modifier_down() -> bool {
    if cfg!(target_os = "macos") { is_key_down(KeyCode::LeftAlt) || is_key_down(KeyCode::RightAlt) }
    else { is_key_down(KeyCode::LeftControl) || is_key_down(KeyCode::RightControl) }
}

pub(super) async fn confirm(root: &Path, locale: &Locale, prompt: &str) -> bool {
    let background = Texture2D::from_image(&get_screen_data());
    let skin = dialog::Skin::load(root);
    let font = skin.font.as_ref();
    let text = locale.game(prompt);
    let width = (measure_text(text, font, 16, 1.0).width + 44.0).clamp(260.0, 800.0);
    let lines = title_screen::wrap(text, font, width - 44.0);
    let height = 86.0 + lines.len() as f32 * 21.0;
    let mut yes_selected = true;
    next_frame().await;
    loop {
        let (r, mouse) = skin.frame("", width, height, &background);
        for (i, line) in lines.iter().enumerate() {
            dialog::text(font, line, r.x + 22.0, r.y + 33.0 + i as f32 * 21.0, 16, dialog::PALE);
        }
        let no = Rect::new(r.right() - 115.0, r.bottom() - 43.0, 100.0, 30.0);
        let yes = Rect::new(no.x - 110.0, no.y, no.w, no.h);
        if is_key_pressed(KeyCode::Left) || is_key_pressed(KeyCode::Right) || is_key_pressed(KeyCode::Tab) {
            yes_selected = !yes_selected;
        }
        let accepted = skin.button(yes, mouse, locale.ui("Yes"), true);
        let cancelled = skin.button(no, mouse, locale.ui("No"), true);
        let focused = if yes_selected { yes } else { no };
        draw_rectangle_lines(focused.x, focused.y, focused.w, focused.h, 1.0, dialog::GOLD);
        set_default_camera();
        if cancelled || is_key_pressed(KeyCode::Escape) { return false; }
        if accepted { return true; }
        if is_key_pressed(KeyCode::Enter) || is_key_pressed(KeyCode::Space) { return yes_selected; }
        next_frame().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_lite::{hex::Hex, scenario_bundle::Bundle};

    #[test]
    fn unchanged_partial_and_next_turn_follow_native_confirm_modes() {
        let setup = br#"[terrain_type]
id=flat
string=Gg
[/terrain_type]
[units]
[movetype]
name=foot
[movement_costs]
flat=1
[/movement_costs]
[/movetype]
[unit_type]
id=Scout
hitpoints=20
movement=3
movement_type=foot
[/unit_type]
[/units]
[scenario]
map_data="Xv,Xv,Xv,Xv,Xv,Xv,Xv
Xv,1 Gg,Gg,Gg,Gg,2 Gg,Xv
Xv,Xv,Xv,Xv,Xv,Xv,Xv"
[side]
type=Scout
[/side]
[side]
type=Scout
no_turn_confirmation=yes
[/side]
[/scenario]"#;
        let data = wml::parse(setup).unwrap();
        let (_, scenario) = data.children.iter().find(|(tag, _)| tag == "scenario").unwrap();
        let overrides = side_overrides(scenario);
        assert_eq!(overrides, [false, true]);
        let bundle = Bundle::parse(setup).unwrap();
        let mut session = Session::new(bundle.start(1, |_| unreachable!()).unwrap());
        assert_eq!(Mode::parse(""), Mode::NoMoves);
        assert_eq!(Mode::parse("yes"), Mode::Green);
        assert_eq!(Mode::parse("no_moves").prompt(&session, &overrides), Some(NO_MOVES));
        assert_eq!(Mode::Green.prompt(&session, &overrides), Some(GREEN));
        assert_eq!(Mode::parse("no").prompt(&session, &overrides), None);
        session.apply(Action::Move { from: Hex { x: 0, y: 0 }, to: Hex { x: 1, y: 0 } }).unwrap();
        assert_eq!(Mode::NoMoves.prompt(&session, &overrides), None);
        assert_eq!(Mode::Green.prompt(&session, &overrides), None);
        assert_eq!(Mode::parse("yellow").prompt(&session, &overrides), Some(YELLOW));
        session.apply(Action::EndTurn).unwrap();
        assert_eq!(Mode::NoMoves.prompt(&session, &[]), Some(NO_MOVES));
        assert_eq!(Mode::NoMoves.prompt(&session, &overrides), None);
        assert!(!acted_this_turn(&[Event::Computer(None), Event::Dismiss]));
    }
}
