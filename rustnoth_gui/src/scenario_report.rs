use crate::dialog::{self, Skin};
use engine_lite::fixed_match::MatchState;
use macroquad::prelude::*;
use rustnoth_gui::session::Session;
use std::path::Path;

fn outcome(state: MatchState, viewer: usize) -> Option<(&'static str, String)> {
    let (won, side) = match state {
        MatchState::Playing(_) => return None,
        MatchState::Draw => return Some(("Scenario Report", "The game is a draw.".into())),
        MatchState::CampaignVictory => (true, None),
        MatchState::CampaignDefeat => (false, None),
        MatchState::Won(side) => (side == viewer, Some(format!("Side {side} wins."))),
        MatchState::AlliedVictory(sides) => (
            sides.contains(&viewer),
            Some(format!(
                "Sides {} win.",
                sides
                    .iter()
                    .map(usize::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
        ),
    };
    let title = if won { "Victory" } else { "Defeat" };
    let mut message = if won {
        "You have emerged victorious!"
    } else {
        "You have been defeated!"
    }
    .to_owned();
    if let Some(result) = side {
        message.push_str("  ");
        message.push_str(&result);
    }
    Some((title, message))
}

pub(super) async fn show(root: &Path, session: &Session<'_>, campaign: bool) -> bool {
    let Some(view) = session.observation() else {
        return false;
    };
    let Some((title, message)) = outcome(session.state(), view.side) else {
        return false;
    };
    let retained = campaign.then(|| session.retained_gold()).flatten();
    let background = dialog::Backdrop::capture(FilterMode::Linear);
    let skin = Skin::load(root);
    next_frame().await;
    loop {
        let (r, mouse) = skin.frame(
            title,
            540.0,
            if retained.is_some() { 238.0 } else { 181.0 },
            background.texture(),
        );
        dialog::text(
            skin.font.as_ref(),
            &message,
            r.x + 22.0,
            r.y + 85.0,
            19,
            WHITE,
        );
        if let Some(amount) = retained {
            if let Some(gold) = view.gold {
                dialog::text(
                    skin.font.as_ref(),
                    &format!("Remaining gold: {gold}"),
                    r.x + 22.0,
                    r.y + 118.0,
                    16,
                    dialog::PALE,
                );
            }
            dialog::text(
                skin.font.as_ref(),
                &format!("Retained gold: {amount}"),
                r.x + 22.0,
                r.y + 143.0,
                17,
                dialog::GOLD,
            );
        }
        let advance = Rect::new(r.right() - 145.0, r.bottom() - 48.0, 125.0, 32.0);
        let inspect = Rect::new(advance.x - 128.0, advance.y, 118.0, advance.h);
        let next = skin.button(
            advance,
            mouse,
            if campaign && session.next_scenario().is_some() {
                "Continue"
            } else {
                "Close Game"
            },
            true,
        );
        let map = skin.button(inspect, mouse, "View Map", true);
        set_default_camera();
        crate::frame_trace::ready("victory_ready");
        if next || is_key_pressed(KeyCode::Enter) {
            next_frame().await;
            return true;
        }
        if map || is_key_pressed(KeyCode::Escape) {
            next_frame().await;
            return false;
        }
        next_frame().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn results_use_the_viewing_side_and_distinguish_a_draw_from_defeat() {
        assert_eq!(
            outcome(MatchState::AlliedVictory(vec![1, 3]), 3).unwrap().0,
            "Victory"
        );
        assert_eq!(outcome(MatchState::Won(2), 1).unwrap().0, "Defeat");
        assert_eq!(outcome(MatchState::Draw, 1).unwrap().0, "Scenario Report");
    }
}
