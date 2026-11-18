use std::collections::BTreeSet;
use std::num::NonZeroU32;

use crate::hex::Hex;
use crate::map::ScenarioMap;
use crate::movement::{reachable_destinations, MovementTile, Occupancy};

/// Playable hexes one viewer uncovers using its effective vision costs and range.
/// A hex too costly to enter is still visible from an adjacent reached hex.
pub fn sight_from(
    map: &ScenarioMap,
    origin: Hex,
    budget: u32,
    cost: impl Fn(&str) -> Option<NonZeroU32>,
) -> BTreeSet<Hex> {
    if !map.is_playable(origin) {
        return BTreeSet::new();
    }
    let reached = reachable_destinations(origin, budget, |hex| {
        if !map.is_playable(hex) {
            return None;
        }
        Some(MovementTile {
            cost: cost(map.terrain_at(hex)?)?,
            occupancy: Occupancy::Empty,
            stops_movement: false,
        })
    });

    let mut sight = BTreeSet::from([origin]);
    for hex in std::iter::once(origin).chain(reached.into_keys()) {
        sight.extend(hex.neighbors().filter(|&next| map.is_playable(next)));
    }
    sight
}

/// What one side can see at a playable hex.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HexVisibility {
    /// The tile is in view, subject to unit-specific concealment rules.
    Visible,
    /// The terrain is known, but the side cannot see units here.
    Fogged,
    /// Neither the terrain nor the units are visible to this side.
    Shrouded,
}

/// A side's current sight and its exploration of the playable map.
pub struct VisibilityMask {
    width: i32,
    height: i32,
    fog: bool,
    shroud: bool,
    sight: BTreeSet<Hex>,
    explored: BTreeSet<Hex>,
}

impl VisibilityMask {
    /// Creates a mask before any tiles have been uncovered by sight.
    pub fn new(width: i32, height: i32, fog: bool, shroud: bool) -> Self {
        Self {
            width,
            height,
            fog,
            shroud,
            sight: BTreeSet::new(),
            explored: BTreeSet::new(),
        }
    }

    /// Updates current sight and preserves the terrain uncovered by earlier sight.
    pub fn replace_sight(&mut self, sight: impl IntoIterator<Item = Hex>) {
        self.sight.clear();
        for hex in sight {
            if self.is_playable(hex) {
                if self.fog {
                    self.sight.insert(hex);
                }
                if self.shroud {
                    self.explored.insert(hex);
                }
            }
        }
    }

    /// Returns the visibility of a playable tile. Map borders lie outside this mask.
    pub fn at(&self, hex: Hex) -> Option<HexVisibility> {
        if !self.is_playable(hex) {
            return None;
        }
        Some(if self.shroud && !self.explored.contains(&hex) {
            HexVisibility::Shrouded
        } else if self.fog && !self.sight.contains(&hex) {
            HexVisibility::Fogged
        } else {
            HexVisibility::Visible
        })
    }

    fn is_playable(&self, hex: Hex) -> bool {
        (0..self.width).contains(&hex.x) && (0..self.height).contains(&hex.y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEFT: Hex = Hex { x: 0, y: 0 };
    const RIGHT: Hex = Hex { x: 1, y: 0 };
    const BORDER: Hex = Hex { x: 2, y: 0 };

    #[test]
    fn sight_reveals_the_front_of_a_costly_or_impassable_tile() {
        let map = ScenarioMap::parse(
            "Xx,Xx,Xx,Xx,Xx,Xx\n\
             Xx,Gg,Gg,Hh,Gg,Xx\n\
             Xx,Xx,Xx,Xx,Xx,Xx",
        )
        .unwrap();
        let hill = Hex { x: 2, y: 0 };
        for hill_cost in [None, NonZeroU32::new(4)] {
            let cost = |terrain: &str| match terrain {
                "Hh" => hill_cost,
                _ => NonZeroU32::new(1),
            };
            assert_eq!(
                sight_from(&map, LEFT, 3, cost),
                BTreeSet::from([LEFT, RIGHT, hill])
            );
            assert_eq!(
                sight_from(&map, LEFT, 0, cost),
                BTreeSet::from([LEFT, RIGHT])
            );
        }
    }

    #[test]
    fn a_cheaper_route_reveals_the_fringe_beyond_its_last_tile() {
        let map = ScenarioMap::parse(
            "Xx,Xx,Xx,Xx,Xx,Xx,Xx\n\
             Xx,Xx,Gg,Gg,Xx,Xx,Xx\n\
             Xx,Gg,Hh,Gg,Gg,Gg,Xx\n\
             Xx,Xx,Xx,Xx,Xx,Xx,Xx",
        )
        .unwrap();
        let cost = |terrain: &str| match terrain {
            "Hh" => NonZeroU32::new(3),
            "Gg" => NonZeroU32::new(1),
            _ => None,
        };
        let origin = Hex { x: 0, y: 1 };
        assert!(sight_from(&map, origin, 3, cost).contains(&Hex { x: 4, y: 1 }));
        assert!(!sight_from(&map, origin, 2, cost).contains(&Hex { x: 4, y: 1 }));
    }

    #[test]
    fn refogging_preserves_earlier_exploration() {
        let mut mask = VisibilityMask::new(2, 1, true, true);
        assert_eq!(mask.at(LEFT), Some(HexVisibility::Shrouded));

        mask.replace_sight([LEFT]);
        assert_eq!(mask.at(LEFT), Some(HexVisibility::Visible));
        assert_eq!(mask.at(RIGHT), Some(HexVisibility::Shrouded));

        mask.replace_sight([RIGHT, BORDER]);
        assert_eq!(mask.at(LEFT), Some(HexVisibility::Fogged));
        assert_eq!(mask.at(RIGHT), Some(HexVisibility::Visible));
        assert_eq!(mask.at(BORDER), None);
    }

    #[test]
    fn shroud_without_fog_keeps_explored_hexes_visible() {
        let mut mask = VisibilityMask::new(2, 1, false, true);
        mask.replace_sight([LEFT]);
        mask.replace_sight([]);

        assert_eq!(mask.at(LEFT), Some(HexVisibility::Visible));
        assert_eq!(mask.at(RIGHT), Some(HexVisibility::Shrouded));
    }

    #[test]
    fn fog_without_shroud_leaves_terrain_known_outside_sight() {
        let mut mask = VisibilityMask::new(2, 1, true, false);
        assert_eq!(mask.at(LEFT), Some(HexVisibility::Fogged));
        mask.replace_sight([LEFT]);
        assert_eq!(mask.at(LEFT), Some(HexVisibility::Visible));
        assert_eq!(mask.at(RIGHT), Some(HexVisibility::Fogged));

        let unrestricted = VisibilityMask::new(2, 1, false, false);
        assert_eq!(unrestricted.at(RIGHT), Some(HexVisibility::Visible));
        assert_eq!(unrestricted.at(BORDER), None);
    }
}
