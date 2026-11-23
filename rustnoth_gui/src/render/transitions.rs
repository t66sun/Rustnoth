//! Original grass blends and water banks from the stock terrain-graphics.cfg.
use std::{collections::HashMap, path::PathBuf};

use engine_lite::{hex::{Direction, Hex}, observation::Observation};
use macroquad::prelude::{Texture2D, Vec2};
use rustnoth_gui::assets::AssetCatalog;

const DIRECTIONS: [&str; 6] = ["n", "ne", "se", "s", "sw", "nw"];
// Inside blends are applied in this order. The outer blends run in reverse.
const GRASS: [(&str, &str, Option<&str>); 4] = [
    ("Gs", "grass/semi-dry-long", Some("grass/semi-dry-abrupt")),
    ("Gg", "grass/green-long", Some("grass/green-abrupt")),
    ("Gd", "grass/dry-long", Some("grass/dry-abrupt")),
    ("Gll", "grass/leaf-litter-long", None),
];

#[derive(Clone, Copy, PartialEq)]
enum Terrain {
    Grass(usize),
    Water,
}

fn terrain(code: &str) -> Option<Terrain> {
    let (base, overlay) = code.split_once('^').unwrap_or((code, ""));
    // Height overlays have their own elevated coast rules.
    if overlay.starts_with("Qh") { return None; }
    if base.starts_with('W') { return Some(Terrain::Water); }
    GRASS.iter().position(|(id, _, _)| *id == base).map(Terrain::Grass)
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct Edge {
    stem: &'static str,
    dir: usize,
    pair: bool,
}

impl Edge {
    fn path(self) -> PathBuf {
        let dir = DIRECTIONS[self.dir];
        if self.pair {
            let next = DIRECTIONS[(self.dir + 1) % 6];
            format!("terrain/{}-{dir}-{next}.png", self.stem).into()
        } else {
            format!("terrain/{}-{dir}.png", self.stem).into()
        }
    }
}

fn edges(target: Terrain, adjacent: [Option<Terrain>; 6]) -> Vec<Edge> {
    let mut result = Vec::new();
    match target {
        Terrain::Water => {
            // NEW:TRANSITION tries multi-edge sprites before any single edge.
            // flat/bank-to-ice supplies all six pairs and singles at layer -483.
            let mut banks = adjacent.map(|neighbor| matches!(neighbor, Some(Terrain::Grass(_))));
            for dir in 0..6 {
                let next = (dir + 1) % 6;
                if banks[dir] && banks[next] {
                    result.push(Edge { stem: "flat/bank-to-ice", dir, pair: true });
                    banks[dir] = false;
                    banks[next] = false;
                }
            }
            for (dir, single) in banks.into_iter().enumerate() {
                if single { result.push(Edge { stem: "flat/bank-to-ice", dir, pair: false }); }
            }
            for source in [2, 0, 1] { // Dry, semi-dry, green at layers -273 through -271.
                for (dir, neighbor) in adjacent.iter().enumerate() {
                    if *neighbor == Some(Terrain::Grass(source)) {
                        result.push(Edge { stem: GRASS[source].2.unwrap(), dir, pair: false });
                    }
                }
            }
        }
        Terrain::Grass(target) => {
            let mut layers = Vec::new();
            for (dir, neighbor) in adjacent.into_iter().enumerate() {
                match neighbor {
                    Some(Terrain::Grass(source)) if source != target => {
                        let layer = if source < target { -250 - source as i32 } else { -257 + source as i32 };
                        layers.push((layer, Edge { stem: GRASS[source].1, dir, pair: false }));
                    }
                    Some(Terrain::Water) if GRASS[target].0 == "Gll" => {
                        layers.push((-300, Edge { stem: "flat/bank", dir, pair: false }));
                    }
                    _ => {}
                }
            }
            layers.sort_by_key(|(layer, _)| *layer);
            result.extend(layers.into_iter().map(|(_, edge)| edge));
        }
    }
    result
}

fn plan(view: &Observation) -> impl Iterator<Item = (Hex, Vec<Edge>)> {
    let known: HashMap<_, _> = view.tiles.iter().filter_map(|tile|
        Some((tile.position, terrain(&tile.terrain)?))).collect();
    view.tiles.iter().filter_map(move |tile| {
        let &target = known.get(&tile.position)?;
        let adjacent = Direction::CLOCKWISE.map(|direction|
            tile.position.neighbor(direction).and_then(|hex| known.get(&hex).copied()));
        let edges = edges(target, adjacent);
        (!edges.is_empty()).then_some((tile.position, edges))
    })
}

#[derive(Default)]
pub(super) struct Transitions {
    textures: HashMap<Edge, Option<Texture2D>>,
    tiles: HashMap<Hex, Vec<Edge>>,
}

impl Transitions {
    pub(super) fn texture_extent(&self) -> Vec2 {
        super::texture_extent(self.textures.values().flatten())
    }

    pub(super) fn load(&mut self, view: &Observation, assets: &AssetCatalog) {
        self.tiles.clear();
        for (hex, edges) in plan(view) {
            for &edge in &edges {
                self.textures.entry(edge).or_insert_with(|| {
                    assets.resolve_image(&edge.path()).and_then(|path| super::load_texture(&path, None))
                });
            }
            self.tiles.insert(hex, edges);
        }
    }

    pub(super) fn draw(&self, hex: Hex, center: Vec2, size: f32, viewport: super::Viewport) {
        if let Some(edges) = self.tiles.get(&hex) {
            for texture in edges.iter().filter_map(|edge| self.textures[edge].as_ref()) {
                super::draw_centered(texture, center, size, Some(viewport));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_lite::observation::ObservedTile;

    #[test]
    fn visible_map_uses_the_source_facing_direction_and_native_layer_order() {
        let water = Hex { x: 1, y: 1 };
        let north = water.neighbor(Direction::North).unwrap();
        let northeast = water.neighbor(Direction::NorthEast).unwrap();
        let tile = |position, terrain: &str| ObservedTile {
            position, terrain: terrain.into(), visible: true, village_owner: None, time_color: [0; 3],
        };
        let mut view = Observation {
            side: 1, allied_sides: vec![1], gold: None, width: 4, height: 4, units: vec![],
            tiles: vec![tile(water, "Ww"), tile(north, "Gg^Fp"), tile(northeast, "Gd")],
        };
        let paths = |view: &Observation, position| plan(view).find(|(hex, _)| *hex == position)
            .map(|(_, edges)| edges.into_iter().map(Edge::path).collect::<Vec<_>>()).unwrap_or_default();
        assert_eq!(paths(&view, water), [
            PathBuf::from("terrain/flat/bank-to-ice-n-ne.png"),
            PathBuf::from("terrain/grass/dry-abrupt-ne.png"),
            PathBuf::from("terrain/grass/green-abrupt-n.png"),
        ]);
        assert_eq!(paths(&view, north), [PathBuf::from("terrain/grass/dry-long-se.png")]);
        assert_eq!(paths(&view, northeast), [PathBuf::from("terrain/grass/green-long-nw.png")]);

        // Remove the shrouded neighbor entirely, as the referee does.
        view.tiles.pop();
        assert_eq!(paths(&view, water), [
            PathBuf::from("terrain/flat/bank-to-ice-n.png"),
            PathBuf::from("terrain/grass/green-abrupt-n.png"),
        ]);
        assert!(paths(&view, north).is_empty());
    }
}

