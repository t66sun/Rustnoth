use std::{collections::HashMap, io};

use engine_lite::wml;

type Rgb = [u8; 3];

/// The original default team ranges, in one-based side order.
#[derive(Debug)]
pub struct TeamColors {
    sides: Vec<TeamColor>,
}

#[derive(Debug)]
struct TeamColor {
    replacements: HashMap<Rgb, Rgb>,
    marker: Rgb,
}

impl TeamColors {
    /// Reads Wesnoth's core team-colors.cfg, including the magenta sprite palette.
    pub fn parse(bytes: &[u8]) -> io::Result<Self> {
        Self::parse_palette(bytes, "magenta")
    }

    /// Reads the green source palette used by the original village flag frames.
    pub fn parse_flags(bytes: &[u8]) -> io::Result<Self> {
        Self::parse_palette(bytes, "flag_green")
    }

    /// Source palette used by the original unit ellipse textures.
    pub fn parse_ellipses(bytes: &[u8]) -> io::Result<Self> {
        Self::parse_palette(bytes, "ellipse_red")
    }

    fn parse_palette(bytes: &[u8], name: &str) -> io::Result<Self> {
        Self::parse_ranges(bytes, name, None)
    }

    /// Applies a named original color range (including non-side colors) using index 1.
    pub fn named_range(bytes: &[u8], palette: &str, range: &str) -> io::Result<Self> {
        Self::parse_ranges(bytes, palette, Some(range))
    }

    fn parse_ranges(bytes: &[u8], name: &str, wanted: Option<&str>) -> io::Result<Self> {
        let config = wml::parse(bytes)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let source = config
            .children
            .iter()
            .filter(|(tag, _)| tag == "color_palette")
            .find_map(|(_, palette)| palette.attribute(name))
            .ok_or_else(|| invalid_data(format!("missing {name} palette")))?;
        let source = parse_colors(source)?;
        let reference = source
            .first()
            .copied()
            .map(shade)
            .ok_or_else(|| invalid_data(format!("{name} palette is empty")))?;

        let mut sides = Vec::new();
        for (tag, range) in &config.children {
            let matches = match wanted {
                Some(id) => range.attribute("id") == Some(id),
                None => matches!(range.attribute("default"), Some("yes" | "true" | "on" | "1")),
            };
            if tag != "color_range" || !matches
            {
                continue;
            }
            let (Some(_), Some(colors)) = (range.attribute("id"), range.attribute("rgb")) else {
                continue;
            };
            let colors = parse_colors(colors)?;
            let midpoint = colors.first().copied().unwrap_or([128; 3]);
            let maximum = colors.get(1).copied().unwrap_or([255; 3]);
            let minimum = colors.get(2).copied().unwrap_or([0; 3]);
            let replacements = source
                .iter()
                .copied()
                .map(|base| {
                    let replacement = recolor_shade(shade(base), reference, midpoint, minimum, maximum);
                    (base, replacement)
                })
                .collect();
            let marker = colors.get(3).copied().unwrap_or(midpoint);
            sides.push(TeamColor { replacements, marker });
        }
        if sides.is_empty() {
            return Err(invalid_data("missing default team color ranges"));
        }
        Ok(Self { sides })
    }

    /// Recolors the sprite's matching palette colors for a one-based side.
    /// Each pixel retains its alpha value, including fully transparent pixels.
    pub fn recolor(&self, side: usize, rgba: &mut [u8]) {
        let Some(team) = side.checked_sub(1).and_then(|index| self.sides.get(index)) else {
            return;
        };
        for pixel in rgba.chunks_exact_mut(4) {
            if pixel[3] != 0
                && let Some(replacement) = team.replacements.get(&[pixel[0], pixel[1], pixel[2]])
            {
                pixel[..3].copy_from_slice(replacement);
            }
        }
    }

    /// Returns the high-contrast marker RGB for a one-based side.
    pub fn marker_color(&self, side: usize) -> Option<[u8; 3]> {
        self.sides.get(side.checked_sub(1)?).map(|team| team.marker)
    }
}

fn parse_colors(value: &str) -> io::Result<Vec<Rgb>> {
    value
        .split(',')
        .map(str::trim)
        .filter(|hex| !hex.is_empty())
        .map(|hex| {
            if hex.len() != 6 || !hex.bytes().all(|digit| digit.is_ascii_hexdigit()) {
                return Err(invalid_data(format!("invalid RGB color {hex:?}")));
            }
            let [_, red, green, blue] = u32::from_str_radix(hex, 16)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?
                .to_be_bytes();
            Ok([red, green, blue])
        })
        .collect()
}

fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn shade(rgb: Rgb) -> f32 {
    let [red, green, blue] = rgb.map(u16::from);
    f32::from((red + green + blue) / 3)
}

fn recolor_shade(shade: f32, reference: f32, midpoint: Rgb, minimum: Rgb, maximum: Rgb) -> Rgb {
    // Wesnoth measures the palette shade against its first RGB entry, then
    // interpolates from the team's midpoint toward the corresponding endpoint.
    let (weight, endpoint) = if reference != 0.0 && shade <= reference {
        (shade / reference, minimum)
    } else {
        ((255.0 - shade) / (255.0 - reference), maximum)
    };
    std::array::from_fn(|channel| {
        (weight * f32::from(midpoint[channel])
            + (1.0 - weight) * f32::from(endpoint[channel])) as u8
    })
}

#[cfg(test)]
mod tests {
    use super::TeamColors;

    #[test]
    fn village_flags_recolor_green_cloth_and_preserve_pole_and_alpha() {
        let cfg = b"[color_range]\nid=blue\ndefault=yes\nrgb=2255AA\n[/color_range]\n\
            [color_palette]\nmagenta=F49AC1\nflag_green=00C800,006400\n[/color_palette]";
        let palette = TeamColors::parse_flags(cfg).unwrap();
        let mut pixels = [0, 200, 0, 180, 0, 100, 0, 255, 92, 62, 41, 255];
        palette.recolor(1, &mut pixels);
        assert_eq!(pixels, [34, 85, 170, 180, 17, 42, 85, 255, 92, 62, 41, 255]);
    }

    #[test]
    fn uses_default_side_order_and_recolors_only_visible_palette_pixels() {
        let colors = TeamColors::parse(
            br#"
[color_range]
    id=first
    default=yes
    rgb=804020,FFFFFF,000C04,0C2238
[/color_range]
[color_range]
    id=custom
    rgb=FFFFFF
[/color_range]
[color_range]
    id=second
    default=yes
    rgb=224466
[/color_range]
[color_palette]
    magenta=960F87,4B0843,F0B0AF
[/color_palette]
"#,
        )
        .unwrap();

        let mut sprite = [
            150, 15, 135, 255,
            75, 8, 67, 220,
            240, 176, 175, 128,
            150, 15, 135, 0,
            1, 2, 3, 255,
        ];
        colors.recolor(1, &mut sprite);
        assert_eq!(
            sprite,
            [
                128, 64, 32, 255,
                64, 38, 18, 220,
                207, 183, 171, 128,
                150, 15, 135, 0,
                1, 2, 3, 255,
            ],
        );

        let mut other_side = [150, 15, 135, 255];
        colors.recolor(2, &mut other_side);
        assert_eq!(other_side, [34, 68, 102, 255]);
        assert_eq!(colors.marker_color(1), Some([12, 34, 56]));
        assert_eq!(colors.marker_color(2), Some([34, 68, 102]));
        assert_eq!(colors.marker_color(3), None);
    }
}
