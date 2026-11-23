use super::FixedMatch;
use crate::{
    board::{MoveError, UnitState},
    hex::Hex,
    visibility::{HexVisibility, VisibilityMask, sight_with_teleports},
};
use std::collections::BTreeSet;

impl FixedMatch<'_> {
    /// Terrain on non-playable border hexes for the local display. Omit
    /// unexplored cells, just as observation_for omits unexplored map tiles.
    pub fn known_border(&self, side: usize) -> crate::map::KnownBorder<'_> {
        if side == 0 || side > self.vision.len() { return Default::default(); }
        let sight = self.effective_vision(side);
        let map = self.scenario.board.map();
        crate::map::border_positions(map.width(), map.height()).filter_map(|at| {
            let visibility = sight.visibility(at);
            if visibility == HexVisibility::Shrouded { return None; }
            Some((at, (map.terrain_at(at)?, visibility == HexVisibility::Visible)))
        }).collect()
    }

    pub(super) fn unit_sight(&self, from: Hex, origin: Hex, viewer: &UnitState<'_>) -> BTreeSet<Hex> {
        let board = &self.scenario.board;
        // On a pass through an ally, native vision_path uses the tunnel ability of
        // the unit at the sight origin, with the moving viewer's range and costs.
        let (teleporter, occupied_village) = match board.unit_at(origin) {
            Some(unit) => (unit, origin),
            None => (viewer, from),
        };
        // The stock WFL unit_at(loc) predicate sees real occupants, including
        // in shroud, while evaluating the village network for the referee.
        let villages = self.teleport_villages(occupied_village, teleporter, |_| true, |_| true);
        sight_with_teleports(board.map(), origin, viewer.vision_range(), &villages, |terrain| {
            Some(self.rules.sight_cost(viewer, terrain))
        })
    }

    pub(super) fn unit_visible_on_known_hex(&self, side: usize, position: Hex) -> bool {
        let sides = &self.scenario.sides;
        let lawful = self.lawful_bonus_at(self.current_round, position);
        self.scenario.board.unit_visible_on_known_hex(side, position, lawful, |a, b|
            sides[a - 1].is_enemy(&sides[b - 1])
        )
    }

    pub(super) fn planned_route(&self, side: usize, from: Hex, to: Hex) -> Result<Vec<(Hex, u32)>, MoveError> {
        let board = &self.scenario.board;
        let unit = board.unit_at(from).ok_or(MoveError::MissingUnit(from))?;
        let unit_type = unit.unit_type;
        let inherited = self.rules.movement.get(&unit_type.movement_type).map(|m| &m.defense);
        let sides = &self.scenario.sides;
        let known = |hex| matches!(self.visibility_at(side, hex), Some(HexVisibility::Visible | HexVisibility::Fogged));
        let visible = |hex| self.visibility_at(side, hex) == Some(HexVisibility::Visible) && self.unit_visible_on_known_hex(side, hex);
        let villages = self.teleport_villages(from, unit, known, visible);
        board.walk_steps_in_view(side, from, to,
            |terrain| self.rules.walking_cost(unit_type, terrain),
            |terrain| unit_type.defense_overrides.hit_chance_with_fallback(self.rules.terrain, terrain, inherited),
            |other| sides[side - 1].is_enemy(&sides[other - 1]),
            known, visible, &villages,
        )
    }

    pub(super) fn visibility_at(&self, side: usize, hex: Hex) -> Option<HexVisibility> {
        let sides = &self.scenario.sides;
        self.vision[side - 1].at_with_allies(hex, &sides[side - 1], sides.iter().zip(&self.vision))
    }

    pub(super) fn effective_vision(&self, side: usize) -> VisibilityMask {
        let sides = &self.scenario.sides;
        self.vision[side - 1].with_allied_vision(&sides[side - 1], sides.iter().zip(&self.vision))
    }

    pub(super) fn refog(&mut self, side: usize) {
        self.update_side_sight(side, true);
    }

    pub(super) fn update_side_sight(&mut self, side: usize, reset_fog: bool) {
        let settings = &self.scenario.sides[side - 1];
        if !settings.fog && !settings.shroud {
            return;
        }
        let board = &self.scenario.board;
        let sight: BTreeSet<_> = board.units().filter(|(_, unit)| unit.side == side)
            .flat_map(|(hex, unit)| self.unit_sight(hex, hex, unit)).collect();
        if reset_fog {
            self.vision[side - 1].replace_sight(sight);
        } else {
            self.vision[side - 1].add_sight(sight);
        }
    }

    pub(super) fn reveal_travelled_hexes(&mut self, position: Hex, route: impl IntoIterator<Item = Hex>) {
        let board = &self.scenario.board;
        if let Some(unit) = board.unit_at(position) {
            let side = &self.scenario.sides[unit.side - 1];
            if side.fog || side.shroud {
                let seen: BTreeSet<_> = route.into_iter()
                    .flat_map(|hex| self.unit_sight(position, hex, unit)).collect();
                self.vision[unit.side - 1].add_sight(seen);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{fixed_match::Action, scenario_bundle::Bundle};

    #[test]
    fn battle_border_obeys_shroud_and_fog_without_expanding_the_policy_map() {
        let bundle = Bundle::parse(br#"
[units]
    [unit_type]
        id=Scout
        hitpoints=10
        movement=4
        vision=0
    [/unit_type]
[/units]
[scenario]
    map_data="Gs^Fds,Gs^Fds,Hh,Hh,Hh,Ww,Ww,Ww
Gs^Fds,1 Gg,Gg,Gg,Gg,Gg,2 Gg,Ww
Gs^Fds,Gg,Gg,Gg,Gg,Gg,Gg,Ww
Gs^Fds,Gs^Fds,Hh,Hh,Hh,Ww,Ww,Ww"
    [side]
        type=Scout
        fog=yes
        shroud=yes
        shroud_data="|0|0|0|0|1"
    [/side]
    [side]
        type=Scout
        fog=yes
    [/side]
[/scenario]
"#).unwrap();
        let game = bundle.start(1, |_| unreachable!()).unwrap();
        let north = Hex { x: 0, y: -1 };
        let remembered = Hex { x: 3, y: -1 };
        let unseen = Hex { x: 4, y: -1 };
        let first = game.known_border(1);
        assert_eq!(first.get(&north), Some(&("Gs^Fds", true)));
        assert_eq!(first.get(&remembered), Some(&("Hh", false)));
        assert!(!first.contains_key(&unseen));
        assert_eq!(game.known_border(2).get(&north), Some(&("Gs^Fds", false)));
        let view = game.observation_for(1).unwrap();
        assert!(view.tiles.iter().all(|tile| !first.contains_key(&tile.position)
            && tile.position.x >= 0 && tile.position.y >= 0
            && tile.position.x < view.width && tile.position.y < view.height));
    }

    #[test]
    fn authored_exploration_persists_and_saved_sight_resets_on_the_viewers_turn() {
        let bundle = Bundle::parse(br#"
[units]
    [unit_type]
        id=Scout
        hitpoints=10
        movement=4
        vision=0
    [/unit_type]
[/units]
[scenario]
    map_data="Xv,Xv,Xv,Xv,Xv,Xv,Xv,Xv,Xv
Xv,1 Gg,Gg,Gg,Gg,Gg,Gg,2 Gg,Xv
Xv,Xv,Xv,Xv,Xv,Xv,Xv,Xv,Xv"
    [side]
        side=1
        type=Scout
        fog=yes
        shroud=yes
        shroud_data="|0|0|0|0|01"
        fog_data="|0|0|0|0|01"
    [/side]
    [side]
        side=2
        type=Scout
        fog=yes
        shroud=yes
        shroud_data="|0|0|01"
        fog_data="|0|0|01|01"
    [/side]
[/scenario]
"#).unwrap();
        let mut game = bundle.start(1, |_| unreachable!()).unwrap();
        let tile = |game: &FixedMatch<'_>, side, wml_x| {
            game.observation_for(side).unwrap().tiles.into_iter()
                .find(|tile| tile.position == Hex::from_wml(wml_x, 1).unwrap())
                .map(|tile| tile.visible)
        };
        assert_eq!(tile(&game, 1, 4), Some(false));
        assert_eq!(tile(&game, 2, 2), Some(true));
        assert_eq!(tile(&game, 2, 3), None);
        game.step(Action::EndTurn).unwrap();
        assert_eq!(tile(&game, 2, 2), Some(false));
        game.step(Action::EndTurn).unwrap();
        assert_eq!(tile(&game, 1, 4), Some(false));
    }

    #[test]
    fn hidden_units_and_terrain_stay_out_of_decisions_while_exploration_survives_refog() {
        let bundle = Bundle::parse(br#"
[terrain_type]
    id=flat
    string=Gg
[/terrain_type]
[units]
    [unit_type]
        id=Scout
        hitpoints=25
        movement=5
        vision=0
        [movement_costs]
            flat=1
        [/movement_costs]
        [attack]
            range=melee
            min_range=1
            max_range=9
            damage=1
            number=1
        [/attack]
    [/unit_type]
[/units]
[scenario]
    map_data="Xv,Xv,Xv,Xv,Xv,Xv,Xv,Xv,Xv
Xv,1 Gg,Gg,Gg,Gg,Gg,Gg,2 Gg,Xv
Xv,Xv,Xv,Xv,Xv,Xv,Xv,Xv,Xv"
    [side]
        side=1
        type=Scout
        fog=yes
        shroud=yes
    [/side]
    [side]
        side=2
        type=Scout
        fog=yes
        shroud=yes
    [/side]
[/scenario]
"#).unwrap();
        let mut game = bundle.start(1, |_| unreachable!()).unwrap();
        let at = |x| Hex { x, y: 0 };
        let (view, actions) = game.decision().unwrap();
        assert_eq!(view.tiles.iter().map(|tile| tile.position).collect::<Vec<_>>(), [at(0), at(1)]);
        assert_eq!(view.units.len(), 1);
        assert!(!actions.iter().any(|a| matches!(a, Action::Attack(_))));
        assert!(!actions.contains(&Action::Move { from: at(0), to: at(2) }));
        for x in 0..2 {
            game.step(Action::Move { from: at(x), to: at(x + 1) }).unwrap();
        }
        assert!(game.observation_for(1).unwrap().tiles.iter().any(|t| t.position == at(0) && t.visible));
        game.step(Action::EndTurn).unwrap();
        let view = game.observation_for(1).unwrap();
        assert!(view.tiles.iter().any(|t| t.position == at(0) && !t.visible));
        assert!(view.tiles.iter().any(|t| t.position == at(3) && t.visible));
        assert!(!view.tiles.iter().any(|t| t.position == at(4)));
    }
}
