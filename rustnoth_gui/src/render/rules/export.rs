use super::{bitmap::Bitmaps, compile, plan};
use engine_lite::{
    hex::Hex,
    map::ScenarioMap,
    observation::{Observation, ObservedTile},
};
use image::{Rgba, RgbaImage, imageops};
use rustnoth_gui::assets::AssetCatalog;
use std::{collections::HashMap, error::Error, path::Path};

pub(crate) fn map_observation(map: &ScenarioMap) -> Observation {
    let mut view = Observation {
        side: 1,
        allied_sides: vec![1],
        gold: None,
        width: map.width(),
        height: map.height(),
        tiles: vec![],
        units: vec![],
    };
    for x in 0..map.width() {
        for y in 0..map.height() {
            let position = Hex { x, y };
            if let Some(terrain) = map.terrain_at(position) {
                view.tiles.push(ObservedTile {
                    position,
                    terrain: terrain.into(),
                    visible: true,
                    village_owner: None,
                    time_color: [0; 3],
                });
            }
        }
    }
    view
}

fn dimensions(map: &ScenarioMap) -> (u32, u32) {
    (
        ((map.width() + 1) * 54 + 18) as u32,
        ((map.height() + 1) * 72 + 36) as u32,
    )
}

fn position(hex: Hex) -> (i64, i64) {
    (
        i64::from(27 + hex.x * 54),
        i64::from(36 + hex.y * 72 + hex.x.rem_euclid(2) * 36),
    )
}

fn composite(canvas: &mut RgbaImage, sprite: &RgbaImage, left: i64, top: i64) {
    let x0 = (-left).max(0) as u32;
    let y0 = (-top).max(0) as u32;
    let right = (i64::from(canvas.width()) - left).clamp(0, i64::from(sprite.width())) as u32;
    let bottom = (i64::from(canvas.height()) - top).clamp(0, i64::from(sprite.height())) as u32;
    for y in y0..bottom {
        for x in x0..right {
            let src = sprite.get_pixel(x, y);
            let alpha = u32::from(src[3]);
            if alpha == 0 {
                continue;
            }
            let dst =
                canvas.get_pixel_mut((left + i64::from(x)) as u32, (top + i64::from(y)) as u32);
            if alpha == 255 {
                *dst = *src;
                continue;
            }
            for c in 0..3 {
                let s = u32::from(src[c]) * alpha;
                let d = u32::from(dst[c]) * (255 - alpha);
                dst[c] = ((s + d + 127) / 255) as u8;
            }
        }
    }
}

pub(crate) fn map_png(root: &Path, file: &Path, output: &Path) -> Result<(), Box<dyn Error>> {
    let map = ScenarioMap::parse(&std::fs::read_to_string(file)?)?;
    let assets = AssetCatalog::parse(root, b"", b"")?;
    if assets.terrain_rules().is_empty() {
        return Err("Wesnoth terrain graphics are missing".into());
    }
    let catalog = compile::compile(assets.terrain_rules(), assets.image_roots());
    let (width, height) = dimensions(&map);
    let mut canvas = RgbaImage::from_pixel(width, height, Rgba([0, 0, 0, 255]));
    if let Some(path) = catalog.files.get("off-map/background.png") {
        let background = image::open(path)?.into_rgba8();
        for y in (0..height).step_by(background.height() as usize) {
            for x in (0..width).step_by(background.width() as usize) {
                imageops::overlay(&mut canvas, &background, i64::from(x), i64::from(y));
            }
        }
    }
    let view = map_observation(&map);
    let tiles = plan::map_tiles(&catalog, &view, &map);
    let mut order: Vec<_> = tiles.keys().copied().collect();
    order.sort_by_key(|hex| (2 * hex.y + hex.x.rem_euclid(2), hex.x));
    let mut bitmaps = Bitmaps::default();
    let mut frames = HashMap::new();
    for foreground in [false, true] {
        for &hex in &order {
            let tile = &tiles[&hex];
            let (x, y) = position(hex);
            for part in &tile.layers {
                if (part.layer > 0 || part.layer == 0 && part.y >= 54) != foreground {
                    continue;
                }
                for variant in &catalog.images[part.image].variants {
                    if !variant.periods.is_empty()
                        || !variant.flags.iter().all(|f| tile.flags.contains(f))
                    {
                        continue;
                    }
                    let index = (part.seed / 7919) as usize % variant.choices.len();
                    let sequence = &variant.choices[index];
                    let duration: u32 = sequence.iter().map(|&(_, time)| time).sum();
                    let mut offset = match variant.random {
                        0 => 0,
                        n if n > 0 => part.seed % (n as u32) % duration,
                        _ => part.seed % duration,
                    };
                    let Some(&(frame, _)) = sequence.iter().find(|&&(_, time)| {
                        if offset < time {
                            true
                        } else {
                            offset -= time;
                            false
                        }
                    }) else {
                        continue;
                    };
                    let image = frames
                        .entry((frame, part.cut))
                        .or_insert_with(|| bitmaps.pixels(&catalog, frame, part.cut));
                    if let Some(sprite) = image {
                        composite(
                            &mut canvas,
                            sprite.as_ref(),
                            x + (72 - i64::from(sprite.width())) / 2,
                            y + (72 - i64::from(sprite.height())) / 2,
                        );
                        break;
                    }
                }
            }
        }
    }
    canvas.save_with_format(output, image::ImageFormat::Png)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn framebuffer_blending_rounds_each_channel_and_clips_tiles_at_the_edge() {
        let mut canvas = RgbaImage::from_pixel(2, 1, Rgba([0, 15, 80, 255]));
        let sprite = RgbaImage::from_pixel(2, 1, Rgba([3, 254, 180, 128]));
        composite(&mut canvas, &sprite, -1, 0);
        assert_eq!(canvas.get_pixel(0, 0).0, [2, 135, 130, 255]);
        assert_eq!(canvas.get_pixel(1, 0).0, [0, 15, 80, 255]);
    }

    #[test]
    fn full_map_png_includes_the_default_half_hex_border_and_odd_column_offset() {
        let map = ScenarioMap::parse("Gg,Gg,Gg,Gg\nGg,Gg,Gg,Gg\nGg,Gg,Gg,Gg").unwrap();
        assert_eq!(dimensions(&map), (180, 180));
        assert_eq!(position(Hex { x: 1, y: 0 }), (81, 72));
        let catalog = compile::compile(&[], &[]);
        let view = Observation {
            side: 1,
            allied_sides: vec![1],
            gold: None,
            width: 2,
            height: 1,
            units: vec![],
            tiles: vec![],
        };
        assert!(
            plan::build_with_flags(&catalog, &view)[&Hex { x: -1, y: 0 }]
                .flags
                .contains("base")
        );
    }
}
