use std::{
    collections::{BTreeMap, btree_map::Entry},
    error, fmt,
};

use crate::{hex::Hex, map::ScenarioMap, starting_units::StartingUnit, unit_type::UnitType};

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

    /// Iterates over units in hex order.
    pub fn units(&self) -> impl Iterator<Item = (Hex, &UnitState<'rules>)> + '_ {
        self.units.iter().map(|(&position, unit)| (position, unit))
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
        let config = wml::parse("id=Spearman\nhitpoints=36\nmovement=5\nattacks=2").unwrap();
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
}
