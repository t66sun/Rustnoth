//! Compile [terrain_graphics] into per-hex sprites from the observed map.
mod bitmap;
mod export;
pub(crate) use export::{map_observation, map_png};
mod compile;
pub(super) mod pattern;
mod plan;

use super::{AssetCatalog, Hex, Observation, atlas::AtlasSprite, atlas_extent, draw_atlas_centered};
use macroquad::prelude::Vec2;
use std::{collections::HashMap, rc::Rc};

#[derive(Default)]
pub(crate) struct Rules {
    catalog: Option<compile::Catalog>,
    bitmaps: bitmap::Bitmaps,
    known: Vec<(Hex, String)>,
    bounds: (i32, i32),
    tiles: HashMap<Hex, Drawing>,
    pub(super) foreground: Vec<Hex>,
}

struct Drawing {
    base: bool,
    layers: Vec<(bool, Vec<Animation>)>,
}
struct Animation {
    sequence: Rc<Sequence>,
    offset: u32,
}

struct Sequence {
    periods: Vec<String>,
    frames: Vec<(Option<AtlasSprite>, u32)>,
    duration: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct SequenceKey {
    image: usize,
    variant: usize,
    choice: usize,
    cut: Option<compile::Cut>,
}

#[derive(Default)]
struct Sequences(HashMap<SequenceKey, Option<Rc<Sequence>>>);

impl Sequences {
    fn load(&mut self, key: SequenceKey, prepare: impl FnOnce() -> Option<Sequence>) -> Option<Rc<Sequence>> {
        self.0.entry(key).or_insert_with(|| prepare().map(Rc::new)).clone()
    }
}

impl Rules {
    /// Returns whether terrain artwork needs refreshing, including fallback transitions.
    pub(super) fn load(&mut self, assets: &AssetCatalog, view: &Observation, border: &engine_lite::map::KnownBorder<'_>) -> bool {
        // Fog alone leaves terrain unchanged; exploration can reveal a new
        // border neighbour and change the forest/shore rules on playable tiles.
        let known = || view.tiles.iter().map(|t| (t.position, t.terrain.as_str()))
            .chain(border.iter().map(|(&at, &(code, _))| (at, code)));
        if self.bounds == (view.width, view.height)
            && self.known.iter().map(|(at, id)| (*at, id.as_str())).eq(known()) {
            return false;
        }
        self.bounds = (view.width, view.height);
        self.known = known().map(|(at, code)| (at, code.to_owned())).collect();
        self.build(assets, |catalog| plan::battlefield(catalog, view, border));
        self.foreground = view.tiles.iter().map(|t| t.position)
            .chain(engine_lite::map::border_positions(view.width, view.height))
            .filter(|hex| self.tiles.get(hex).is_some_and(|t| t.layers.iter().any(|(front, _)| *front)))
            .collect();
        self.foreground.sort_unstable_by_key(|hex| (super::draw_row(*hex), hex.x));
        true
    }

    pub(crate) fn load_editor(&mut self, assets: &AssetCatalog, view: &Observation, map: &engine_lite::map::ScenarioMap) {
        self.build(assets, |catalog| plan::map_tiles(catalog, view, map));
    }

    fn build(&mut self, assets: &AssetCatalog, plan: impl FnOnce(&compile::Catalog) -> HashMap<Hex, plan::Tile>) {
        if assets.terrain_rules().is_empty() { return; }
        let catalog = self
            .catalog
            .get_or_insert_with(|| compile::compile(assets.terrain_rules(), assets.image_roots()));
        self.tiles.clear();
        let tiles = plan(catalog);
        let mut sequences = Sequences::default();
        for (hex, tile) in tiles {
            let mut layers = Vec::new();
            for part in &tile.layers {
                let mut animations = Vec::new();
                for (variant_index, variant) in catalog.images[part.image].variants.iter().enumerate() {
                    if !variant.flags.iter().all(|&flag| tile.has_flag(flag)) {
                        continue;
                    }
                    let choice = (part.seed / 7919) as usize % variant.choices.len();
                    let key = SequenceKey { image: part.image, variant: variant_index, choice, cut: part.cut };
                    let Some(sequence) = sequences.load(key, || {
                        let mut duration = 0u32;
                        let frames: Vec<_> = variant.choices[choice].iter().map(|&(id, time)| {
                            duration = duration.saturating_add(time);
                            (self.bitmaps.frame(catalog, id, part.cut), duration)
                        }).collect();
                        frames.iter().any(|(image, _)| image.is_some()).then(|| Sequence {
                            periods: variant.periods.clone(),
                            frames,
                            duration,
                        })
                    }) else {
                        continue;
                    };
                    let offset = match variant.random {
                        0 => 0,
                        n if n > 0 => part.seed % n as u32,
                        _ => part.seed % sequence.duration,
                    };
                    animations.push(Animation { sequence, offset });
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
                    base: tile.has_flag(compile::Flag::BASE),
                    layers,
                },
            );
        }
        self.bitmaps.release_sources();
    }

    pub(super) fn has_base(&self, hex: Hex) -> bool {
        self.tiles.get(&hex).is_some_and(|t| t.base)
    }

    pub(super) fn texture_extent(&self) -> Vec2 {
        atlas_extent(self.bitmaps.frames())
    }

    pub(super) fn add_terrain_sprite(&mut self, pixels: &image::RgbaImage) -> Option<AtlasSprite> {
        self.bitmaps.add_sprite(pixels)
    }

    pub(crate) fn draw(
        &self,
        hex: Hex,
        center: Vec2,
        size: f32,
        foreground: bool,
        tod: &str,
        ms: u64,
        clip: Option<super::Viewport>,
    ) {
        let Some(tile) = self.tiles.get(&hex) else {
            return;
        };
        for (_, variants) in tile.layers.iter().filter(|(front, _)| *front == foreground) {
            let Some(animation) = variants
                .iter()
                .find(|a| a.sequence.periods.is_empty() || a.sequence.periods.iter().any(|p| p == tod))
            else {
                continue;
            };
            let sequence = &animation.sequence;
            let phase = ((ms + u64::from(animation.offset)) % u64::from(sequence.duration)) as u32;
            if let Some((Some(sprite), _)) = sequence.frames.iter().find(|(_, end)| phase < *end)
            {
                draw_atlas_centered(sprite, center, size, clip);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_lite::{observation::ObservedTile, wml};

    #[test]
    fn same_variant_reuses_frames_until_its_choice_or_crop_changes() {
        let mut sequences = Sequences::default();
        let key = SequenceKey { image: 0, variant: 0, choice: 0, cut: None };
        let prepare = || Some(Sequence {
            periods: vec!["dawn".into()],
            frames: vec![(None, 60), (None, 120)],
            duration: 120,
        });

        let first = sequences.load(key, prepare).unwrap();
        let repeated = sequences.load(key, || panic!("already prepared")).unwrap();
        assert!(Rc::ptr_eq(&first, &repeated));

        let other_choice = SequenceKey { choice: 1, ..key };
        assert!(!Rc::ptr_eq(&first, &sequences.load(other_choice, prepare).unwrap()));

        let crop = compile::Cut { x: -8, y: 12, center: None };
        let absent = SequenceKey { cut: Some(crop), ..key };
        assert!(sequences.load(absent, || None).is_none());
        assert!(sequences.load(absent, || panic!("already found missing")).is_none());
    }

    #[test]
    fn fallback_terrain_refreshes_only_when_known_terrain_changes() {
        let assets = AssetCatalog::parse(std::path::Path::new(""), b"", b"").unwrap();
        let map = engine_lite::map::ScenarioMap::parse("Gg,Gg,Gg\nGg,Gg,Gg\nGg,Gg,Gg").unwrap();
        let mut view = map_observation(&map);
        let border = Default::default();
        let mut rules = Rules::default();

        assert!(rules.load(&assets, &view, &border));

        view.tiles[0].visible = false;
        view.tiles[0].village_owner = Some(2);
        view.tiles[0].time_color = [-20, -10, 0];
        assert!(!rules.load(&assets, &view, &border));

        view.tiles[0].terrain = "Gs".into();
        assert!(rules.load(&assets, &view, &border));
        let hidden_tile = view.tiles.pop().unwrap();
        assert!(rules.load(&assets, &view, &border));
        view.tiles.push(hidden_tile);
        assert!(rules.load(&assets, &view, &border));
        assert!(!rules.load(&assets, &view, &border));
    }

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
        assert_eq!(catalog.rules.len(), 8); // default theme plus the seven WML rules
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
        let tile = |view: &Observation| {
            // A rule can supply connectivity flags without supplying a sprite.
            plan::build_with_flags(&catalog, view, &Default::default()).remove(&road).unwrap()
        };
        let road_n = catalog.flag("road-n");
        let result = tile(&view);
        assert!(result.has_flag(road_n) && result.has_flag(catalog.flag("base")));
        assert!(!result.has_flag(catalog.flag("road-ne")));
        view.tiles.pop();
        assert!(!tile(&view).has_flag(road_n));
    }
}
