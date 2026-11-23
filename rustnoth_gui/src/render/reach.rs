use engine_lite::{hex::Hex, observation::Observation};
use macroquad::prelude::{Rect, Vec2};
use rustnoth_gui::layout::{BoardLayout, Viewport};

use super::{ART_HEX_SIZE, terrain_clip::TerrainClip, visible_rect};

pub(super) fn mask_rects<'a>(
    view: &'a Observation,
    layout: &'a BoardLayout,
    viewport: Viewport,
    extent: Vec2,
    masked: impl Fn(Hex) -> bool + 'a,
) -> impl Iterator<Item = Rect> + 'a {
    let size = layout.tile_size();
    let half_tile = Vec2::splat(size * 0.5);
    let mask_size = extent * (size / ART_HEX_SIZE);
    let half_mask = mask_size * 0.5;
    let clip = TerrainClip::new(viewport, size, extent);

    clip.rows(layout, &view.tiles, |tile| tile.position.y)
        .iter()
        .filter_map(move |tile| {
            let at = tile.position;
            let top = Vec2::from_array(layout.tile_origin(at)) + half_tile - half_mask;
            (visible_rect(viewport, top, mask_size) && masked(at))
                .then(|| Rect::new(top.x, top.y, mask_size.x, mask_size.y))
        })
}

pub(super) fn draw_counts(
    reach: &engine_lite::fixed_match::enemy_moves::EnemyReach,
    layout: &rustnoth_gui::layout::BoardLayout,
    viewport: rustnoth_gui::layout::Viewport,
    font: Option<&macroquad::text::Font>,
) {
    use macroquad::prelude::*;
    let size = layout.tile_size();
    let pixels = (16.0 * size / super::ART_HEX_SIZE).round().max(1.0) as u16;
    for (hex, count) in reach.counts.iter().filter(|(_, count)| **count > 1) {
        let [x, y] = layout.tile_origin(*hex);
        let center = vec2(x + size / 2.0, y + size / 2.0);
        if !viewport.contains(center.to_array()) { continue; }
        let label = count.to_string();
        let text = measure_text(&label, font, pixels, 1.0);
        let left = center.x - text.width / 2.0;
        let baseline = center.y + text.offset_y - text.height / 2.0;
        for (dx, dy, color) in [(-1., 0., BLACK), (1., 0., BLACK), (0., -1., BLACK), (0., 1., BLACK),
            (0., 0., Color::from_rgba(255, 255, 0, 255))] {
            draw_text_ex(&label, left + dx, baseline + dy, TextParams { font, font_size: pixels, color, ..Default::default() });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_lite::observation::ObservedTile;
    use std::cell::RefCell;

    #[test]
    fn reach_checks_only_visible_mask_rectangles_in_painter_order() {
        let viewport = Viewport { x: 20.25, y: 33.5, width: 120.0, height: 120.0 };
        let focus = Hex { x: 50, y: 50 };
        let native = BoardLayout::battlefield(100, 100, viewport).unwrap().native_scale(Some(focus));
        let zoomed = native.zoomed_at([80.25, 93.5], 1.3);
        let view = Observation {
            side: 1, allied_sides: vec![1], gold: None, width: 100, height: 100, units: vec![],
            tiles: (0..100).flat_map(|y| (0..100).map(move |x| ObservedTile {
                position: Hex { x, y }, terrain: "Gg".into(),
                visible: true, village_owner: None, time_color: [0; 3],
            })).collect(),
        };
        let extent = Vec2::new(108.0, 86.0);
        for layout in [native, zoomed] {
            let size = layout.tile_size();
            let center = |at| Vec2::from_array(layout.tile_origin(at)) + size * 0.5;
            let visible: Vec<_> = view.tiles.iter().filter_map(|tile| {
                let at = tile.position;
                super::super::sprite_rect(center(at), extent, size, Some(viewport)).map(|rect| (at, rect))
            }).collect();
            assert!(visible.iter().any(|&(at, _)| !viewport.contains(center(at).to_array())));

            let checked = RefCell::new(Vec::new());
            let painted: Vec<_> = mask_rects(&view, &layout, viewport, extent, |at| {
                checked.borrow_mut().push(at);
                at != focus
            }).collect();
            assert!(checked.borrow().len() < 20);
            assert_eq!(checked.into_inner(), visible.iter().map(|&(at, _)| at).collect::<Vec<_>>());
            assert_eq!(painted, visible.into_iter().filter_map(|(at, rect)| (at != focus).then_some(rect)).collect::<Vec<_>>());
        }
    }
}
