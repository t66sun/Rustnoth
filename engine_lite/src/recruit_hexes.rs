use std::collections::{HashSet, VecDeque};

use crate::{hex::Hex, map::ScenarioMap};

const LAST_VACANCY_SEARCH_STEP: i32 = 49;

/// Lists legal explicit recruit destinations for an eligible leader on a
/// fully visible map, in hex order.
///
/// The leader must be on a keep that belongs to the castle network. Occupied
/// castle tiles connect the network, while placement requires an empty tile.
/// The native recruit command first checks for a vacancy at most 49 steps away,
/// then accepts destinations at most the map's width plus height steps away.
pub fn destinations(
    leader: Hex,
    map: &ScenarioMap,
    is_keep: impl Fn(&str) -> bool,
    is_castle: impl Fn(&str) -> bool,
    occupied: impl Fn(Hex) -> bool,
) -> Vec<Hex> {
    let Some(terrain) = map.terrain_at(leader).filter(|_| map.is_playable(leader)) else {
        return Vec::new();
    };
    if !is_keep(terrain) || !is_castle(terrain) {
        return Vec::new();
    }

    let placement_limit = map.width() + map.height();
    let search_limit = placement_limit.max(LAST_VACANCY_SEARCH_STEP);
    let mut frontier = VecDeque::from([(leader, 0)]);
    let mut visited = HashSet::from([leader]);
    let mut has_vacancy = false;
    let mut destinations = Vec::new();

    while let Some((hex, steps)) = frontier.pop_front() {
        if hex != leader && !occupied(hex) {
            has_vacancy |= steps <= LAST_VACANCY_SEARCH_STEP;
            if steps <= placement_limit {
                destinations.push(hex);
            }
        }
        if steps == search_limit {
            continue;
        }
        for next in hex.neighbors().filter(|&hex| map.is_playable(hex)) {
            if visited.insert(next) && map.terrain_at(next).is_some_and(&is_castle) {
                frontier.push_back((next, steps + 1));
            }
        }
    }

    if !has_vacancy {
        return Vec::new();
    }
    destinations.sort_unstable();
    destinations
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recruit_hexes(map: &ScenarioMap, leader: Hex, occupied: impl Fn(Hex) -> bool) -> Vec<Hex> {
        destinations(
            leader,
            map,
            |terrain| terrain == "Kh",
            |terrain| matches!(terrain, "Kh" | "Ch"),
            occupied,
        )
    }

    #[test]
    fn crosses_occupied_castle_but_excludes_disconnected_castle_and_the_border() {
        let map = ScenarioMap::parse(
            "Ch,Ch,Ch,Ch,Ch,Ch,Ch\nCh,Kh,Ch,Ch,Gg,Ch,Ch\nCh,Ch,Ch,Ch,Ch,Ch,Ch",
        )
        .unwrap();
        assert_eq!(
            recruit_hexes(&map, Hex { x: 0, y: 0 }, |hex| hex.x == 1),
            [Hex { x: 2, y: 0 }]
        );
        assert!(recruit_hexes(&map, Hex { x: 1, y: 0 }, |_| false).is_empty());
    }

    #[test]
    fn accepts_a_detour_at_the_placement_limit_and_excludes_a_longer_one() {
        let map = ScenarioMap::parse(concat!(
            "Gg,Gg,Gg,Gg,Gg,Gg,Gg\n",
            "Gg,Kh,Ch,Ch,Ch,Ch,Gg\n",
            "Gg,Gg,Gg,Gg,Gg,Ch,Gg\n",
            "Gg,Ch,Ch,Ch,Ch,Ch,Gg\n",
            "Gg,Gg,Gg,Gg,Gg,Gg,Gg",
        ))
        .unwrap();
        let result = recruit_hexes(&map, Hex { x: 0, y: 0 }, |_| false);

        assert!(result.contains(&Hex { x: 1, y: 2 }));
        assert!(!result.contains(&Hex { x: 0, y: 2 }));
    }

    #[test]
    fn requires_one_vacancy_within_the_native_search_bound() {
        let border = vec!["Gg"; 53].join(",");
        let middle = format!("Gg,Kh,{},Gg", vec!["Ch"; 50].join(","));
        let map = ScenarioMap::parse(&format!("{border}\n{middle}\n{border}")).unwrap();
        let leader = Hex { x: 0, y: 0 };

        assert!(recruit_hexes(&map, leader, |hex| hex.x < 50).is_empty());
        assert_eq!(
            recruit_hexes(&map, leader, |hex| hex.x < 49),
            [Hex { x: 49, y: 0 }, Hex { x: 50, y: 0 }]
        );
    }
}
