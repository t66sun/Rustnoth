use super::{FixedMatch, Rules};
use crate::{
    board::{MoveError, UnitState},
    hex::Hex,
    map::ScenarioMap,
    visibility::{HexVisibility, VisibilityMask, sight_from},
};
use std::collections::BTreeSet;

fn unit_sight(map: &ScenarioMap, rules: &Rules<'_>, origin: Hex, unit: &UnitState<'_>) -> BTreeSet<Hex> {
    sight_from(map, origin, unit.vision_range(), |terrain| Some(rules.sight_cost(unit, terrain)))
}

impl FixedMatch<'_> {
    pub(super) fn unit_visible_on_known_hex(&self, side: usize, position: Hex) -> bool {
        let sides = &self.scenario.sides;
        let lawful = self.scenario.schedule.at_turn(self.current_round).lawful_bonus;
        self.scenario.board.unit_visible_on_known_hex(side, position, lawful, |a, b|
            sides[a - 1].is_enemy(&sides[b - 1])
        )
    }

    pub(super) fn planned_route(&self, side: usize, from: Hex, to: Hex) -> Result<Vec<(Hex, u32)>, MoveError> {
        let board = &self.scenario.board;
        let unit = board.unit_at(from).ok_or(MoveError::MissingUnit(from))?;
        let vision = self.effective_vision(side);
        let unit_type = unit.unit_type;
        let inherited = self.rules.movement.get(&unit_type.movement_type).map(|m| &m.defense);
        let sides = &self.scenario.sides;
        board.walk_steps_in_view(side, from, to,
            |terrain| self.rules.walking_cost(unit_type, terrain),
            |terrain| unit_type.defense_overrides.hit_chance_with_fallback(self.rules.terrain, terrain, inherited),
            |other| sides[side - 1].is_enemy(&sides[other - 1]),
            |hex| matches!(vision.at(hex), Some(HexVisibility::Visible | HexVisibility::Fogged)),
            |hex| vision.at(hex) == Some(HexVisibility::Visible) && self.unit_visible_on_known_hex(side, hex),
        )
    }

    pub(super) fn effective_vision(&self, side: usize) -> VisibilityMask {
        let sides = &self.scenario.sides;
        self.vision[side - 1].with_allied_vision(&sides[side - 1], sides.iter().zip(&self.vision))
    }

    pub(super) fn refog(&mut self, side: usize) {
        let settings = &self.scenario.sides[side - 1];
        if !settings.fog && !settings.shroud {
            return;
        }
        let board = &self.scenario.board;
        let sight = board.units().filter(|(_, unit)| unit.side == side)
            .flat_map(|(hex, unit)| unit_sight(board.map(), &self.rules, hex, unit));
        self.vision[side - 1].replace_sight(sight);
    }

    pub(super) fn reveal_travelled_hexes(&mut self, position: Hex, route: impl IntoIterator<Item = Hex>) {
        let board = &self.scenario.board;
        if let Some(unit) = board.unit_at(position) {
            let side = &self.scenario.sides[unit.side - 1];
            if side.fog || side.shroud {
                let seen = route.into_iter().flat_map(|hex| unit_sight(board.map(), &self.rules, hex, unit));
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
