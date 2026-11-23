use std::ops::Range;

use super::{ART_HEX_SIZE, BoardLayout, Hex, Vec2, Viewport, vec2};

pub(super) fn village_marker(size: f32, minimum_radius: f32) -> (Vec2, f32) {
    (vec2(0.27, -0.26) * size, (size * 0.055).max(minimum_radius))
}

/// Screen bounds for terrain centers whose artwork can reach the battlefield viewport.
#[derive(Clone, Copy)]
pub(super) struct TerrainClip {
    min: Vec2,
    max: Vec2,
}

impl TerrainClip {
    pub(super) fn new(viewport: Viewport, size: f32, texture_extent: Vec2) -> Self {
        let sprite = texture_extent * (size / ART_HEX_SIZE) * 0.5;
        let (offset, radius) = village_marker(size, 2.0);
        let half = sprite.max(Vec2::splat(size * 0.5)).max(offset.abs() + radius);
        Self {
            min: vec2(viewport.x, viewport.y) - half,
            max: vec2(viewport.x + viewport.width, viewport.y + viewport.height) + half,
        }
    }

    pub(super) fn contains(self, center: Vec2) -> bool {
        center.x >= self.min.x && center.x <= self.max.x
            && center.y >= self.min.y && center.y <= self.max.y
    }

    /// Keep the original order of row-sorted anchors, using the staggered
    /// column centers at either end of each row to skip whole rows.
    pub(super) fn rows<'a, T>(
        self,
        layout: &BoardLayout,
        anchors: &'a [T],
        row: impl Fn(&T) -> i32,
    ) -> &'a [T] {
        let half = layout.tile_size() * 0.5;
        let first = anchors.partition_point(|anchor| {
            layout.tile_origin(Hex { x: 1, y: row(anchor) })[1] + half < self.min.y
        });
        let anchors = &anchors[first..];
        let end = anchors.partition_point(|anchor| {
            layout.tile_origin(Hex { x: 0, y: row(anchor) })[1] + half <= self.max.y
        });
        &anchors[..end]
    }
}

/// Only the visible portion of the public board and its one-hex border.
/// Each column parity has its own row range because odd columns sit lower.
pub(super) fn linger_anchors(
    layout: &BoardLayout,
    (width, height): (i32, i32),
    viewport: Viewport,
    extent: Vec2,
) -> impl Iterator<Item = Hex> {
    let size = layout.tile_size();
    let sprite = extent * (size / ART_HEX_SIZE);
    let top_left = |hex| {
        let center = Vec2::from_array(layout.tile_origin(hex)) + size * 0.5;
        center - sprite * 0.5
    };
    let columns = if viewport.height > 0.0 && viewport.width > 0.0 {
        overlapping_axis(width, viewport.x, viewport.width, sprite.x, |x| {
            top_left(Hex { x, y: 0 }).x
        })
    } else {
        0..0
    };
    let rows = [0, 1].map(|x| {
        overlapping_axis(height, viewport.y, viewport.height, sprite.y, |y| {
            top_left(Hex { x, y }).y
        })
    });

    columns.flat_map(move |x| {
        let x = x as i32;
        rows[x.rem_euclid(2) as usize].clone().map(move |y| Hex { x, y: y as i32 })
    })
}

fn overlapping_axis(
    last: i32,
    view_start: f32,
    view_extent: f32,
    sprite_extent: f32,
    sprite_start: impl Fn(i32) -> f32,
) -> Range<i64> {
    // Search the projected edges so native-scale pixel snapping and strict
    // viewport overlap match sprite_rect at a fractional camera position.
    let limit = i64::from(last) + 1;
    let first = first_matching(-1, limit, |i| sprite_start(i) + sprite_extent > view_start);
    let end = first_matching(first, limit, |i| sprite_start(i) >= view_start + view_extent);
    first..end
}

fn first_matching(mut first: i64, mut end: i64, matches: impl Fn(i32) -> bool) -> i64 {
    while first < end {
        let middle = first + (end - first) / 2;
        if matches(middle as i32) {
            end = middle;
        } else {
            first = middle + 1;
        }
    }
    first
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terrain_clip_skips_offscreen_rows_and_keeps_artwork_from_staggered_anchors() {
        let viewport = Viewport { x: 20.0, y: 20.0, width: 120.0, height: 120.0 };
        let layout = BoardLayout::battlefield(100, 100, viewport).unwrap()
            .native_scale(Some(Hex { x: 50, y: 50 }));
        let size = layout.tile_size();
        let extent = vec2(216.0, 248.0);
        let clip = TerrainClip::new(viewport, size, extent);
        let anchors: Vec<_> = (0..100)
            .flat_map(|y| (0..100).map(move |x| Hex { x, y }))
            .collect();
        let center = |hex: Hex| Vec2::from_array(layout.tile_origin(hex)) + size * 0.5;
        let outside = Hex { x: 47, y: 47 };
        assert!(!viewport.contains(center(outside).to_array()));

        let rows = clip.rows(&layout, &anchors, |hex| hex.y);
        assert!(rows.len() <= 700, "{} anchors remained after clipping rows", rows.len());
        let visible: Vec<_> = rows.iter().copied().filter(|&hex| clip.contains(center(hex))).collect();
        assert!(visible.contains(&outside));
        let overlapping: Vec<_> = anchors.into_iter().filter(|&hex| {
            super::super::sprite_rect(center(hex), extent, size, Some(viewport)).is_some()
        }).collect();
        assert_eq!(visible, overlapping);
    }

    #[test]
    fn a_zoomed_out_village_marker_can_overlap_from_outside_its_hex() {
        let viewport = Viewport { x: 0.0, y: 0.0, width: 10.0, height: 10.0 };
        let clip = TerrainClip::new(viewport, 1.0, Vec2::splat(72.0));
        assert!(clip.contains(vec2(-2.2, 5.0)));
        assert!(!clip.contains(vec2(-2.4, 5.0)));
    }

    #[test]
    fn linger_clips_to_the_same_drawn_columns_and_staggered_rows_as_a_full_sweep() {
        let viewport = Viewport { x: 20.5, y: 19.25, width: 235.0, height: 210.0 };
        let (width, height) = (200, 160);
        let layout = BoardLayout::battlefield(width, height, viewport).unwrap();
        let center = layout.native_scale(None);
        let corner = layout.native_scale(Some(Hex { x: 0, y: 0 }));
        let zoomed = corner.zoomed_at([viewport.x, viewport.y], 1.3);
        let extent = vec2(144.0, 96.0);

        for layout in [center, corner, zoomed] {
            let clipped: Vec<_> = linger_anchors(&layout, (width, height), viewport, extent).collect();
            let size = layout.tile_size();
            let full: Vec<_> = (-1..=width)
                .flat_map(|x| (-1..=height).map(move |y| Hex { x, y }))
                .filter(|&hex| {
                    let center = Vec2::from_array(layout.tile_origin(hex)) + size * 0.5;
                    super::super::sprite_rect(center, extent, size, Some(viewport)).is_some()
                })
                .collect();
            assert!(clipped.len() < 50);
            assert_eq!(clipped, full);
        }

        let border = linger_anchors(&corner, (width, height), viewport, extent).collect::<Vec<_>>();
        assert!(border.iter().any(|hex| hex.x == -1));
        assert!(border.iter().any(|hex| hex.y == -1));
    }
}
