use crate::{dialog, title_screen};
use engine_lite::{fixed_match::MatchState, turn::Turn, wml::Node};
use macroquad::prelude::*;
use std::path::Path;

pub(super) fn names(scenario: &Node) -> Vec<String> {
    let login = std::env::var("USERNAME").or_else(|_| std::env::var("USER")).unwrap_or_default();
    scenario.children.iter().filter(|(tag, _)| tag == "side").map(|(_, side)| {
        ["side_name", "current_player"].iter()
            .find_map(|key| side.attribute(key).filter(|value| !value.is_empty()))
            .unwrap_or(&login).to_owned()
    }).collect()
}

pub(super) fn new_turn(state: MatchState, can_play: bool, previous: Option<Turn>) -> Option<Turn> {
    let MatchState::Playing(turn) = state else { return None };
    (previous != Some(turn) && can_play).then_some(turn)
}

pub(super) async fn show(root: &Path, font: Option<&Font>, message: &str) {
    let blindfold = Texture2D::from_rgba8(1, 1, &[0, 0, 0, 255]);
    show_on(root, font, message, &blindfold).await;
}

pub(super) async fn over_board(root: &Path, font: Option<&Font>, message: &str) {
    let background = Texture2D::from_image(&get_screen_data());
    show_on(root, font, message, &background).await;
}

async fn show_on(root: &Path, font: Option<&Font>, message: &str, background: &Texture2D) {
    let skin = dialog::Skin::load(root);
    let width = (measure_text(message, font, 17, 1.).width.ceil() + 30.).clamp(80., 800.);
    let lines = title_screen::wrap(message, font, width - 30.);
    let height = 30. + lines.len() as f32 * 22.;
    let mut opening = true;
    loop {
        let (panel, _) = skin.frame("", width, height, background);
        for (row, line) in lines.iter().enumerate() {
            dialog::text(font, line, panel.x + 15., panel.y + 32. + row as f32 * 22.,
                17, Color::from_rgba(215, 215, 215, 255));
        }
        set_default_camera();
        let dismiss = !opening && (is_mouse_button_pressed(MouseButton::Left) || is_key_pressed(KeyCode::Enter)
            || is_key_pressed(KeyCode::Space) || is_key_pressed(KeyCode::Escape));
        opening = false;
        next_frame().await;
        if dismiss { break; }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompts_once_for_each_local_human_turn_and_ends_at_victory() {
        let first = Turn { round: 1, side: 1 };
        let other = Turn { round: 1, side: 2 };
        let next = Turn { round: 2, side: 1 };
        assert_eq!(new_turn(MatchState::Playing(first), true, None), Some(first));
        assert_eq!(new_turn(MatchState::Playing(first), true, Some(first)), None);
        assert_eq!(new_turn(MatchState::Playing(other), false, Some(first)), None);
        assert_eq!(new_turn(MatchState::Playing(other), true, Some(first)), Some(other));
        assert_eq!(new_turn(MatchState::Playing(next), true, Some(first)), Some(next));
        assert_eq!(new_turn(MatchState::Won(1), true, Some(first)), None);
        let config = engine_lite::wml::parse("[side]\nside_name=Guard\ncurrent_player=Pat\n[/side]\n[side]\ncurrent_player=Sam\n[/side]").unwrap();
        assert_eq!(names(&config), ["Guard", "Sam"]);
    }
}
