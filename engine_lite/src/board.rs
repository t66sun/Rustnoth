use std::{
    collections::{BTreeMap, btree_map::Entry},
    error, fmt,
    num::NonZeroU32,
};

use crate::{
    hex::Hex,
    map::ScenarioMap,
    movement::{MovementTile, Occupancy, reachable_destinations},
    starting_units::StartingUnit,
    turn::Turn,
    unit_type::UnitType,
};

mod attack;
pub use attack::{Attack, AttackChoice, AttackError, StrikeModifiers};

/// Full referee state for the fixed map and its units.
///
/// Use a side-specific observation when supplying information to a policy.
#[derive(Debug)]
pub struct Battlefield<'rules> {
    map: ScenarioMap,
    units: BTreeMap<Hex, UnitState<'rules>>,
}

#[derive(Debug)]
pub struct UnitState<'rules> {
    pub side: usize,
    pub id: Option<String>,
    pub unit_type: &'rules UnitType,
    pub hitpoints: i32,
    pub moves_left: i32,
    pub attacks_left: i32,
    pub can_recruit: bool,
}

impl<'rules> Battlefield<'rules> {
    /// Places the authored units at their resolved locations with base type stats.
    pub fn new(
        map: ScenarioMap,
        starting_units: Vec<StartingUnit>,
        unit_types: &'rules BTreeMap<String, UnitType>,
    ) -> Result<Self, BoardError> {
        let mut units = BTreeMap::new();
        for start in starting_units {
            let position = start.position;
            if !map.is_playable(position) {
                return Err(BoardError::OutsideMap(position));
            }
            let Entry::Vacant(tile) = units.entry(position) else {
                return Err(BoardError::Occupied(position));
            };
            let unit_type =
                unit_types
                    .get(&start.type_id)
                    .ok_or_else(|| BoardError::UnknownUnitType {
                        position,
                        type_id: start.type_id,
                    })?;
            tile.insert(UnitState {
                side: start.side,
                id: start.id,
                unit_type,
                hitpoints: unit_type.hitpoints,
                moves_left: unit_type.movement.max(0),
                attacks_left: unit_type.max_attacks.max(0),
                can_recruit: start.can_recruit,
            });
        }
        Ok(Self { map, units })
    }

    pub fn map(&self) -> &ScenarioMap {
        &self.map
    }

    pub fn unit_at(&self, position: Hex) -> Option<&UnitState<'rules>> {
        self.units.get(&position)
    }

    pub(crate) fn stop_movement(&mut self, position: Hex) {
        self.units.get_mut(&position).expect("unit at move destination").moves_left = 0;
    }

    /// Applies a current-turn move on a fully visible fixed battlefield.
    ///
    /// The referee supplies the mover's terrain cost and the active side's
    /// enemy relations. Zone of control comes from adjacent enemy base units.
    /// This handles ordinary walking without intermediate movement effects.
    pub fn move_unit(
        &mut self,
        active_side: usize,
        from: Hex,
        to: Hex,
        movement_cost: impl Fn(&str) -> Option<NonZeroU32>,
        is_enemy: impl Fn(usize) -> bool,
    ) -> Result<(), MoveError> {
        let remaining = self
            .move_destinations(active_side, from, movement_cost, is_enemy)?
            .get(&to)
            .copied()
            .ok_or(MoveError::Unreachable(to))?;

        let mut unit = self.units.remove(&from).expect("validated move source");
        // The search can leave at most the unit's starting i32 movement budget.
        unit.moves_left = remaining as i32;
        self.units.insert(to, unit);
        Ok(())
    }

    /// Returns legal destinations in hex order and the movement left at each.
    ///
    /// The referee must use this on a fully visible battlefield with the
    /// active side's terrain costs and enemy relations.
    pub fn move_destinations(
        &self,
        active_side: usize,
        from: Hex,
        movement_cost: impl Fn(&str) -> Option<NonZeroU32>,
        is_enemy: impl Fn(usize) -> bool,
    ) -> Result<BTreeMap<Hex, u32>, MoveError> {
        let unit = self.unit_at(from).ok_or(MoveError::MissingUnit(from))?;
        if unit.side != active_side {
            return Err(MoveError::WrongSide(from));
        }
        let moves_left = unit.moves_left.max(0) as u32;
        let hostile = |unit: &UnitState<'rules>| unit.side != active_side && is_enemy(unit.side);

        Ok(reachable_destinations(from, moves_left, |position| {
            if !self.map.is_playable(position) {
                return None;
            }
            let cost = movement_cost(self.map.terrain_at(position)?)?;
            let occupancy = match self.unit_at(position) {
                Some(unit) if hostile(unit) => Occupancy::Enemy,
                Some(_) => Occupancy::Ally,
                None => Occupancy::Empty,
            };
            let stops_movement = position.neighbors().any(|neighbor| {
                self.unit_at(neighbor)
                    .is_some_and(|unit| hostile(unit) && unit.unit_type.emits_zoc)
            });
            Some(MovementTile {
                cost,
                occupancy,
                stops_movement,
            })
        }))
    }

    /// Iterates over units in hex order.
    pub fn units(&self) -> impl Iterator<Item = (Hex, &UnitState<'rules>)> + '_ {
        self.units.iter().map(|(&position, unit)| (position, unit))
    }

    /// Advances the referee's side turn and refreshes the incoming side's actions.
    pub fn advance_turn(
        &mut self,
        current: Turn,
        active_sides: &[usize],
        last_round: Option<i32>,
    ) -> Option<Turn> {
        let next = current.next(active_sides, last_round)?;
        for unit in self.units.values_mut().filter(|unit| unit.side == next.side) {
            unit.moves_left = unit.unit_type.movement.max(0);
            unit.attacks_left = unit.unit_type.max_attacks.max(0);
        }
        Some(next)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum BoardError {
    OutsideMap(Hex),
    Occupied(Hex),
    UnknownUnitType { position: Hex, type_id: String },
}

impl fmt::Display for BoardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutsideMap(hex) => write!(f, "starting hex {hex:?} is outside the playable map"),
            Self::Occupied(hex) => write!(f, "starting hex {hex:?} already has a unit"),
            Self::UnknownUnitType { position, type_id } => {
                write!(f, "unknown unit type {type_id:?} at starting hex {position:?}")
            }
        }
    }
}

impl error::Error for BoardError {}

#[derive(Debug, PartialEq, Eq)]
pub enum MoveError {
    MissingUnit(Hex),
    WrongSide(Hex),
    Unreachable(Hex),
}

impl fmt::Display for MoveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingUnit(hex) => write!(f, "no unit to move at {hex:?}"),
            Self::WrongSide(hex) => write!(f, "unit at {hex:?} is not on the active side"),
            Self::Unreachable(hex) => write!(f, "cannot reach {hex:?} this turn"),
        }
    }
}

impl error::Error for MoveError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{starting_units::read_starting_units, wml};

    const WEST: Hex = Hex { x: 0, y: 0 };
    const EAST: Hex = Hex { x: 1, y: 0 };

    fn map() -> ScenarioMap {
        ScenarioMap::parse("Xv,Xv,Xv,Xv\nXv,2 Kh,1 Kh,Xv\nXv,Xv,Xv,Xv").unwrap()
    }

    fn types() -> BTreeMap<String, UnitType> {
        let config = wml::parse("id=Spearman\nhitpoints=36\nmovement=5\nattacks=2\nlevel=1").unwrap();
        let unit_type = UnitType::from_wml(config);
        BTreeMap::from([(unit_type.id.clone(), unit_type)])
    }

    #[test]
    fn initializes_authored_units_from_their_types() {
        let scenario = wml::parse(
            "[side]\nid=first\ntype=Spearman\n[/side]\n[side]\ntype=Spearman\n[/side]",
        )
        .unwrap();
        let map = map();
        let starts = read_starting_units(&scenario, &map).unwrap();
        let unit_types = types();
        let board = Battlefield::new(map, starts, &unit_types).unwrap();
        let first = board.unit_at(EAST).unwrap();

        assert_eq!(board.map().terrain_at(WEST), Some("Kh"));
        assert_eq!(first.side, 1);
        assert_eq!(first.id.as_deref(), Some("first"));
        assert!(std::ptr::eq(first.unit_type, &unit_types["Spearman"]));
        assert_eq!(
            (first.hitpoints, first.moves_left, first.attacks_left),
            (36, 5, 2)
        );
        assert!(first.can_recruit);
        let positions: Vec<_> = board.units().map(|(hex, unit)| (hex, unit.side)).collect();
        assert_eq!(positions, [(WEST, 2), (EAST, 1)]);
    }

    #[test]
    fn rejects_unknown_types_and_invalid_placements() {
        let unit_types = types();
        let start = |position, type_id: &str| StartingUnit {
            side: 1,
            id: None,
            type_id: type_id.to_owned(),
            position,
            can_recruit: false,
        };
        let border = Hex { x: -1, y: 0 };

        assert_eq!(
            Battlefield::new(map(), vec![start(border, "Spearman")], &unit_types).unwrap_err(),
            BoardError::OutsideMap(border)
        );
        assert_eq!(
            Battlefield::new(map(), vec![start(WEST, "missing")], &unit_types).unwrap_err(),
            BoardError::UnknownUnitType {
                position: WEST,
                type_id: "missing".to_owned(),
            }
        );
        assert_eq!(
            Battlefield::new(
                map(),
                vec![start(WEST, "Spearman"), start(WEST, "Spearman")],
                &unit_types,
            )
            .unwrap_err(),
            BoardError::Occupied(WEST)
        );
    }

    fn corridor_hex(y: i32) -> Hex {
        Hex { x: 0, y }
    }

    fn corridor<'rules>(
        unit_types: &'rules BTreeMap<String, UnitType>,
        positions: &[(i32, usize)],
    ) -> Battlefield<'rules> {
        let map = ScenarioMap::parse(
            "Xv,Xv,Xv\nXv,Gg,Xv\nXv,Gg,Xv\nXv,Hh,Xv\nXv,Gg,Xv\nXv,Gg,Xv\nXv,Xv,Xv",
        )
        .unwrap();
        let starts = positions
            .iter()
            .map(|&(y, side)| StartingUnit {
                side,
                id: Some(format!("unit_{y}")),
                type_id: "Spearman".to_owned(),
                position: corridor_hex(y),
                can_recruit: false,
            })
            .collect();
        Battlefield::new(map, starts, unit_types).unwrap()
    }

    fn walking_cost(terrain: &str) -> Option<NonZeroU32> {
        NonZeroU32::new(if terrain == "Hh" { 2 } else { 1 })
    }

    #[test]
    fn lists_legal_destinations_with_their_execution_costs() {
        let unit_types = types();
        let from = corridor_hex(0);
        let occupied = [(0, 1), (1, 2), (4, 3)];
        let board = corridor(&unit_types, &occupied);
        let is_enemy = |side| side == 3;

        let destinations = board
            .move_destinations(1, from, walking_cost, is_enemy)
            .unwrap();
        assert_eq!(
            destinations,
            BTreeMap::from([(corridor_hex(2), 2), (corridor_hex(3), 0)]),
        );
        assert_eq!(board.unit_at(from).unwrap().moves_left, 5);
        assert_eq!(
            board.move_destinations(1, corridor_hex(1), walking_cost, is_enemy),
            Err(MoveError::WrongSide(corridor_hex(1))),
        );
        assert_eq!(
            board.move_destinations(1, corridor_hex(2), walking_cost, is_enemy),
            Err(MoveError::MissingUnit(corridor_hex(2))),
        );

        for (to, remaining) in destinations {
            let mut board = corridor(&unit_types, &occupied);
            board.move_unit(1, from, to, walking_cost, is_enemy).unwrap();
            assert_eq!(board.unit_at(to).unwrap().moves_left, remaining as i32);
        }
    }

    #[test]
    fn moves_through_an_ally_and_spends_the_route_cost() {
        let unit_types = types();
        let from = corridor_hex(0);
        let ally = corridor_hex(1);
        let to = corridor_hex(2);
        let mut board = corridor(&unit_types, &[(0, 1), (1, 2)]);

        board.move_unit(1, from, to, walking_cost, |_| false).unwrap();

        assert!(board.unit_at(from).is_none());
        assert_eq!(board.unit_at(ally).unwrap().id.as_deref(), Some("unit_1"));
        let moved = board.unit_at(to).unwrap();
        assert_eq!(moved.id.as_deref(), Some("unit_0"));
        assert_eq!(
            (moved.moves_left, moved.hitpoints, moved.attacks_left),
            (2, 36, 2)
        );
    }

    #[test]
    fn rejected_moves_leave_units_and_movement_in_place() {
        let unit_types = types();
        let from = corridor_hex(0);
        let ally = corridor_hex(1);
        let to = corridor_hex(3);
        let mut board = corridor(&unit_types, &[(0, 1), (1, 2)]);

        assert_eq!(
            board.move_unit(2, from, to, walking_cost, |_| false),
            Err(MoveError::WrongSide(from)),
        );
        assert_eq!(
            board.move_unit(1, from, ally, walking_cost, |_| false),
            Err(MoveError::Unreachable(ally)),
        );
        assert_eq!(
            board.move_unit(1, from, to, |_| NonZeroU32::new(2), |_| false),
            Err(MoveError::Unreachable(to)),
        );
        assert_eq!(board.unit_at(from).unwrap().moves_left, 5);
        assert_eq!(board.unit_at(ally).unwrap().side, 2);
        assert!(board.unit_at(to).is_none());
    }

    #[test]
    fn enemy_occupancy_blocks_entry_and_zone_of_control_ends_movement() {
        let unit_types = types();
        let from = corridor_hex(0);
        let controlled = corridor_hex(2);
        let enemy = corridor_hex(3);
        let mut board = corridor(&unit_types, &[(0, 1), (3, 2)]);

        assert_eq!(
            board.move_unit(1, from, enemy, walking_cost, |side| side == 2),
            Err(MoveError::Unreachable(enemy)),
        );
        assert_eq!(board.unit_at(from).unwrap().moves_left, 5);
        board
            .move_unit(1, from, controlled, walking_cost, |side| side == 2)
            .unwrap();

        assert_eq!(board.unit_at(controlled).unwrap().moves_left, 0);
        assert_eq!(board.unit_at(enemy).unwrap().side, 2);
        assert_eq!(
            board.move_unit(1, controlled, corridor_hex(1), walking_cost, |side| side == 2),
            Err(MoveError::Unreachable(corridor_hex(1))),
        );
    }

    #[test]
    fn advancing_refreshes_only_the_incoming_side() {
        let scenario = wml::parse(
            "[side]\ntype=Spearman\n[/side]\n[side]\ntype=Spearman\n[/side]",
        )
        .unwrap();
        let map = map();
        let starts = read_starting_units(&scenario, &map).unwrap();
        let unit_types = types();
        let mut board = Battlefield::new(map, starts, &unit_types).unwrap();
        for unit in board.units.values_mut() {
            unit.hitpoints = 17;
            unit.moves_left = 0;
            unit.attacks_left = 0;
        }

        let first = Turn { round: 1, side: 1 };
        let second = Turn { round: 1, side: 2 };
        assert_eq!(board.advance_turn(first, &[1, 2], Some(1)), Some(second));
        let incoming = board.unit_at(WEST).unwrap();
        assert_eq!(
            (incoming.moves_left, incoming.attacks_left, incoming.hitpoints),
            (5, 2, 17)
        );
        assert_eq!(board.unit_at(EAST).unwrap().moves_left, 0);
        assert_eq!(board.advance_turn(second, &[1, 2], Some(1)), None);
        assert_eq!(board.unit_at(EAST).unwrap().attacks_left, 0);
    }
}
