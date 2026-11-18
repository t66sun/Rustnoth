use super::{
    Hex,
    compile::{Catalog, Cut, translate},
    pattern,
};
use crate::render::ground::noise;
use engine_lite::observation::Observation;
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Clone, Copy)]
pub(super) struct Layer {
    pub layer: i32,
    pub y: i32,
    pub image: usize,
    pub cut: Option<Cut>,
    pub seed: u32,
}

#[derive(Default)]
pub(super) struct Tile {
    pub layers: Vec<Layer>,
    pub flags: HashSet<String>,
}

/// Geometry and terrain come exclusively from the current observation. Unknown neighbors
/// have no terrain code which could match an authored road, shore or forest.
pub(super) fn build(catalog: &Catalog, view: &Observation) -> HashMap<Hex, Tile> {
    let mut tiles = build_with_flags(catalog, view);
    tiles.retain(|hex, tile| {
        tile.layers.sort_by_key(|p| (p.layer, p.y));
        hex.x >= -1
            && hex.y >= -1
            && hex.x <= view.width
            && hex.y <= view.height
            && !tile.layers.is_empty()
    });
    tiles
}

pub(super) fn build_with_flags(catalog: &Catalog, view: &Observation) -> HashMap<Hex, Tile> {
    let mut groups: BTreeMap<&str, Vec<Hex>> = BTreeMap::new();
    for tile in &view.tiles {
        groups.entry(&tile.terrain).or_default().push(tile.position);
    }
    for (x, y) in (-2..=view.width).flat_map(|x| (-2..=view.height).map(move |y| (x, y))) {
        if x < 0 || y < 0 || x >= view.width || y >= view.height {
            groups.entry("_off^_usr").or_default().push(Hex { x, y });
        }
    }
    let groups: Vec<_> = groups
        .into_iter()
        .map(|(code, mut points)| {
            points.sort_unstable();
            (code, points)
        })
        .collect();
    let types: HashMap<_, _> = groups
        .iter()
        .enumerate()
        .flat_map(|(id, (_, hexes))| hexes.iter().map(move |hex| (*hex, id)))
        .collect();
    let allowed: Vec<Vec<bool>> = catalog
        .filters
        .iter()
        .map(|filter| {
            groups
                .iter()
                .map(|(terrain, _)| pattern::matches(filter, terrain))
                .collect()
        })
        .collect();
    let counts: Vec<usize> = allowed
        .iter()
        .map(|accept| {
            groups
                .iter()
                .zip(accept)
                .filter_map(|((_, hexes), ok)| ok.then_some(hexes.len()))
                .sum()
        })
        .collect();
    let mut tiles: HashMap<Hex, Tile> = types
        .keys()
        .map(|&hex| {
            let inside = hex.x >= 0 && hex.y >= 0 && hex.x < view.width && hex.y < view.height;
            let mut tile = Tile::default();
            if !inside { tile.flags.insert("base".into()); }
            tile.flags
                .insert(if inside { "_board" } else { "_border" }.into());
            (hex, tile)
        })
        .collect();
    for rule in &catalog.rules {
        let Some(anchor) = rule.cells.iter().min_by_key(|c| counts[c.filter]) else {
            continue;
        };
        if counts[anchor.filter] == 0 {
            continue;
        }
        for ((_, positions), valid) in groups.iter().zip(&allowed[anchor.filter]) {
            if !valid {
                continue;
            }
            for pos in positions {
                let origin = translate(
                    *pos,
                    Hex {
                        x: -anchor.offset.x,
                        y: -anchor.offset.y,
                    },
                );
                if rule.location.is_some_and(|at| at != origin)
                    || (rule.modulo[0] > 0 && origin.x % rule.modulo[0] != 0)
                    || (rule.modulo[1] > 0 && origin.y % rule.modulo[1] != 0)
                {
                    continue;
                }
                let seed = noise(origin, rule.hash);
                if seed % 100 > rule.probability {
                    continue;
                }
                let matches = rule.cells.iter().all(|cell| {
                    let at = translate(origin, cell.offset);
                    let Some(&terrain) = types.get(&at) else {
                        return false;
                    };
                    let flags = &tiles[&at].flags;
                    allowed[cell.filter][terrain]
                        && cell.require.iter().all(|f| flags.contains(f))
                        && cell.forbid.iter().all(|f| !flags.contains(f))
                });
                if !matches {
                    continue;
                }
                for cell in &rule.cells {
                    let at = translate(origin, cell.offset);
                    let tile = tiles.get_mut(&at).unwrap();
                    tile.flags.extend(cell.set.iter().cloned());
                    tile.layers.extend(cell.parts.iter().map(|part| Layer {
                        layer: part.layer,
                        y: part.base_y,
                        image: part.image,
                        cut: part.cut,
                        seed,
                    }));
                }
            }
        }
    }
    tiles
}
