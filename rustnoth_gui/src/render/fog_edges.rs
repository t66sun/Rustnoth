//! Directional visibility overlays from display::get_fog_shroud_images in Wesnoth 1.18.8.
use super::{
    BoardLayout, Cover, Hex, Observation, Viewport,
    atlas::{Atlas, AtlasSprite},
    recolored_pixels, sprite_rect,
};
use macroquad::prelude::{Vec2, WHITE, vec2};
use std::{collections::HashMap, fs, path::Path};

const DIRECTIONS: [&str; 6] = ["n", "ne", "se", "s", "sw", "nw"];
const ALL: u8 = 0b11_1111;
// One page fits the 46 stock 72px masks and their sampling borders.
const VISIBILITY_PAGE_SIDE: u16 = 576;

type Variants<T> = [Vec<T>; 64];

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

fn load_variants<T: Clone>(
    terrain: &Path,
    stem: &str,
    upload: &mut impl FnMut(&Path) -> Option<T>,
) -> Variants<T> {
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
                && let Some(texture) = upload(&path)
            {
                art.insert(key.to_owned(), texture);
            }
        }
    }
    std::array::from_fn(|mask| select(mask as u8, &art))
}

struct Sprites<T> {
    fog: Vec<T>,
    shroud: Option<T>,
    fog_edges: Variants<T>,
    shroud_edges: Variants<T>,
}

impl<T: Clone> Sprites<T> {
    fn load(terrain: &Path, mut upload: impl FnMut(&Path) -> Option<T>) -> Self {
        Self {
            fog: (1..=3).filter_map(|i| upload(&terrain.join(format!("fog/fog{i}.png")))).collect(),
            shroud: upload(&terrain.join("void/void.png")),
            fog_edges: load_variants(terrain, "fog", &mut upload),
            shroud_edges: load_variants(terrain, "void", &mut upload),
        }
    }

    fn fill(&self, at: Hex, cover: &Cover) -> Option<&T> {
        match cover {
            Cover::Fog if !self.fog.is_empty() => {
                Some(&self.fog[(at.x + at.y).rem_euclid(self.fog.len() as i32) as usize])
            }
            Cover::Fog => None,
            Cover::Shroud => self.shroud.as_ref(),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Boundary {
    at: Hex,
    fog: u8,
    shroud: u8,
}

fn boundaries(known: impl Iterator<Item = (Hex, bool)>, cover: &[(Hex, Cover)]) -> Vec<Boundary> {
    if cover.is_empty() {
        return Vec::new();
    }
    let covered: HashMap<_, _> = cover.iter().map(|(at, kind)| (*at, kind)).collect();
    known
        .filter_map(|(at, visible)| {
            let mut fog = 0;
            let mut shroud = 0;
            for (i, neighbor) in at.neighbors().enumerate() {
                match covered.get(&neighbor) {
                    Some(Cover::Shroud) => shroud |= 1 << i,
                    Some(Cover::Fog) if visible => fog |= 1 << i,
                    _ => {}
                }
            }
            (fog != 0 || shroud != 0).then_some(Boundary {
                at,
                fog,
                shroud,
            })
        })
        .collect()
}

pub(super) struct Art {
    sprites: Sprites<AtlasSprite>,
    fill_extent: Vec2,
    tiles: Vec<Boundary>,
}

impl Art {
    pub(super) fn load(terrain: &Path) -> Self {
        let mut atlas = Atlas::with_page_side(VISIBILITY_PAGE_SIDE);
        let sprites = Sprites::load(terrain, |path| atlas.add(&recolored_pixels(path, None)?));
        let fill_extent = sprites.fog.iter().chain(sprites.shroud.iter())
            .map(AtlasSprite::size).fold(Vec2::ZERO, Vec2::max);
        Self { sprites, fill_extent, tiles: Vec::new() }
    }

    pub(super) fn fill(&self, at: Hex, cover: &Cover) -> Option<&AtlasSprite> {
        self.sprites.fill(at, cover)
    }

    pub(super) fn fill_extent(&self) -> Vec2 {
        self.fill_extent
    }

    pub(super) fn update(&mut self, view: &Observation, border: &engine_lite::map::KnownBorder<'_>, cover: &[(Hex, Cover)]) {
        let known = view.tiles.iter().map(|t| (t.position, t.visible))
            .chain(border.iter().map(|(&at, &(_, visible))| (at, visible)));
        self.tiles = boundaries(known, cover);
    }

    pub(super) fn draw_edges(&self, layout: &BoardLayout, viewport: Viewport) {
        let size = layout.tile_size();
        for tile in &self.tiles {
            let [x, y] = layout.tile_origin(tile.at);
            let center = vec2(x + size / 2.0, y + size / 2.0);
            for sprite in self.sprites.fog_edges[tile.fog as usize]
                .iter()
                .chain(&self.sprites.shroud_edges[tile.shroud as usize])
            {
                draw_sprite(sprite, center, size, viewport);
            }
        }
    }
}

pub(super) fn draw_sprite(sprite: &AtlasSprite, center: Vec2, size: f32, viewport: Viewport) {
    if let Some(rect) = sprite_rect(center, sprite.size(), size, Some(viewport)) {
        sprite.draw(rect, WHITE, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::map_cover;
    use engine_lite::observation::ObservedTile;

    #[test]
    fn visibility_art_uses_one_uploader_for_fills_and_edges() {
        let terrain = std::env::temp_dir()
            .join(format!("rustnoth-visibility-art-{}", std::process::id()));
        let paths = [terrain.join("fog/fog-n.png"), terrain.join("void/void-ne.png")];
        for path in &paths {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, []).unwrap();
        }

        let mut next_slot = 0;
        let art = Sprites::load(&terrain, |path| {
            if path.ends_with("fog2.png") { return None; }
            let slot = next_slot;
            next_slot += 1;
            Some(slot)
        });
        for path in &paths {
            fs::remove_file(path).unwrap();
            fs::remove_dir(path.parent().unwrap()).unwrap();
        }
        fs::remove_dir(&terrain).unwrap();

        assert_eq!(art.fill(Hex { x: -1, y: 0 }, &Cover::Fog), Some(&1));
        assert_eq!(art.fill(Hex { x: -1, y: 0 }, &Cover::Shroud), Some(&2));
        assert_eq!(art.fog_edges[1], [3]);
        assert_eq!(art.shroud_edges[2], [4]);
        assert_eq!(next_slot, 5);
    }

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
        let result = boundaries(view.tiles.iter().map(|t| (t.position, t.visible)), &map_cover(&view).cells);
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
