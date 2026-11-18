use std::collections::{BTreeMap, BTreeSet, BinaryHeap};
use std::num::NonZeroU32;

use crate::hex::Hex;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Occupancy {
    Empty,
    Ally,
    Enemy,
}

/// Movement information visible to the side considering the move.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MovementTile {
    /// The cost for this mover to enter the hex, including movement modifiers.
    pub cost: NonZeroU32,
    pub occupancy: Occupancy,
    /// Whether a visible enemy zone of control stops this mover on entry.
    /// Supply `false` where the mover's skirmisher ability applies.
    pub stops_movement: bool,
}

/// Returns unoccupied destinations and the most movement they leave this turn.
///
/// The origin is the mover's current on-board position. The query must use
/// only information visible to the moving side and return `None` for
/// off-board or impassable hexes. Its result for a hex is fixed for the
/// duration of the search. Allied units may be passed through.
pub fn reachable_destinations(
    origin: Hex,
    moves_left: u32,
    tile_at: impl Fn(Hex) -> Option<MovementTile>,
) -> BTreeMap<Hex, u32> {
    reachable_with_teleports(origin, moves_left, &[], tile_at)
}

/// Sorted owned villages that the mover knows to be vacant (or occupies itself).
pub(crate) fn teleport_exits(villages: &[Hex], at: Hex) -> &[Hex] {
    if villages.binary_search(&at).is_ok() { villages } else { &[] }
}

pub(crate) fn reachable_with_teleports(
    origin: Hex,
    moves_left: u32,
    villages: &[Hex],
    tile_at: impl Fn(Hex) -> Option<MovementTile>,
) -> BTreeMap<Hex, u32> {
    let mut frontier = BinaryHeap::from([(moves_left, origin)]);
    let mut reached = BTreeSet::from([origin]);
    let mut destinations = BTreeMap::new();

    while let Some((remaining, current)) = frontier.pop() {
        if remaining == 0 {
            break;
        }

        for next in current.neighbors().chain(teleport_exits(villages, current).iter().copied()) {
            if reached.contains(&next) {
                continue;
            }
            let Some(tile) = tile_at(next) else {
                continue;
            };
            if tile.occupancy == Occupancy::Enemy {
                continue;
            }
            let Some(after_entry) = remaining.checked_sub(tile.cost.get()) else {
                continue;
            };
            let after_entry = if tile.stops_movement { 0 } else { after_entry };

            // Entry cost depends only on the destination. The max-heap visits
            // the predecessor with the most remaining movement first.
            reached.insert(next);
            if after_entry > 0 {
                frontier.push((after_entry, next));
            }
            if tile.occupancy == Occupancy::Empty {
                destinations.insert(next, after_entry);
            }
        }
    }

    destinations
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tile(cost: u32, occupancy: Occupancy) -> MovementTile {
        MovementTile {
            cost: NonZeroU32::new(cost).unwrap(),
            occupancy,
            stops_movement: false,
        }
    }

    #[test]
    fn keeps_more_movement_with_a_longer_cheaper_route() {
        let origin = Hex { x: 2, y: 2 };
        let high_cost = Hex { x: 2, y: 1 };
        let first_detour = Hex { x: 3, y: 1 };
        let second_detour = Hex { x: 3, y: 0 };
        let destination = Hex { x: 2, y: 0 };
        let exact_cost = Hex { x: 1, y: 2 };
        let tiles = BTreeMap::from([
            (high_cost, tile(3, Occupancy::Empty)),
            (first_detour, tile(1, Occupancy::Empty)),
            (second_detour, tile(1, Occupancy::Empty)),
            (destination, tile(1, Occupancy::Empty)),
            (exact_cost, tile(4, Occupancy::Empty)),
        ]);

        assert_eq!(
            reachable_destinations(origin, 4, |hex| tiles.get(&hex).copied()),
            BTreeMap::from([
                (high_cost, 1),
                (first_detour, 3),
                (second_detour, 2),
                (destination, 1),
                (exact_cost, 0),
            ])
        );
    }

    #[test]
    fn moves_through_allies_and_is_blocked_by_enemies() {
        let origin = Hex { x: 0, y: 0 };
        let ally = Hex { x: 0, y: 1 };
        let empty = Hex { x: 0, y: 2 };
        let enemy = Hex { x: 0, y: 3 };
        let behind_enemy = Hex { x: 0, y: 4 };
        let tiles = BTreeMap::from([
            (ally, tile(1, Occupancy::Ally)),
            (empty, tile(1, Occupancy::Empty)),
            (enemy, tile(1, Occupancy::Enemy)),
            (behind_enemy, tile(1, Occupancy::Empty)),
        ]);

        assert_eq!(
            reachable_destinations(origin, 4, |hex| tiles.get(&hex).copied()),
            BTreeMap::from([(empty, 2)])
        );
    }

    #[test]
    fn a_zone_of_control_consumes_remaining_movement_on_entry() {
        let origin = Hex { x: 0, y: 0 };
        let controlled = Hex { x: 0, y: 1 };
        let beyond = Hex { x: 0, y: 2 };
        let query = |hex: Hex, skirmisher: bool| {
            (hex == controlled || hex == beyond).then(|| MovementTile {
                stops_movement: hex == controlled && !skirmisher,
                ..tile(1, Occupancy::Empty)
            })
        };

        assert_eq!(
            reachable_destinations(origin, 3, |hex| query(hex, false)),
            BTreeMap::from([(controlled, 0)])
        );
        assert_eq!(
            reachable_destinations(origin, 3, |hex| query(hex, true)),
            BTreeMap::from([(controlled, 2), (beyond, 1)])
        );
    }
}
