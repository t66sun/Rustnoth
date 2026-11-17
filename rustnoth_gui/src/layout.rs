use engine_lite::hex::Hex;

const COLUMN_SPACING: f32 = 0.75;
const ODD_COLUMN_SHIFT: f32 = 0.5;
const MAX_TILE_SIZE: f32 = 144.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewport {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// A fitted board with Wesnoth's southward shift on odd columns.
#[derive(Clone, Copy, Debug)]
pub struct BoardLayout {
    width: i32,
    height: i32,
    origin: [f32; 2],
    tile_size: f32,
    fitted_tile_size: f32,
}

impl BoardLayout {
    /// Centers the board at the largest fitting size, up to 144 pixels per tile.
    pub fn fit(width: i32, height: i32, viewport: Viewport) -> Option<Self> {
        let Viewport {
            x,
            y,
            width: available_width,
            height: available_height,
        } = viewport;
        if width <= 0
            || height <= 0
            || ![x, y, available_width, available_height]
                .into_iter()
                .all(f32::is_finite)
        {
            return None;
        }
        let [board_width, board_height] = board_extent(width, height);
        let tile_size = (available_width / board_width)
            .min(available_height / board_height)
            .min(MAX_TILE_SIZE);
        if tile_size <= 0.0 {
            return None;
        }
        Some(Self {
            width,
            height,
            origin: [
                x + (available_width - board_width * tile_size) / 2.0,
                y + (available_height - board_height * tile_size) / 2.0,
            ],
            tile_size,
            fitted_tile_size: tile_size,
        })
    }

    /// Scales around a screen point, between the fitted size and 144 pixels per tile.
    pub fn zoomed_at(&self, [x, y]: [f32; 2], factor: f32) -> Self {
        if factor <= 0.0 || ![x, y, factor].into_iter().all(f32::is_finite) {
            return *self;
        }
        let tile_size = (self.tile_size * factor).clamp(self.fitted_tile_size, MAX_TILE_SIZE);
        if tile_size == self.tile_size {
            return *self;
        }
        let scale = tile_size / self.tile_size;
        let [left, top] = self.origin;
        Self {
            origin: [x + (left - x) * scale, y + (top - y) * scale],
            tile_size,
            ..*self
        }
    }

    /// The width and height of one hex's square sprite.
    pub fn tile_size(&self) -> f32 {
        self.tile_size
    }

    /// Screen coordinates of the top-left corner of a hex's sprite.
    pub fn tile_origin(&self, hex: Hex) -> [f32; 2] {
        [
            self.origin[0] + hex.x as f32 * COLUMN_SPACING * self.tile_size,
            self.origin[1]
                + (hex.y as f32 + hex.x.rem_euclid(2) as f32 * ODD_COLUMN_SHIFT) * self.tile_size,
        ]
    }

    /// Maps a screen point to a playable hex, including the sloped sprite edges.
    pub fn hex_at(&self, [x, y]: [f32; 2]) -> Option<Hex> {
        let x = (x - self.origin[0]) / self.tile_size;
        let y = (y - self.origin[1]) / self.tile_size;
        let [board_width, board_height] = board_extent(self.width, self.height);
        if !(0.0..board_width).contains(&x) || !(0.0..board_height).contains(&y) {
            return None;
        }

        // Each column pair repeats the same cross section. The even hex spans
        // from the left sloped edge to its mirror on the right.
        let pair_width = 2.0 * COLUMN_SPACING;
        let pair = (x / pair_width).floor() as i64;
        let x_in_pair = x % pair_width;
        let y_in_row = y.fract();
        let left_edge = (y_in_row - ODD_COLUMN_SHIFT).abs() / 2.0;
        let column_offset = if x_in_pair < left_edge {
            -1
        } else if x_in_pair < 1.0 - left_edge {
            0
        } else {
            1
        };
        let column = pair * 2 + column_offset;
        let upper_odd_hex = column_offset != 0 && y_in_row < ODD_COLUMN_SHIFT;
        let row = y.floor() as i64 - upper_odd_hex as i64;

        if !(0..i64::from(self.width)).contains(&column)
            || !(0..i64::from(self.height)).contains(&row)
        {
            return None;
        }
        Some(Hex { x: column as i32, y: row as i32 })
    }
}

fn board_extent(width: i32, height: i32) -> [f32; 2] {
    let stagger = if width > 1 { ODD_COLUMN_SHIFT } else { 0.0 };
    [
        1.0 + (width - 1) as f32 * COLUMN_SPACING,
        height as f32 + stagger,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout() -> BoardLayout {
        let viewport = Viewport { x: 40.0, y: 30.0, width: 230.0, height: 180.0 };
        BoardLayout::fit(3, 2, viewport).unwrap()
    }

    #[test]
    fn fits_native_sprite_extents_and_shifts_odd_columns_south() {
        let layout = layout();
        assert_eq!(layout.tile_size(), 72.0);
        assert_eq!(layout.tile_origin(Hex { x: 0, y: 0 }), [65.0, 30.0]);
        assert_eq!(layout.tile_origin(Hex { x: 1, y: 0 }), [119.0, 66.0]);
        assert_eq!(layout.tile_origin(Hex { x: 2, y: 0 }), [173.0, 30.0]);
    }

    #[test]
    fn zoom_preserves_the_anchored_hex_and_clamps_to_the_fitted_size() {
        let fit = layout();
        let anchor = [137.0, 84.0];
        let hex = Hex { x: 1, y: 0 };
        assert_eq!(fit.hex_at(anchor), Some(hex));

        let zoomed = fit.zoomed_at(anchor, 3.0);
        assert_eq!(zoomed.tile_size(), 144.0);
        assert_eq!(zoomed.hex_at(anchor), Some(hex));
        assert_eq!(zoomed.tile_origin(hex), [101.0, 48.0]);

        let restored = zoomed.zoomed_at(anchor, 0.25);
        assert_eq!(restored.tile_size(), fit.tile_size());
        assert_eq!(restored.tile_origin(hex), fit.tile_origin(hex));
    }

    #[test]
    fn fits_a_small_board_at_the_zoom_ceiling() {
        let viewport = Viewport { x: 0.0, y: 0.0, width: 600.0, height: 500.0 };
        let layout = BoardLayout::fit(1, 1, viewport).unwrap();
        assert_eq!(layout.tile_size(), 144.0);
        assert_eq!(layout.tile_origin(Hex { x: 0, y: 0 }), [228.0, 178.0]);
    }

    #[test]
    fn picks_centers_and_the_neighbor_across_a_sloped_edge() {
        let layout = layout();
        for y in 0..2 {
            for x in 0..3 {
                let hex = Hex { x, y };
                let [left, top] = layout.tile_origin(hex);
                assert_eq!(layout.hex_at([left + 36.0, top + 36.0]), Some(hex));
            }
        }
        assert_eq!(layout.hex_at([120.0, 67.0]), Some(Hex { x: 0, y: 0 }));
        assert_eq!(layout.hex_at([146.0, 70.0]), Some(Hex { x: 1, y: 0 }));
    }

    #[test]
    fn rejects_board_margin_and_cut_corners() {
        let layout = layout();
        for point in [[45.0, 80.0], [66.0, 31.0], [141.0, 32.0], [186.0, 204.0]] {
            assert_eq!(layout.hex_at(point), None, "{point:?}");
        }
    }
}
