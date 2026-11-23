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
