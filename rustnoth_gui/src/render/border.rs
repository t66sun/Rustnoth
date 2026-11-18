use super::{Hex, Texture2D, Viewport, WHITE, draw_texture};

pub(super) fn backdrop(texture: &Texture2D, area: Viewport) {
    let mut y = area.y;
    while y < area.y + area.height {
        let mut x = area.x;
        while x < area.x + area.width {
            draw_texture(texture, x, y, WHITE);
            x += texture.width();
        }
        y += texture.height();
    }
}

/// The map border is outside the playable hexes and derived solely from map dimensions.
pub(super) fn ring(width: i32, height: i32) -> impl Iterator<Item = Hex> {
    (-1..=width)
        .flat_map(move |x| [Hex { x, y: -1 }, Hex { x, y: height }])
        .chain((0..height).flat_map(move |y| [Hex { x: -1, y }, Hex { x: width, y }]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    #[test]
    fn native_border_includes_each_surrounding_hex_once_and_never_playable_terrain() {
        let ring: Vec<_> = ring(3, 2).collect();
        assert_eq!(ring.len(), 14);
        assert_eq!(ring.iter().collect::<HashSet<_>>().len(), ring.len());
        assert!(ring.contains(&Hex { x: -1, y: -1 }) && ring.contains(&Hex { x: 3, y: 2 }));
        assert!(
            ring.iter()
                .all(|hex| !(0..3).contains(&hex.x) || !(0..2).contains(&hex.y))
        );
    }
}
