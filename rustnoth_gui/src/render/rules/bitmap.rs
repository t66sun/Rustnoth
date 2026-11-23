use super::compile::{Catalog, Cut};
use super::pattern;
use crate::render::atlas::{Atlas, AtlasSprite};
use image::{RgbaImage, imageops};
use std::{collections::HashMap, rc::Rc};

#[derive(Default)]
pub(super) struct Bitmaps {
    sources: HashMap<String, Rc<RgbaImage>>,
    cropped: HashMap<(usize, Option<Cut>), Option<AtlasSprite>>,
    atlas: Atlas,
}

fn operations(expression: &str) -> Vec<&str> {
    let mut segments = Vec::new();
    let mut begin = 0;
    let mut depth = 0;
    for (idx, ch) in expression.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth -= 1,
            '~' if depth == 0 => {
                segments.push(&expression[begin..idx]);
                begin = idx + 1;
            }
            _ => {}
        }
    }
    segments.push(&expression[begin..]);
    segments
}

fn cut(image: &RgbaImage, left: i32, top: i32, width: u32, height: u32) -> RgbaImage {
    let mut result = RgbaImage::new(width, height);
    imageops::replace(&mut result, image, -i64::from(left), -i64::from(top));
    result
}

fn mask(image: &mut RgbaImage, stencil: &RgbaImage) {
    for (pixel, alpha) in image.pixels_mut().zip(stencil.pixels()) {
        pixel[3] = pixel[3].min(alpha[3]);
    }
}

fn apply_opacity(image: &mut RgbaImage, argument: &str) {
    let factor = match argument.split_once('%') {
        Some((percent, _)) => percent.trim().parse::<f32>().unwrap_or(0.0) / 100.0,
        None => argument.trim().parse::<f32>().unwrap_or(0.0),
    };
    // Wesnoth's float_to_color quantizes the multiplier before color_multiply.
    let multiplier = (factor * 256.0).clamp(0.0, 255.0) as u8;
    for pixel in image.pixels_mut() {
        pixel[3] = (u16::from(pixel[3]) * u16::from(multiplier) / 255) as u8;
    }
}

impl Bitmaps {
    pub(super) fn frames(&self) -> impl Iterator<Item = &AtlasSprite> {
        self.cropped.values().flatten()
    }

    fn read(&mut self, catalog: &Catalog, expression: &str) -> Option<Rc<RgbaImage>> {
        if let Some(src) = self.sources.get(expression) {
            return Some(Rc::clone(src));
        }
        let mut pieces = operations(expression).into_iter();
        let name = pieces
            .next()?
            .strip_prefix("terrain/")
            .unwrap_or_else(|| expression.split('~').next().unwrap_or(""));
        let mut data = image::open(catalog.files.get(name)?).ok()?.into_rgba8();
        for modifier in pieces {
            let (command, body) = modifier.split_once('(')?;
            let args = pattern::split(body.strip_suffix(')')?);
            let arg = |n: usize| args.get(n).copied().unwrap_or("");
            let num = |n: usize| arg(n).parse::<i32>().unwrap_or(0);
            match command {
                "NOP" | "NO_TOD_SHIFT" => {}
                "BLIT" => {
                    let other = self.read(catalog, arg(0))?;
                    imageops::overlay(
                        &mut data,
                        other.as_ref(),
                        i64::from(num(1)),
                        i64::from(num(2)),
                    );
                }
                "MASK" => {
                    let (x, y) = if args.len() == 3 { (num(1), num(2)) } else { (0, 0) };
                    // Invalid MASK arguments discard this modifier, leaving the source in the chain.
                    if x < 0 || y < 0 { continue; }
                    let Some(other) = self.read(catalog, arg(0)) else { continue };
                    if x == 0 && y == 0 && data.dimensions() == other.dimensions() {
                        mask(&mut data, &other);
                    } else {
                        let mut positioned = RgbaImage::new(data.width(), data.height());
                        imageops::replace(&mut positioned, other.as_ref(), i64::from(x), i64::from(y));
                        mask(&mut data, &positioned);
                    }
                }
                "O" => apply_opacity(&mut data, arg(0)),
                "CS" => {
                    let offsets = [num(0), num(1), num(2)];
                    for pixel in data.pixels_mut() {
                        for (channel, delta) in pixel.0[..3].iter_mut().zip(offsets) {
                            *channel = (i32::from(*channel) + delta).clamp(0, 255) as u8;
                        }
                    }
                }
                "CROP" => {
                    let (w, h) = (u32::try_from(num(2)).ok()?, u32::try_from(num(3)).ok()?);
                    if w == 0 || h == 0 || w > data.width() || h > data.height() {
                        return None;
                    }
                    data = cut(&data, num(0), num(1), w, h);
                }
                _ => return None,
            }
        }
        let data = Rc::new(data);
        self.sources.insert(expression.into(), Rc::clone(&data));
        Some(data)
    }

    pub(super) fn pixels(
        &mut self,
        catalog: &Catalog,
        id: usize,
        part: Option<Cut>,
    ) -> Option<Rc<RgbaImage>> {
        let data = self.read(catalog, &catalog.names[id])?;
        let mut tile = if let Some(part) = part {
            let [dx, dy] = part.center.map_or([0, 0], |[x, y]| {
                [data.width() as i32 / 2 - x, data.height() as i32 / 2 - y]
            });
            Rc::new(cut(&data, part.x + dx, part.y + dy, 72, 72))
        } else {
            data
        };
        // Wesnoth requests HEXED images for local terrain and multi-hex fragments alike.
        if let Some(stencil) = self.read(catalog, "alphamask.png") {
            if tile.dimensions() != stencil.dimensions() {
                let left = (tile.width() as i32 - stencil.width() as i32) / 2;
                let top = (tile.height() as i32 - stencil.height() as i32) / 2;
                tile = Rc::new(cut(&tile, left, top, stencil.width(), stencil.height()));
            }
            mask(Rc::make_mut(&mut tile), &stencil);
        }
        tile.pixels().any(|pixel| pixel[3] != 0).then_some(tile)
    }

    pub(super) fn frame(
        &mut self,
        catalog: &Catalog,
        id: usize,
        part: Option<Cut>,
    ) -> Option<AtlasSprite> {
        if let Some(sprite) = self.cropped.get(&(id, part)) {
            return sprite.clone();
        }
        let result = self.pixels(catalog, id, part).and_then(|tile| self.atlas.add(&tile));
        self.cropped.insert((id, part), result.clone());
        result
    }

    pub(super) fn release_sources(&mut self) {
        // The uploaded fragments are cached independently of the decoded images.
        self.sources.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn terrain_cuts_copy_rgba_bytes_and_pad_outside_the_source() {
        let mut source = RgbaImage::new(2, 1);
        source.put_pixel(0, 0, Rgba([123, 63, 31, 137]));
        source.put_pixel(1, 0, Rgba([4, 5, 6, 0]));
        let result = cut(&source, -1, 0, 3, 2);
        assert_eq!(result.get_pixel(0, 0).0, [0; 4]);
        assert_eq!(result.get_pixel(1, 0), source.get_pixel(0, 0));
        assert_eq!(result.get_pixel(2, 0), source.get_pixel(1, 0));
        assert_eq!(result.get_pixel(1, 1).0, [0; 4]);
    }

    #[test]
    fn opacity_quantizes_the_factor_before_multiplying_each_alpha() {
        for (argument, expected) in [
            ("0.5", [0, 1, 127, 128]),
            ("33.3%", [0, 1, 84, 85]),
            ("0.65", [0, 1, 165, 166]),
            ("150%", [0, 3, 254, 255]),
            ("-0.1", [0; 4]),
        ] {
            let mut image = RgbaImage::from_fn(4, 1, |x, _| Rgba([45, 16, 9, [0, 3, 254, 255][x as usize]]));
            apply_opacity(&mut image, argument);
            for (pixel, alpha) in image.pixels().zip(expected) {
                assert_eq!(pixel.0, [45, 16, 9, alpha], "{argument}");
            }
        }
    }

    #[test]
    fn local_water_frames_are_clipped_to_the_same_hex_as_rule_fragments() {
        let mut catalog = crate::render::rules::compile::compile(&[], &[]);
        catalog.names.push("water".into());
        let mut bitmaps = Bitmaps::default();
        bitmaps.sources.insert(
            "water".into(),
            Rc::new(RgbaImage::from_pixel(72, 72, Rgba([10, 20, 30, 255]))),
        );
        let mut stencil = RgbaImage::from_pixel(72, 72, Rgba([0, 0, 0, 255]));
        stencil.get_pixel_mut(0, 0)[3] = 0;
        bitmaps
            .sources
            .insert("alphamask.png".into(), Rc::new(stencil));
        let frame = bitmaps.pixels(&catalog, 0, None).unwrap();
        assert_eq!(frame.get_pixel(0, 0)[3], 0);
        assert_eq!(frame.get_pixel(35, 35).0, [10, 20, 30, 255]);
    }

    #[test]
    fn transparent_crops_keep_distinct_cache_entries_without_allocating_a_page() {
        let mut catalog = super::super::compile::compile(&[], &[]);
        let name = "transparent.png";
        catalog.names.push(name.into());
        let mut bitmaps = Bitmaps::default();
        bitmaps.sources.insert(name.into(), Rc::new(RgbaImage::new(144, 72)));

        for x in [0, 72] {
            let cut = Some(Cut { x, y: 0, center: None });
            assert!(bitmaps.frame(&catalog, 0, cut).is_none());
            assert!(bitmaps.cropped.contains_key(&(0, cut)));
        }
        assert_eq!(bitmaps.cropped.len(), 2);
        assert!(bitmaps.atlas.is_empty());
    }

    #[test]
    fn building_terrain_releases_decoded_sources_and_retains_fragment_entries() {
        use super::super::{Rules, compile, map_observation};
        use engine_lite::map::ScenarioMap;
        use rustnoth_gui::assets::AssetCatalog;

        let name = "transparent.png";
        let assets = AssetCatalog::parse(std::path::Path::new(""), br#"
[terrain_graphics]
    [tile]
        x,y=0,0
        type=Gg
        [image]
            name=transparent.png
        [/image]
    [/tile]
[/terrain_graphics]
"#, b"").unwrap();
        let mut rules = Rules::default();
        rules.catalog = Some(compile::compile_files(
            assets.terrain_rules(),
            HashMap::from([(name.into(), name.into())]),
        ));
        // Transparent fragments exercise the upload cache without a graphics context.
        let source = Rc::new(RgbaImage::new(144, 144));
        let decoded = Rc::downgrade(&source);
        rules.bitmaps.sources.insert(name.into(), source);
        let map = ScenarioMap::parse("Gg,Gg,Gg\nGg,Gg,Gg\nGg,Gg,Gg").unwrap();
        rules.load_editor(&assets, &map_observation(&map), &map);

        assert!(rules.bitmaps.cropped.contains_key(&(0, None)));
        assert!(decoded.upgrade().is_none());
    }

    #[test]
    fn positioned_masks_clip_the_stencil_and_clear_alpha_outside_its_bounds() {
        let file = std::env::temp_dir().join(format!("rustnoth-mask-{}.png", std::process::id()));
        let source = RgbaImage::from_pixel(3, 2, Rgba([23, 61, 17, 120]));
        source.save(&file).unwrap();
        let mut catalog = crate::render::rules::compile::compile(&[], &[]);
        catalog.files.insert("base.png".into(), file.clone());
        let mut bitmaps = Bitmaps::default();
        let stencil = RgbaImage::from_fn(2, 2, |x, y| {
            Rgba([3, 2, 1, if x == y { 70 } else { 250 }])
        });
        bitmaps.sources.insert("stencil.png".into(), Rc::new(stencil));
        for (arguments, expected) in [
            ("stencil.png", [70, 120, 0, 120, 70, 0]),
            ("stencil.png,1,1", [0, 0, 0, 0, 70, 120]),
            ("stencil.png,2,0", [0, 0, 70, 0, 0, 120]),
            ("stencil.png,3,0", [0; 6]),
            ("stencil.png,-1,0", [120; 6]),
            ("missing.png", [120; 6]),
        ] {
            let result = bitmaps.read(&catalog, &format!("base.png~MASK({arguments})")).unwrap();
            for (p, alpha) in result.pixels().zip(expected) {
                assert_eq!(p.0, [23, 61, 17, alpha], "{arguments}");
            }
        }
        std::fs::remove_file(file).unwrap();
    }
}
