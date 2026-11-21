#[cfg(test)]
mod tests;

use super::Session;
use engine_lite::{fixed_match::{MatchState, enemy_moves::EnemyReach}, hex::Hex};

#[derive(Default)]
pub(super) struct Display {
    reach: Option<EnemyReach>,
    // While this is set, motion over empty hexes leaves the combined mask intact.
    combined: bool,
    wait_for_empty: bool,
    last_hover: Option<Hex>,
}

impl Session<'_> {
    pub fn can_show_enemy_moves(&self) -> bool {
        self.computer_action.is_none() && matches!(self.state(), MatchState::Playing(_))
            && self.observation().is_some_and(|view| view.units.iter().any(|u| {
                !u.petrified && !view.allied_sides.contains(&u.side)
            }))
    }

    pub fn show_enemy_moves(&mut self, cursor: Option<Hex>) {
        if !self.can_show_enemy_moves() { return; }
        let Some(view) = self.observation.as_ref() else { return };
        let side = view.side;
        let wait_for_empty = view.units.iter().any(|u| Some(u.position) == cursor);
        self.enemy_moves = Display {
            reach: Some(self.game.enemy_reach(side, None, cursor)),
            combined: true, wait_for_empty,
            last_hover: cursor,
        };
    }

    pub fn clear_enemy_moves(&mut self) {
        self.enemy_moves = Display::default();
    }

    pub fn dismiss_enemy_moves(&mut self, cursor: Option<Hex>) {
        self.enemy_moves = Display { last_hover: cursor, ..Default::default() };
    }

    pub fn enemy_reach(&self) -> Option<&EnemyReach> {
        self.enemy_moves.reach.as_ref()
    }

    pub fn hover_enemy_moves(&mut self, at: Option<Hex>, selected: bool) {
        if self.enemy_moves.last_hover == at { return; }
        self.enemy_moves.last_hover = at;
        if selected || self.computer_action.is_some() || !matches!(self.state(), MatchState::Playing(_)) { return; }
        let Some(view) = self.observation.as_ref() else { return };
        let unit = at.and_then(|hex| view.units.iter().find(|u| u.position == hex));
        if self.enemy_moves.wait_for_empty {
            if unit.is_none() { self.enemy_moves.wait_for_empty = false; }
            return;
        }
        if unit.is_none() && self.enemy_moves.combined { return; }
        // Native mouse movement over a unit replaces the global reach map.
        self.enemy_moves.combined = false;
        self.enemy_moves.reach = unit.filter(|u| !u.petrified && !view.allied_sides.contains(&u.side))
            .map(|u| self.game.enemy_reach(view.side, Some(u.position), None));
    }
}
