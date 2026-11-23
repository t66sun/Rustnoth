use super::{Texture2D, Viewport, WHITE, draw_texture};

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

#[cfg(test)]
mod tests {
    use engine_lite::{hex::Hex, map::border_positions};
    use std::collections::HashSet;
    #[test]
    fn native_border_includes_each_surrounding_hex_once_and_never_playable_terrain() {
        let ring: Vec<_> = border_positions(3, 2).collect();
        assert_eq!(ring.len(), 14);
        assert_eq!(ring.iter().collect::<HashSet<_>>().len(), ring.len());
        assert!(ring.contains(&Hex { x: -1, y: -1 }) && ring.contains(&Hex { x: 3, y: 2 }));
        assert!(
            ring.iter()
                .all(|hex| !(0..3).contains(&hex.x) || !(0..2).contains(&hex.y))
        );
    }
}
