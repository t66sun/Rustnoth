use engine_lite::{fixed_match::Action, hex::Hex};
use std::collections::HashSet;

/// A selected unit always occupies a reachable hex. Only the current attack
/// target is exempt from the dark mask on an otherwise unreachable enemy tile.
pub(super) fn reachable(selected: Option<Hex>, actions: &[Action], target: Option<Hex>) -> Option<HashSet<Hex>> {
    let origin = selected?;
    let mut reachable = HashSet::from([origin]);
    for action in actions {
        match action {
            Action::Move { from, to } if *from == origin => { reachable.insert(*to); }
            Action::Attack(attack) if attack.from == origin && Some(attack.to) == target => {
                reachable.insert(attack.to);
            }
            _ => {}
        }
    }
    Some(reachable)
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

    #[test]
    fn only_the_selected_unit_exposes_its_destination_range() {
        let start = Hex { x: 1, y: 2 };
        let goal = Hex { x: 2, y: 2 };
        let other = Hex { x: 8, y: 3 };
        let actions = [Action::Move { from: start, to: goal }, Action::Move { from: other, to: start }];
        assert_eq!(reachable(None, &actions, None), None);
        assert_eq!(reachable(Some(start), &actions, None), Some(HashSet::from([start, goal])));
    }
}
