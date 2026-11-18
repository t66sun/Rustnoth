//! Compile [terrain_graphics] into per-hex sprites from the observed map.
mod bitmap;
mod compile;
mod pattern;
mod plan;

use super::{AssetCatalog, Hex, Observation, draw_centered};
use macroquad::prelude::{Texture2D, Vec2};
use std::collections::HashMap;

#[derive(Default)]
pub(super) struct Rules {
    catalog: Option<compile::Catalog>,
    bitmaps: bitmap::Bitmaps,
    known: Vec<(Hex, String)>,
    bounds: (i32, i32),
    tiles: HashMap<Hex, Drawing>,
}

struct Drawing {
    base: bool,
    layers: Vec<(bool, Vec<Animation>)>,
}
struct Animation {
    periods: Vec<String>,
    frames: Vec<(Option<Texture2D>, u32)>,
    duration: u32,
    offset: u32,
}

impl Rules {
    pub(super) fn load(&mut self, assets: &AssetCatalog, view: &Observation) {
        if assets.terrain_rules().is_empty() {
            return;
        }
        if self.bounds == (view.width, view.height)
            && self.known.len() == view.tiles.len()
            && self
                .known
                .iter()
                .zip(&view.tiles)
                .all(|((at, id), tile)| *at == tile.position && id == &tile.terrain)
        {
            return;
        }
        self.bounds = (view.width, view.height);
        self.known = view
            .tiles
            .iter()
            .map(|t| (t.position, t.terrain.clone()))
            .collect();
        let catalog = self
            .catalog
            .get_or_insert_with(|| compile::compile(assets.terrain_rules(), assets.image_roots()));
        self.tiles.clear();
        for (hex, tile) in plan::build(catalog, view) {
            let mut layers = Vec::new();
            for part in &tile.layers {
                let mut animations = Vec::new();
                for variant in &catalog.images[part.image].variants {
                    if !variant.flags.iter().all(|f| tile.flags.contains(f)) {
                        continue;
                    }
                    let index = (part.seed / 7919) as usize % variant.choices.len();
                    let mut duration = 0u32;
                    let frames: Vec<_> = variant.choices[index]
                        .iter()
                        .map(|&(id, time)| {
                            duration = duration.saturating_add(time);
                            (self.bitmaps.frame(catalog, id, part.cut), duration)
                        })
                        .collect();
                    if frames.iter().all(|(image, _)| image.is_none()) {
                        continue;
                    }
                    let offset = match variant.random {
                        0 => 0,
                        n if n > 0 => part.seed % n as u32,
                        _ => part.seed % duration,
                    };
                    animations.push(Animation {
                        periods: variant.periods.clone(),
                        frames,
                        duration,
                        offset,
                    });
                }
                if !animations.is_empty() {
                    layers.push((
                        part.layer > 0 || part.layer == 0 && part.y >= 54,
                        animations,
                    ));
                }
            }
            self.tiles.insert(
                hex,
                Drawing {
                    base: tile.flags.contains("base"),
                    layers,
                },
            );
        }
    }

    pub(super) fn has_base(&self, hex: Hex) -> bool {
        self.tiles.get(&hex).is_some_and(|t| t.base)
    }

    pub(super) fn draw(
        &self,
        hex: Hex,
        center: Vec2,
        size: f32,
        foreground: bool,
        tod: &str,
        ms: u64,
    ) {
        let Some(tile) = self.tiles.get(&hex) else {
            return;
        };
        for (_, variants) in tile.layers.iter().filter(|(front, _)| *front == foreground) {
            let Some(animation) = variants
                .iter()
                .find(|a| a.periods.is_empty() || a.periods.iter().any(|p| p == tod))
            else {
                continue;
            };
            let phase = ((ms + u64::from(animation.offset)) % u64::from(animation.duration)) as u32;
            if let Some((Some(texture), _)) = animation.frames.iter().find(|(_, end)| phase < *end)
            {
                draw_centered(texture, center, size);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_lite::{observation::ObservedTile, wml};

    #[test]
    fn applies_rotated_roads_only_when_the_neighbor_is_known_and_respects_flags() {
        let config = br#"
[terrain_graphics]
map="
2
, .
1"
rotations=n,ne,se,s,sw,nw
[tile]
pos=1
type=Rr
set_no_flag=road-@R0
[/tile]
[tile]
pos=2
type=Gg
[/tile]
[/terrain_graphics]
[terrain_graphics]
[tile]
x,y=0,0
type=Rr
has_flag=road-n
set_no_flag=base
[/tile]
[/terrain_graphics]
"#;
        let nodes = wml::parse(config)
            .unwrap()
            .children
            .into_iter()
            .map(|(_, n)| n)
            .collect::<Vec<_>>();
        let catalog = compile::compile(&nodes, &[]);
        assert_eq!(catalog.rules.len(), 7);
        let road = Hex { x: 2, y: 2 };
        let mut view = Observation {
            side: 1,
            allied_sides: vec![1],
            gold: None,
            width: 5,
            height: 5,
            units: vec![],
            tiles: vec![
                ObservedTile {
                    position: road,
                    terrain: "Rr".into(),
                    visible: true,
                    village_owner: None,
                    time_color: [0; 3],
                },
                ObservedTile {
                    position: Hex { x: 2, y: 1 },
                    terrain: "Gg^Fp".into(),
                    visible: true,
                    village_owner: None,
                    time_color: [0; 3],
                },
            ],
        };
        let flags = |view: &Observation| {
            // A rule can supply connectivity flags without supplying a sprite.
            let participating = plan::build_with_flags(&catalog, view);
            participating[&road].flags.clone()
        };
        let result = flags(&view);
        assert!(result.contains("road-n") && result.contains("base"));
        assert!(!result.contains("road-ne"));
        view.tiles.pop();
        assert!(!flags(&view).contains("road-n"));
    }
}
