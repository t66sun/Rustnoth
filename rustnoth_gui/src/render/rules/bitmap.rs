use super::compile::{Catalog, Cut};
use super::pattern;
use image::{RgbaImage, imageops};
use macroquad::prelude::{FilterMode, Texture2D};
use std::{collections::HashMap, rc::Rc};

#[derive(Default)]
pub(super) struct Bitmaps {
    sources: HashMap<String, Rc<RgbaImage>>,
    cropped: HashMap<(usize, Option<Cut>), Option<Texture2D>>,
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

impl Bitmaps {
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
                    let other = self.read(catalog, arg(0))?;
                    if data.dimensions() == other.dimensions() {
                        mask(&mut data, &other);
                    }
                }
                "O" => {
                    let factor = arg(0)
                        .strip_suffix('%')
                        .and_then(|n| n.parse::<f32>().ok())
                        .map(|p| p / 100.0)
                        .or_else(|| arg(0).parse().ok())?;
                    for pixel in data.pixels_mut() {
                        pixel[3] = (f32::from(pixel[3]) * factor).clamp(0.0, 255.0) as u8;
                    }
                }
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
    ) -> Option<Texture2D> {
        if let Some(texture) = self.cropped.get(&(id, part)) {
            return texture.clone();
        }
        let result = self.pixels(catalog, id, part).and_then(|tile| {
            let texture = Texture2D::from_rgba8(
                tile.width().try_into().ok()?,
                tile.height().try_into().ok()?,
                tile.as_raw(),
            );
            texture.set_filter(FilterMode::Linear);
            Some(texture)
        });
        self.cropped.insert((id, part), result.clone());
        result
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
}
