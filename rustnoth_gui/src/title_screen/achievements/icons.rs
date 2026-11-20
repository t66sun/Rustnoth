use std::{collections::HashMap, fs, path::Path};
use image::{RgbaImage, imageops};
use macroquad::prelude::{Texture2D, FilterMode};
use rustnoth_gui::team_colors::TeamColors;

pub(super) struct Icons<'a> {
    root: &'a Path,
    colors: Vec<u8>,
    textures: HashMap<(String, bool), Option<Texture2D>>,
}

fn split(expression: &str, separator: char) -> Vec<&str> {
    let mut depth = 0;
    let mut start = 0;
    let mut parts = Vec::new();
    for (pos, c) in expression.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            c if c == separator && depth == 0 => {
                parts.push(expression[start..pos].trim());
                start = pos + c.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(expression[start..].trim());
    parts
}

pub(in crate::title_screen) fn pixels(expression: &str, colors: &[u8], resolve: &impl Fn(&Path) -> Option<std::path::PathBuf>) -> Option<RgbaImage> {
    let mut terms = split(expression, '~').into_iter();
    let base = Path::new(terms.next()?.trim_matches('"'));
    let path = resolve(base)?;
    let mut output = image::open(path).ok()?.into_rgba8();
    for term in terms {
        let (command, args) = term.split_once('(')?;
        let args = split(args.strip_suffix(')')?, ',');
        match (command, args.as_slice()) {
            ("SCALE", [w, h]) => {
                output = imageops::resize(&output, w.parse().ok()?, h.parse().ok()?, imageops::FilterType::Triangle);
            }
            ("BLIT", [layer, rest @ ..]) => {
                let top = pixels(layer.trim_matches('"'), colors, resolve)?;
                let coord = |i: usize| rest.get(i).copied().unwrap_or("0").parse::<i64>().ok();
                imageops::overlay(&mut output, &top, coord(0)?, coord(1)?);
            }
            ("RC", [pair]) => {
                let (from, to) = pair.split_once('>')?;
                TeamColors::named_range(colors, from, to).ok()?.recolor(1, output.as_mut());
            }
            _ => return None,
        }
    }
    Some(output)
}

impl<'a> Icons<'a> {
    pub fn new(root: &'a Path) -> Self {
        Self { root, colors: fs::read(root.join("data/core/team-colors.cfg")).unwrap_or_default(),
            textures: HashMap::new() }
    }

    pub fn get(&mut self, expression: &str, completed: bool) -> Option<&Texture2D> {
        self.textures.entry((expression.to_owned(), completed)).or_insert_with(|| {
            let mut data = pixels(expression, &self.colors, &|base| Some(if base.starts_with("data") { self.root.join(base) } else { self.root.join("data/core/images").join(base) }))?;
            if !completed {
                for pixel in data.pixels_mut() {
                    let [r, g, b, _] = pixel.0;
                    let gray = ((77 * r as u32 + 150 * g as u32 + 29 * b as u32) >> 8) as u8;
                    pixel.0[..3].fill(gray);
                }
            }
            let texture = Texture2D::from_rgba8(data.width().try_into().ok()?, data.height().try_into().ok()?, data.as_raw());
            texture.set_filter(FilterMode::Linear);
            Some(texture)
        }).as_ref()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn layer_arguments_keep_nested_modifiers_together() {
        assert_eq!(super::split("unit.png~RC(magenta>blue)~SCALE(32,32), 1, 5", ','), ["unit.png~RC(magenta>blue)~SCALE(32,32)", "1", "5"]);
    }
}
