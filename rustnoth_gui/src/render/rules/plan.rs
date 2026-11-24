use super::{
    Hex,
    compile::{Catalog, Cut, Flag, translate},
    pattern,
};
use crate::render::ground::noise;
use engine_lite::observation::Observation;
use std::{collections::HashMap, num::NonZeroUsize};

#[derive(Clone, Copy)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
pub(super) struct Layer {
    pub layer: i32,
    pub y: i32,
    pub image: usize,
    pub cut: Option<Cut>,
    pub seed: u32,
}

#[derive(Default)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
pub(super) struct Tile {
    pub layers: Vec<Layer>,
    flags: Vec<Flag>,
    // One-based index into the terrain groups. The outer fringe is unindexed.
    terrain_group: Option<NonZeroUsize>,
}

impl Tile {
    pub(super) fn has_flag(&self, flag: Flag) -> bool {
        self.flags.binary_search(&flag).is_ok()
    }

    fn set_flag(&mut self, flag: Flag) {
        if let Err(index) = self.flags.binary_search(&flag) {
            self.flags.insert(index, flag);
        }
    }
}

struct Grid {
    width: usize,
    height: usize,
}

impl Grid {
    fn index(&self, hex: Hex) -> Option<usize> {
        // The planner includes two cells west/north and east/south of the board.
        let x = usize::try_from(i64::from(hex.x) + 2).ok()?;
        let y = usize::try_from(i64::from(hex.y) + 2).ok()?;
        if x < self.width && y < self.height {
            Some(y * self.width + x)
        } else {
            None
        }
    }

    fn position(&self, index: usize) -> Hex {
        Hex {
            x: ((index % self.width) as i64 - 2) as i32,
            y: ((index / self.width) as i64 - 2) as i32,
        }
    }
}

enum RuleTiles {
    Dense { grid: Grid, slots: Vec<Option<Tile>> },
    Sparse(HashMap<Hex, Tile>),
}

impl RuleTiles {
    fn for_view(view: &Observation) -> Self {
        Self::dense(view).unwrap_or_else(|| Self::Sparse(HashMap::new()))
    }

    fn dense(view: &Observation) -> Option<Self> {
        let width = usize::try_from(view.width).ok()?;
        let height = usize::try_from(view.height).ok()?;
        let playable = width.checked_mul(height)?;
        // Reserve a full grid once at least three quarters of its playable cells are known.
        if view.tiles.len() < playable - playable / 4 {
            return None;
        }
        let grid = Grid { width: width.checked_add(4)?, height: height.checked_add(4)? };
        let capacity = grid.width.checked_mul(grid.height)?;
        if view.tiles.iter().any(|tile| grid.index(tile.position).is_none()) {
            return None;
        }
        let slots = std::iter::repeat_with(|| None).take(capacity).collect();
        Some(Self::Dense { grid, slots })
    }

    fn insert(&mut self, hex: Hex, tile: Tile) {
        match self {
            Self::Dense { grid, slots } => slots[grid.index(hex).unwrap()] = Some(tile),
            Self::Sparse(tiles) => { tiles.insert(hex, tile); }
        }
    }

    fn clear_fringe_flags(&mut self, hex: Hex) {
        let tile = match self {
            Self::Dense { grid, slots } => slots[grid.index(hex).unwrap()].get_or_insert_with(Tile::default),
            Self::Sparse(tiles) => tiles.entry(hex).or_default(),
        };
        tile.flags.clear();
    }

    fn get(&self, hex: Hex) -> Option<&Tile> {
        match self {
            Self::Dense { grid, slots } => slots[grid.index(hex)?].as_ref(),
            Self::Sparse(tiles) => tiles.get(&hex),
        }
    }

    fn get_mut(&mut self, hex: Hex) -> Option<&mut Tile> {
        match self {
            Self::Dense { grid, slots } => slots[grid.index(hex)?].as_mut(),
            Self::Sparse(tiles) => tiles.get_mut(&hex),
        }
    }

    fn into_map(self) -> HashMap<Hex, Tile> {
        match self {
            Self::Dense { grid, slots } => slots.into_iter().enumerate()
                .filter_map(|(index, slot)| slot.map(|tile| (grid.position(index), tile)))
                .collect(),
            Self::Sparse(tiles) => tiles,
        }
    }
}

/// Playable terrain comes only from the observation. The referee separately
/// projects explored border terrain for the display.
pub(super) fn battlefield(catalog: &Catalog, view: &Observation, border: &engine_lite::map::KnownBorder<'_>) -> HashMap<Hex, Tile> {
    sorted(view, build_with_flags(catalog, view, border))
}

/// Standalone map export and the editor use the entire authored border.
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
            && (!tile.layers.is_empty() || tile.has_flag(Flag::BASE))
    });
    tiles
}

pub(super) fn build_with_flags(catalog: &Catalog, view: &Observation, border: &engine_lite::map::KnownBorder<'_>) -> HashMap<Hex, Tile> {
    apply(catalog, view, |at| {
        border.get(&at).map(|&(code, _)| code)
            .or_else(|| (at.x >= -1 && at.y >= -1).then_some("_off^_usr"))
    })
}

fn fringe_positions(width: i32, height: i32) -> impl Iterator<Item = Hex> {
    (-2..=width).flat_map(move |x| {
        let side_column = x < 0 || x == width;
        let north_end = if side_column { height } else { height.min(-1) };
        let south = (!side_column && height >= 0).then_some(height);
        (-2..=north_end).chain(south).map(move |y| Hex { x, y })
    })
}

fn apply<'a>(
    catalog: &Catalog,
    view: &'a Observation,
    border: impl Fn(Hex) -> Option<&'a str>,
) -> HashMap<Hex, Tile> {
    apply_with_tiles(catalog, view, border, RuleTiles::for_view(view))
}

fn apply_with_tiles<'a>(
    catalog: &Catalog,
    view: &'a Observation,
    border: impl Fn(Hex) -> Option<&'a str>,
    mut tiles: RuleTiles,
) -> HashMap<Hex, Tile> {
    let mut groups: HashMap<&str, Vec<Hex>> = HashMap::new();
    for tile in &view.tiles {
        groups.entry(&tile.terrain).or_default().push(tile.position);
    }
    for at in fringe_positions(view.width, view.height) {
        groups.entry(border(at).unwrap_or("")).or_default().push(at);
    }
    let mut groups: Vec<_> = groups
        .into_iter()
        .map(|(code, mut points)| {
            points.sort_unstable();
            (code, points)
        })
        .collect();
    // The native terrain_by_type map compares base and overlay separately. Its
    // missing-overlay sentinel (0xffffffff) sorts after every authored overlay.
    groups.sort_unstable_by_key(|&(code, _)| match code.split_once('^') {
        Some((base, overlay)) => (base, false, overlay),
        None => (code, true, ""),
    });
    let matches_empty: Vec<_> = catalog.filters.iter()
        .map(|filter| pattern::matches(filter, "")).collect();
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
    for (index, (_, hexes)) in groups.iter().enumerate() {
        let terrain_group = NonZeroUsize::new(index + 1);
        for &hex in hexes {
            let inside = hex.x >= 0 && hex.y >= 0 && hex.x < view.width && hex.y < view.height;
            let mut tile = Tile { terrain_group, ..Tile::default() };
            tile.set_flag(if inside { Flag::BOARD } else { Flag::BORDER });
            tiles.insert(hex, tile);
        }
    }
    // Only anchors through (width,height) enter the terrain index. Multi-hex
    // rules can still read and set flags one row and column beyond that index.
    for y in -2..=view.height + 1 {
        tiles.clear_fringe_flags(Hex { x: view.width + 1, y });
    }
    for x in -2..=view.width {
        tiles.clear_fringe_flags(Hex { x, y: view.height + 1 });
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
                let Some(seed) = winning_seed(rule.probability, || noise(origin, rule.hash), || {
                    rule.cells.iter().all(|cell| {
                        let at = translate(origin, cell.offset);
                        let Some(tile) = tiles.get(at) else {
                            return false;
                        };
                        let terrain_matches = tile.terrain_group.map_or(matches_empty[cell.filter], |group| {
                            allowed[cell.filter][group.get() - 1]
                        });
                        terrain_matches && cell.require.iter().all(|&flag| tile.has_flag(flag))
                            && cell.forbid.iter().all(|&flag| !tile.has_flag(flag))
                    })
                }) else {
                    continue;
                };
                for cell in &rule.cells {
                    let at = translate(origin, cell.offset);
                    let tile = tiles.get_mut(at).unwrap();
                    for &flag in &cell.set {
                        tile.set_flag(flag);
                    }
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
    tiles.into_map()
}

fn winning_seed(probability: u32, noise: impl FnOnce() -> u32, matches: impl FnOnce() -> bool) -> Option<u32> {
    if probability >= 99 {
        // Every roll passes. The seed is only needed to choose artwork for a match.
        return matches().then(noise);
    }
    let seed = noise();
    (seed % 100 <= probability && matches()).then_some(seed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_lite::{observation::ObservedTile, wml};

    #[test]
    fn dense_terrain_planning_matches_sparse_layers_flags_and_fringe() {
        let rule = wml::parse(br#"
[tile]
x,y=0,0
type=Gg
set_flag=joined
[image]
name=grass/green.png
[/image]
[/tile]
[tile]
x,y=0,1
type=Gs
[/tile]
"#).unwrap();
        let catalog = super::super::compile::compile_files(&[rule], HashMap::from([
            ("grass/green.png".into(), "grass/green.png".into()),
        ]));
        let mut view = Observation {
            side: 1,
            allied_sides: vec![1],
            gold: None,
            width: 3,
            height: 2,
            tiles: vec![],
            units: vec![],
        };
        let hidden = Hex { x: 1, y: 1 };
        for (x, y, terrain) in [(0, 0, "Gg"), (1, 0, "Gg"), (2, 0, "Gg"), (0, 1, "Gs"), (2, 1, "Gg")] {
            view.tiles.push(ObservedTile {
                position: Hex { x, y },
                terrain: terrain.into(),
                visible: true,
                village_owner: None,
                time_color: [0; 3],
            });
        }

        let border = |at| (at == (Hex { x: 3, y: 1 })).then_some("Gs");
        assert!(matches!(RuleTiles::for_view(&view), RuleTiles::Dense { .. }));
        let dense = apply(&catalog, &view, border);
        let sparse = apply_with_tiles(&catalog, &view, border, RuleTiles::Sparse(HashMap::new()));
        assert_eq!(dense, sparse);
        let origin = &dense[&Hex { x: 0, y: 0 }];
        assert!(origin.has_flag(catalog.flag("joined")));
        assert_eq!(origin.layers.len(), 1);
        assert!(!dense.contains_key(&hidden));
        assert!(dense[&Hex { x: -2, y: -2 }].has_flag(Flag::BORDER));
        assert!(dense[&Hex { x: 4, y: 3 }].terrain_group.is_none());

        view.tiles.truncate(2);
        assert!(matches!(RuleTiles::for_view(&view), RuleTiles::Sparse(_)));
    }

    #[test]
    fn terrain_fringe_preserves_column_order_and_scales_with_the_perimeter() {
        for width in 0..=4 {
            for height in 0..=4 {
                let reference = (-2..=width)
                    .flat_map(|x| (-2..=height).map(move |y| Hex { x, y }))
                    .filter(|at| at.x < 0 || at.y < 0 || at.x >= width || at.y >= height);
                assert!(fringe_positions(width, height).eq(reference));
            }
        }
        assert_eq!(fringe_positions(10_000, 8_000).count(), 54_009);
    }

    #[test]
    fn artwork_seeds_are_deferred_for_certain_rules_but_rare_rules_still_reject_early() {
        assert_eq!(winning_seed(100, || panic!("nonmatching tile needs no seed"), || false), None);
        assert_eq!(winning_seed(99, || 199, || true), Some(199));
        assert_eq!(winning_seed(20, || 120, || true), Some(120));
        assert_eq!(winning_seed(20, || 121, || panic!("probability rejected tile")), None);
    }

    #[test]
    fn overlapping_terrain_rules_claim_overlay_anchors_before_bare_anchors() {
        let rule = wml::parse(
            br#"
[terrain_graphics]
[tile]
x,y=0,0
type=Gg
set_no_flag=claimed
set_flag=origin
[/tile]
[tile]
x,y=0,1
type=Gg
set_no_flag=claimed
[/tile]
[/terrain_graphics]
"#,
        )
        .unwrap();
        let catalog = super::super::compile::compile(&[rule.children[0].1.clone()], &[]);
        let map = engine_lite::map::ScenarioMap::parse(
            "Xv,Xv,Xv\nXv,Gg,Xv\nXv,Gg^Fds,Xv\nXv,Gg,Xv\nXv,Xv,Xv",
        )
        .unwrap();
        let view = super::super::map_observation(&map);
        let tiles = build_with_flags(&catalog, &view, &Default::default());
        let claimed = catalog.flag("claimed");
        assert!(tiles[&Hex { x: 0, y: 1 }].has_flag(catalog.flag("origin")));
        assert!(tiles[&Hex { x: 0, y: 2 }].has_flag(claimed));
        assert!(!tiles[&Hex { x: 0, y: 0 }].has_flag(claimed));
    }

    #[test]
    fn forest_can_use_an_empty_fringe_cell_but_cannot_match_an_unseen_playable_hex() {
        let rule = engine_lite::wml::parse(b"[tile]\nx,y=0,0\ntype=Gg^Fp\nset_flag=forest\n[/tile]\n[tile]\nx,y=0,1\ntype=*\nno_flag=_border\n[/tile]").unwrap();
        let catalog = super::super::compile::compile(&[rule], &[]);
        let map = engine_lite::map::ScenarioMap::parse("Gg,Gg,Gg\nGg,Gg,Gg\nGg,Gg^Fp,Gg").unwrap();
        let view = super::super::map_observation(&map);
        let border = Hex { x: 0, y: map.height() };
        let forest = catalog.flag("forest");
        assert!(apply(&catalog, &view, |at| map.terrain_at(at))[&border].has_flag(forest));
        let explored = [(border, ("Gg^Fp", false))].into_iter().collect();
        let tiles = build_with_flags(&catalog, &view, &explored);
        assert!(tiles[&border].has_flag(forest));
        assert!(tiles[&border].terrain_group.is_some());
        let outer_fringe = Hex { x: view.width + 1, y: 0 };
        assert!(tiles[&outer_fringe].terrain_group.is_none());
        assert!(!build_with_flags(&catalog, &view, &Default::default())[&border].has_flag(forest));

        let mut obscured = view;
        obscured.height = 2;
        obscured.tiles[0].terrain = "Gg^Fp".into();
        let origin = obscured.tiles[0].position;
        let hidden = translate(origin, Hex { x: 0, y: 1 });
        obscured.tiles.retain(|tile| tile.position != hidden);
        let tiles = build_with_flags(&catalog, &obscured, &Default::default());
        assert!(!tiles.contains_key(&hidden));
        assert!(!tiles[&origin].has_flag(forest));
    }

    #[test]
    fn authored_offmap_inside_the_rectangle_uses_the_themes_textured_hex() {
        let catalog = super::super::compile::compile_files(&[], HashMap::from([
            ("off-map/alpha.png".to_owned(), "off-map/alpha.png".into())
        ]));
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
        let tiles = battlefield(&catalog, &view, &Default::default());
        assert!(tiles[&at].has_flag(catalog.flag("base")) && tiles[&at].has_flag(catalog.flag("_board")));
        let layers = &tiles[&at].layers;
        assert_eq!(layers.len(), 1);
        assert_eq!(layers[0].layer, -1000);
        let frame = catalog.images[layers[0].image].variants[0].choices[0][0].0;
        assert_eq!(catalog.names[frame], "off-map/alpha.png");
        assert!(!tiles.contains_key(&Hex { x: 0, y: 0 }));
    }

    #[test]
    fn matched_tiles_and_image_variants_share_compact_flag_ids() {
        let rule = wml::parse(
            br#"
[tile]
x,y=0,0
type=Gg
set_flag=track,track
[image]
name=grass/green.png
    [variant]
    name=grass/tracked.png
    has_flag=track,_board
    [/variant]
[/image]
[/tile]
"#,
        )
        .unwrap();
        let catalog = super::super::compile::compile_files(
            &[rule],
            ["grass/green.png", "grass/tracked.png"]
                .into_iter()
                .map(|name| (name.into(), name.into()))
                .collect(),
        );
        let map = engine_lite::map::ScenarioMap::parse("Gg,Gg,Gg,Gg\nGg,Gg,Gg,Gg\nGg,Gg,Gg,Gg").unwrap();
        let view = super::super::map_observation(&map);
        let tiles = build_with_flags(&catalog, &view, &Default::default());
        let track = catalog.flag("track");
        for x in 0..2 {
            let tile = &tiles[&Hex { x, y: 0 }];
            assert_eq!(tile.flags, [Flag::BOARD, track]);
            let variants = &catalog.images[tile.layers[0].image].variants;
            assert_eq!(variants[0].flags, [track, Flag::BOARD]);
            assert!(variants[1].flags.is_empty());
        }
        assert_eq!(std::mem::size_of_val(&track), std::mem::size_of::<usize>());
    }
}
