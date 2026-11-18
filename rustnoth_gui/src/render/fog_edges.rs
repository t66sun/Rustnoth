//! Directional visibility overlays from display::get_fog_shroud_images in Wesnoth 1.18.8.
use super::{BoardLayout, Cover, Hex, Observation, draw_centered, load_texture};
use macroquad::prelude::{Texture2D, vec2};
use std::{collections::HashMap, fs, path::Path};

const DIRECTIONS: [&str; 6] = ["n", "ne", "se", "s", "sw", "nw"];
const ALL: u8 = 0b11_1111;

type Variants = [Vec<Texture2D>; 64];

/// The native assets provide clockwise runs of adjacent edges. Extend each
/// filename one direction at a time and draw the longest available prefix.
fn select<T: Clone>(mask: u8, art: &HashMap<String, T>) -> Vec<T> {
    let mut result = Vec::new();
    if mask == ALL
        && let Some(all) = art.get("all")
    {
        return vec![all.clone()];
    }
    let marked = |direction: usize| mask & (1 << direction) != 0;
    let start = (0..6).find(|&direction| !marked(direction)).unwrap_or(0);
    let mut direction = (start + 1) % 6;
    // The two six-step bounds also cover an enclosed hex with no "-all" image.
    for _ in 0..6 {
        if direction == start {
            break;
        }
        if !marked(direction) {
            direction = (direction + 1) % 6;
            continue;
        }
        let mut candidate = String::new();
        let mut longest = None;
        for _ in 0..6 {
            if !marked(direction) {
                break;
            }
            if !candidate.is_empty() {
                candidate.push('-');
            }
            candidate.push_str(DIRECTIONS[direction]);
            if let Some(sprite) = art.get(&candidate) {
                longest = Some(sprite);
                direction = (direction + 1) % 6;
            } else {
                if longest.is_none() {
                    direction = (direction + 1) % 6;
                }
                break;
            }
        }
        if let Some(sprite) = longest {
            result.push(sprite.clone());
        }
    }
    result
}

fn load_variants(terrain: &Path, stem: &str) -> Variants {
    let mut art = HashMap::new();
    let prefix = format!("{stem}-");
    if let Ok(entries) = fs::read_dir(terrain.join(stem)) {
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(key) = path
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.strip_prefix(&prefix))
                .and_then(|n| n.strip_suffix(".png"))
            else {
                continue;
            };
            if (key == "all" || key.split('-').all(|d| DIRECTIONS.contains(&d)))
                && let Some(texture) = load_texture(&path, None)
            {
                art.insert(key.to_owned(), texture);
            }
        }
    }
    std::array::from_fn(|mask| select(mask as u8, &art))
}

#[derive(Debug, PartialEq, Eq)]
struct Boundary {
    at: Hex,
    fog: u8,
    shroud: u8,
}

fn boundaries(view: &Observation, cover: &[(Hex, Cover)]) -> Vec<Boundary> {
    if cover.is_empty() {
        return Vec::new();
    }
    let covered: HashMap<_, _> = cover.iter().map(|(at, kind)| (*at, kind)).collect();
    view.tiles
        .iter()
        .filter_map(|tile| {
            let mut fog = 0;
            let mut shroud = 0;
            // Explored tiles are the only possible receivers. Off-board coordinates
            // are absent from the bounded cover map and contribute no overlap.
            for (i, neighbor) in tile.position.neighbors().enumerate() {
                match covered.get(&neighbor) {
                    Some(Cover::Shroud) => shroud |= 1 << i,
                    Some(Cover::Fog) if tile.visible => fog |= 1 << i,
                    _ => {}
                }
            }
            (fog != 0 || shroud != 0).then_some(Boundary {
                at: tile.position,
                fog,
                shroud,
            })
        })
        .collect()
}

pub(super) struct Edges {
    fog: Variants,
    shroud: Variants,
    tiles: Vec<Boundary>,
}

impl Edges {
    pub(super) fn load(terrain: &Path) -> Self {
        Self {
            fog: load_variants(terrain, "fog"),
            shroud: load_variants(terrain, "void"),
            tiles: Vec::new(),
        }
    }

    pub(super) fn update(&mut self, view: &Observation, cover: &[(Hex, Cover)]) {
        self.tiles = boundaries(view, cover);
    }

    pub(super) fn draw(&self, layout: &BoardLayout) {
        let size = layout.tile_size();
        for tile in &self.tiles {
            let [x, y] = layout.tile_origin(tile.at);
            let center = vec2(x + size / 2.0, y + size / 2.0);
            for texture in self.fog[tile.fog as usize]
                .iter()
                .chain(&self.shroud[tile.shroud as usize])
            {
                draw_centered(texture, center, size);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::map_cover;
    use engine_lite::observation::ObservedTile;

    #[test]
    fn selects_longest_clockwise_runs_including_northwest_wrap_and_surrounded_art() {
        let art: HashMap<_, _> = ["n", "s", "sw", "nw", "nw-n", "nw-n-ne", "all"]
            .into_iter()
            .map(|id| (id.to_owned(), id))
            .collect();
        assert_eq!(select(0b10_1011, &art), ["s", "nw-n-ne"]);
        assert_eq!(select(0b11_0001, &art), ["sw", "nw-n"]);
        assert_eq!(select(ALL, &art), ["all"]);
    }

    #[test]
    fn fog_and_shroud_edges_are_derived_only_from_explored_cells() {
        let tile = |x, y, visible| ObservedTile {
            position: Hex { x, y },
            terrain: "Gg".into(),
            visible,
            village_owner: None,
            time_color: [0; 3],
        };
        let view = Observation {
            side: 1,
            allied_sides: vec![1],
            gold: None,
            width: 2,
            height: 2,
            tiles: vec![tile(0, 0, true), tile(1, 0, false), tile(1, 1, false)],
            units: vec![],
        };
        let result = boundaries(&view, &map_cover(&view));
        assert_eq!(
            result,
            [
                Boundary {
                    at: Hex { x: 0, y: 0 },
                    fog: 1 << 2,
                    shroud: 1 << 3
                },
                Boundary {
                    at: Hex { x: 1, y: 0 },
                    fog: 0,
                    shroud: 1 << 4
                },
                Boundary {
                    at: Hex { x: 1, y: 1 },
                    fog: 0,
                    shroud: 1 << 5
                },
            ]
        );
    }
}
