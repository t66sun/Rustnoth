use std::collections::BTreeMap;

use crate::{
    hex::{Direction, Hex},
    movement::{MovementTile, Occupancy},
};

struct Visit {
    spent: f64,
    moves_left: u32,
    estimate: f64,
    previous: Option<Hex>,
    open: bool,
}

fn heuristic(from: Hex, to: Hex) -> f64 {
    let dx = (f64::from(from.x) - f64::from(to.x)) * 0.75;
    let dy = f64::from(from.y) - f64::from(to.y)
        + f64::from((from.x & 1) - (to.x & 1)) * 0.5;
    from.distance(to) as f64 + (dx * dx + dy * dy) / 900_000_000.0
}

// Native A* updates entries in place. Its heap compares just the estimated
// cost, so equal-cost children and neighbor insertion order affect the route.
fn sift_up(open: &mut [Hex], mut index: usize, visits: &BTreeMap<Hex, Visit>) {
    let item = open[index];
    while index > 0 {
        let parent = (index - 1) / 2;
        if visits[&open[parent]].estimate <= visits[&item].estimate {
            break;
        }
        open[index] = open[parent];
        index = parent;
    }
    open[index] = item;
}

fn pop(open: &mut Vec<Hex>, visits: &BTreeMap<Hex, Visit>) -> Option<Hex> {
    let first = *open.first()?;
    let last = open.pop()?;
    if !open.is_empty() {
        let mut index = 0;
        while index * 2 + 1 < open.len() {
            let left = index * 2 + 1;
            let right = left + 1;
            let child = if right < open.len()
                && visits[&open[right]].estimate <= visits[&open[left]].estimate
            {
                right
            } else {
                left
            };
            open[index] = open[child];
            index = child;
        }
        open[index] = last;
        sift_up(open, index, visits);
    }
    Some(first)
}

pub(super) fn find(
    from: Hex,
    to: Hex,
    moves: u32,
    tile_at: impl Fn(Hex) -> Option<(MovementTile, u8)>,
) -> Option<Vec<(Hex, u32)>> {
    if from == to || tile_at(to)?.0.occupancy != Occupancy::Empty {
        return None;
    }
    let mut visits = BTreeMap::from([(from, Visit {
        spent: 0.0,
        moves_left: moves,
        estimate: heuristic(from, to),
        previous: None,
        open: true,
    })]);
    let mut open = vec![from];
    let mut best = f64::INFINITY;
    while let Some(current) = pop(&mut open, &visits) {
        let visit = visits.get_mut(&current).expect("open node was visited");
        visit.open = false;
        if visit.estimate >= best {
            break;
        }
        let spent = visit.spent;
        let remaining = visit.moves_left;
        for direction in Direction::CLOCKWISE.into_iter().rev() {
            let Some(next) = current.neighbor(direction) else { continue };
            let old = visits.get(&next);
            let threshold = old.map_or(best, |node| node.spent);
            if spent + 1.0 >= threshold {
                continue;
            }
            let Some((tile, hit_chance)) = tile_at(next) else { continue };
            let cost = tile.cost.get();
            if tile.occupancy == Occupancy::Enemy || cost >= 99 || cost > remaining {
                continue;
            }
            let movement = if tile.stops_movement { remaining } else { cost };
            let penalty = u32::from(hit_chance) + u32::from(tile.occupancy == Occupancy::Ally);
            let added = f64::from(movement) + f64::from(penalty) / 10_000.0;
            let candidate = spent + added;
            if candidate >= threshold || candidate as u32 > moves {
                continue;
            }
            let was_open = old.is_some_and(|node| node.open);
            visits.insert(next, Visit {
                spent: candidate,
                moves_left: remaining - movement,
                estimate: candidate + heuristic(next, to),
                previous: Some(current),
                open: true,
            });
            if next == to {
                best = candidate;
            }
            let index = if was_open {
                open.iter().position(|hex| *hex == next).expect("queued visit")
            } else {
                open.push(next);
                open.len() - 1
            };
            sift_up(&mut open, index, &visits);
        }
    }
    let mut route = Vec::new();
    let mut current = to;
    loop {
        let visit = visits.get(&current)?;
        route.push((current, visit.moves_left));
        let Some(previous) = visit.previous else { break };
        current = previous;
    }
    route.reverse();
    Some(route)
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use super::*;

    #[test]
    fn tied_forks_use_native_hex_order_then_prefer_safer_or_unoccupied_terrain() {
        let from = Hex { x: 0, y: 1 };
        let north = Hex { x: 1, y: 0 };
        let south = Hex { x: 1, y: 1 };
        let to = Hex { x: 2, y: 1 };
        let fork = |south_hit_chance, south_occupancy| find(from, to, 2, |hex| {
            if ![north, south, to].contains(&hex) {
                return None;
            }
            let occupancy = if hex == south { south_occupancy } else { Occupancy::Empty };
            let hit_chance = if hex == south { south_hit_chance } else { 50 };
            Some((MovementTile { cost: NonZeroU32::MIN, occupancy, stops_movement: false }, hit_chance))
        }).unwrap();
        assert_eq!(fork(50, Occupancy::Empty), [(from, 2), (south, 1), (to, 0)]);
        assert_eq!(fork(60, Occupancy::Empty), [(from, 2), (north, 1), (to, 0)]);
        assert_eq!(fork(50, Occupancy::Ally), [(from, 2), (north, 1), (to, 0)]);
    }
}
