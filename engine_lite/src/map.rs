pub mod editor;

use std::collections::HashMap;
use std::error::Error;
use std::fmt;

use crate::hex::Hex;

// Wesnoth 1.18.8 permits stored tile indices 0 through 1000, including the border.
const MAX_STORED_SIDE: usize = 1001;
const BORDER: i32 = 1;

/// A Wesnoth scenario's raw terrain codes and special locations.
///
/// Hex (0, 0) is the first playable tile. The stored map includes a one-tile
/// border, so the first tile in the map text has coordinates (-1, -1).
#[derive(Debug)]
pub struct ScenarioMap {
    columns: i32,
    rows: i32,
    terrain: Vec<Box<str>>,
    special_locations: HashMap<String, Hex>,
}

/// Terrain and current visibility for explored, non-playable display hexes.
pub type KnownBorder<'a> = std::collections::BTreeMap<Hex, (&'a str, bool)>;

/// The authored half-hexes surrounding the playable rectangle.
pub fn border_positions(width: i32, height: i32) -> impl Iterator<Item = Hex> {
    (-1..=width)
        .flat_map(move |x| [Hex { x, y: -1 }, Hex { x, y: height }])
        .chain((0..height).flat_map(move |y| [Hex { x: -1, y }, Hex { x: width, y }]))
}

impl ScenarioMap {
    /// Reads scenario map data. Terrain codes are kept as text without a terrain catalog.
    pub fn parse(text: &str) -> Result<Self, MapParseError> {
        let mut columns = 0;
        let mut rows = 0;
        let mut terrain = Vec::new();
        let mut special_locations = HashMap::new();

        for line in map_body(text)
            .split(['\n', '\r'])
            .filter(|line| !line.is_empty())
        {
            rows += 1;
            let cells = line.split(',');
            let found = cells.clone().count();
            if rows > MAX_STORED_SIDE || found > MAX_STORED_SIDE {
                return Err(MapParseError::TooLarge);
            }
            if rows == 1 {
                columns = found;
            } else if found != columns {
                return Err(MapParseError::NonRectangular {
                    row: rows,
                    expected: columns,
                    found,
                });
            }

            for (column, cell) in cells.enumerate() {
                let mut parts = cell.trim().split(' ');
                // The last token is terrain. All preceding tokens are identifiers.
                let code = parts.next_back().unwrap_or_default();
                if !valid_terrain_code(code) {
                    return Err(MapParseError::InvalidTerrain {
                        row: rows,
                        column: column + 1,
                    });
                }
                let hex = Hex {
                    x: column as i32 - BORDER,
                    y: (rows - 1) as i32 - BORDER,
                };
                for id in parts.filter(|id| !id.is_empty()) {
                    special_locations.entry(id.to_owned()).or_insert(hex);
                }
                terrain.push(code.into());
            }
        }

        if rows == 0 {
            return Err(MapParseError::Empty);
        }
        Ok(Self {
            columns: columns as i32,
            rows: rows as i32,
            terrain,
            special_locations,
        })
    }

    /// Playable width in hexes.
    pub fn width(&self) -> i32 {
        (self.columns - 2 * BORDER).max(0)
    }

    /// Playable height in hexes.
    pub fn height(&self) -> i32 {
        (self.rows - 2 * BORDER).max(0)
    }

    pub fn is_playable(&self, hex: Hex) -> bool {
        (0..self.width()).contains(&hex.x) && (0..self.height()).contains(&hex.y)
    }

    /// Returns raw terrain at a playable or border hex.
    pub fn terrain_at(&self, hex: Hex) -> Option<&str> {
        if !(-BORDER..self.columns - BORDER).contains(&hex.x)
            || !(-BORDER..self.rows - BORDER).contains(&hex.y)
        {
            return None;
        }

        let column = (hex.x + BORDER) as usize;
        let row = (hex.y + BORDER) as usize;
        Some(&self.terrain[row * self.columns as usize + column])
    }

    pub(crate) fn replace_terrain(&mut self, hex: Hex, code: &str) {
        if self.is_playable(hex) {
            let index = ((hex.y + BORDER) * self.columns + hex.x + BORDER) as usize;
            self.terrain[index] = code.into();
        }
    }

    /// Finds the first occurrence of a starting side number or named location.
    pub fn special_location(&self, id: &str) -> Option<Hex> {
        self.special_locations.get(id).copied()
    }

    /// Numbered starting locations, in unspecified order, using WML side numbers.
    pub fn numbered_starts(&self) -> impl Iterator<Item = (usize, Hex)> + '_ {
        self.special_locations.iter().filter_map(|(id, &hex)| {
            let side = id.parse::<usize>().ok()?;
            (side > 0 && side.to_string() == *id).then_some((side, hex))
        })
    }
}

fn map_body(text: &str) -> &str {
    let text = text.trim_start_matches(['\n', '\r']);
    let Some((header, body)) = text
        .split_once("\n\n")
        .or_else(|| text.split_once("\r\n\r\n"))
    else {
        return text;
    };
    // Legacy header attributes are ignored in 1.18.8.
    let has_only_attributes_or_comments = header.lines().all(|line| {
        let line = line.trim();
        line.starts_with('#') || line.contains('=')
    });
    if !header.contains(',') && header.contains('=') && has_only_attributes_or_comments {
        body
    } else {
        text
    }
}

fn valid_terrain_code(code: &str) -> bool {
    let (base, overlay) = code.split_once('^').unwrap_or((code, ""));
    !(base.is_empty() && overlay.is_empty())
        && base.len() <= 4
        && overlay.len() <= 4
        && !overlay.contains('^')
}

/// Row and column numbers count from one in the stored grid, including the border.
#[derive(Debug, PartialEq, Eq)]
pub enum MapParseError {
    Empty,
    TooLarge,
    NonRectangular {
        row: usize,
        expected: usize,
        found: usize,
    },
    InvalidTerrain {
        row: usize,
        column: usize,
    },
}

impl fmt::Display for MapParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("scenario map has no tiles"),
            Self::TooLarge => {
                write!(f, "scenario map exceeds {MAX_STORED_SIDE} stored tiles per side")
            }
            Self::NonRectangular {
                row,
                expected,
                found,
            } => {
                write!(f, "map row {row} has {found} tiles, expected {expected}")
            }
            Self::InvalidTerrain { row, column } => {
                write!(f, "invalid terrain code at map row {row}, column {column}")
            }
        }
    }
}

impl Error for MapParseError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_layered_terrain_and_special_locations_with_the_fixed_border() {
        let map = ScenarioMap::parse(
            "\r\nusage=map\r\nborder_size=0\r\n\r\n\
             edge Xv, Xv, Xv, Xv\r\n\r\n\
             Xv, 1 keep Kh, Gg^Fp, Xv\r\n\
             Xv, Ww, 2 keep Kha, Xv\r\n\
             Xv, Xv, Xv, Xv\r\n",
        )
        .unwrap();
        let upper_left = Hex { x: 0, y: 0 };
        let border = Hex { x: -1, y: -1 };

        assert_eq!((map.width(), map.height()), (2, 2));
        assert_eq!(map.terrain_at(upper_left), Some("Kh"));
        assert_eq!(map.terrain_at(Hex { x: 1, y: 0 }), Some("Gg^Fp"));
        assert_eq!(map.terrain_at(Hex { x: 2, y: 1 }), Some("Xv"));
        assert_eq!(map.terrain_at(Hex { x: 3, y: 1 }), None);
        assert_eq!(map.special_location("1"), Some(upper_left));
        assert_eq!(map.special_location("keep"), Some(upper_left));
        assert_eq!(map.special_location("2"), Some(Hex { x: 1, y: 1 }));
        assert_eq!(map.special_location("edge"), Some(border));
        assert_eq!(map.terrain_at(border), Some("Xv"));
        assert!(map.is_playable(upper_left));
        assert!(!map.is_playable(border));
        assert!(!map.is_playable(Hex { x: 2, y: 1 }));
    }

    #[test]
    fn parses_headerless_map_data() {
        let map = ScenarioMap::parse("\nXv, Xv, Xv\nXv,\tWw^Bw|\t, Xv\nXv, Xv, Xv").unwrap();
        assert_eq!(map.terrain_at(Hex { x: 0, y: 0 }), Some("Ww^Bw|"));
    }

    #[test]
    fn rejects_empty_ragged_or_invalid_maps() {
        assert_eq!(
            ScenarioMap::parse("\n\r\n").unwrap_err(),
            MapParseError::Empty,
        );
        assert_eq!(
            ScenarioMap::parse("Xv, Xv\nXv").unwrap_err(),
            MapParseError::NonRectangular {
                row: 2,
                expected: 2,
                found: 1,
            },
        );
        assert_eq!(
            ScenarioMap::parse("Xv, Gg^Foooo").unwrap_err(),
            MapParseError::InvalidTerrain { row: 1, column: 2 },
        );
    }

    #[test]
    fn checks_the_native_stored_dimension_limit() {
        let largest_row = vec!["Gg"; MAX_STORED_SIDE].join(",");
        assert_eq!(ScenarioMap::parse(&largest_row).unwrap().width(), 999);
        let oversized_row = format!("{largest_row},Gg");
        assert_eq!(
            ScenarioMap::parse(&oversized_row).unwrap_err(),
            MapParseError::TooLarge,
        );
    }
}
