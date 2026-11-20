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
    sorted(view, build_with_flags(catalog, view))
}

/// Standalone map export can use the map file's authored border. Gameplay only
/// supplies terrain that is present in the current side's observation.
pub(super) fn map_tiles(
    catalog: &Catalog,
    view: &Observation,
    map: &engine_lite::map::ScenarioMap,
) -> HashMap<Hex, Tile> {
    sorted(view, apply(catalog, view, |at| map.terrain_at(at)))
}

fn sorted(view: &Observation, mut tiles: HashMap<Hex, Tile>) -> HashMap<Hex, Tile> {
    tiles.retain(|hex, tile| {
        tile.layers.sort_by_key(|p| (p.layer, p.y));
        hex.x >= -1
            && hex.y >= -1
            && hex.x <= view.width
            && hex.y <= view.height
            && (!tile.layers.is_empty() || tile.flags.contains("base"))
    });
    tiles
}

pub(super) fn build_with_flags(catalog: &Catalog, view: &Observation) -> HashMap<Hex, Tile> {
    apply(catalog, view, |at| {
        (at.x >= -1 && at.y >= -1).then_some("_off^_usr")
    })
}

fn apply<'a>(
    catalog: &Catalog,
    view: &'a Observation,
    border: impl Fn(Hex) -> Option<&'a str>,
) -> HashMap<Hex, Tile> {
    let mut groups: BTreeMap<&str, Vec<Hex>> = BTreeMap::new();
    for tile in &view.tiles {
        groups.entry(&tile.terrain).or_default().push(tile.position);
    }
    for (x, y) in (-2..=view.width).flat_map(|x| (-2..=view.height).map(move |y| (x, y))) {
        if x < 0 || y < 0 || x >= view.width || y >= view.height {
            let at = Hex { x, y };
            groups
                .entry(border(at).unwrap_or(""))
                .or_default()
                .push(at);
        }
    }
    let groups: Vec<_> = groups
        .into_iter()
        .map(|(code, mut points)| {
            points.sort_unstable();
            (code, points)
        })
        .collect();
    let matches_empty: Vec<_> = catalog.filters.iter()
        .map(|filter| pattern::matches(filter, "")).collect();
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
            // The stock default theme installs an invisible base image for OFF_MAP_USER
            // before any WML rule. Authored holes can be inside the map rectangle.
            if groups[types[&hex]].0 == "_off^_usr" {
                tile.flags.insert("base".into());
            }
            tile.flags
                .insert(if inside { "_board" } else { "_border" }.into());
            (hex, tile)
        })
        .collect();
    // Only anchors through (width,height) enter the terrain index. Multi-hex
    // rules can still read and set flags one row and column beyond that index.
    for y in -2..=view.height + 1 {
        tiles.insert(Hex { x: view.width + 1, y }, Tile::default());
    }
    for x in -2..=view.width {
        tiles.insert(Hex { x, y: view.height + 1 }, Tile::default());
    }
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
                    let Some(tile) = tiles.get(&at) else {
                        return false;
                    };
                    let flags = &tile.flags;
                    types.get(&at).map_or(matches_empty[cell.filter], |&id| allowed[cell.filter][id])
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

#[cfg(test)]
mod tests {
    use super::*;
    use engine_lite::observation::ObservedTile;

    #[test]
    fn forest_can_use_an_empty_fringe_cell_but_cannot_match_an_unseen_playable_hex() {
        let rule = engine_lite::wml::parse(b"[tile]\nx,y=0,0\ntype=Gg^Fp\nset_flag=forest\n[/tile]\n[tile]\nx,y=0,1\ntype=*\nno_flag=_border\n[/tile]").unwrap();
        let catalog = super::super::compile::compile(&[rule], &[]);
        let map = engine_lite::map::ScenarioMap::parse("Gg,Gg,Gg\nGg,Gg,Gg\nGg,Gg^Fp,Gg").unwrap();
        let view = super::super::map_observation(&map);
        let border = Hex { x: 0, y: map.height() };
        assert!(apply(&catalog, &view, |at| map.terrain_at(at))[&border].flags.contains("forest"));

        let mut obscured = view;
        obscured.height = 2;
        obscured.tiles[0].terrain = "Gg^Fp".into();
        let origin = obscured.tiles[0].position;
        assert!(!build_with_flags(&catalog, &obscured)[&origin].flags.contains("forest"));
    }

    #[test]
    fn authored_offmap_inside_the_rectangle_keeps_the_theme_backdrop_visible() {
        let catalog = super::super::compile::compile(&[], &[]);
        let at = Hex { x: 1, y: 0 };
        let view = Observation {
            side: 1,
            allied_sides: vec![1],
            gold: None,
            width: 2,
            height: 1,
            units: vec![],
            tiles: vec![ObservedTile {
                position: at,
                terrain: "_off^_usr".into(),
                visible: true,
                village_owner: None,
                time_color: [0; 3],
            }],
        };
        let tiles = build(&catalog, &view);
        assert!(tiles[&at].flags.contains("base") && tiles[&at].flags.contains("_board"));
        assert!(tiles[&at].layers.is_empty());
        assert!(!tiles.contains_key(&Hex { x: 0, y: 0 }));
    }
}
