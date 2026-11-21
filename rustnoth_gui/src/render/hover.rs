//! The two halves of the mouseover hex straddle a visible unit in the native drawer.
use super::{
    BoardLayout, Hex, Observation, TeamColors, Texture2D, Vec2, Viewport, draw_centered,
    load_texture,
};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Relation {
    Empty,
    Enemy,
    Own,
    Ally,
}

fn relation(side: usize, allies: &[usize], occupant: Option<usize>) -> Relation {
    match occupant {
        None => Relation::Empty,
        Some(owner) if owner == side => Relation::Own,
        Some(owner) if allies.contains(&owner) => Relation::Ally,
        Some(_) => Relation::Enemy,
    }
}

pub(super) struct Art {
    halves: [[Option<Texture2D>; 2]; 4],
}

impl Art {
    pub(super) fn load(root: &Path, colors: Option<&[u8]>) -> Self {
        let folder = root.join("images/misc");
        let halves = [
            ("", "gold"),
            ("-enemy", "red"),
            ("", "green"),
            ("", "lightblue"),
        ]
        .map(|(style, color)| {
            let range =
                colors.and_then(|bytes| TeamColors::named_range(bytes, "magenta", color).ok());
            ["bottom", "top"].map(|half| {
                load_texture(
                    &folder.join(format!("hover-hex{style}-{half}.png")),
                    range.as_ref().map(|r| (r, 1)),
                )
            })
        });
        Self { halves }
    }

    pub(super) fn focus(
        &self,
        view: &Observation,
        layout: &BoardLayout,
        viewport: Viewport,
        x: f32,
        y: f32,
    ) -> Option<Focus<'_>> {
        if x < viewport.x
            || x >= viewport.x + viewport.width
            || y < viewport.y
            || y >= viewport.y + viewport.height
        {
            return None;
        }
        let hex = layout.hex_at([x, y])?;
        let occupant = view
            .units
            .iter()
            .find(|unit| unit.position == hex)
            .map(|unit| unit.side);
        let color = relation(view.side, &view.allied_sides, occupant);
        Some(Focus {
            hex,
            halves: &self.halves[color as usize],
            occupied: occupant.is_some(),
        })
    }

    pub(super) fn draw_empty_hex(&self, hex: Hex, layout: &BoardLayout) {
        Focus { hex, halves: &self.halves[Relation::Empty as usize], occupied: false }.draw(layout, false);
    }
}

pub(super) struct Focus<'a> {
    pub hex: Hex,
    halves: &'a [Option<Texture2D>; 2],
    occupied: bool,
}
impl Focus<'_> {
    pub(super) fn draw(&self, layout: &BoardLayout, above_unit: bool) {
        let size = layout.tile_size();
        let [left, top] = layout.tile_origin(self.hex);
        let center = Vec2::new(left + size / 2.0, top + size / 2.0);
        for (i, texture) in self.halves.iter().enumerate() {
            if (i == 1 && self.occupied) == above_unit
                && let Some(texture) = texture
            {
                draw_centered(texture, center, size, None);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_visible_occupant_controls_the_stock_mouseover_color() {
        assert_eq!(relation(2, &[2, 3], None), Relation::Empty);
        assert_eq!(relation(2, &[2, 3], Some(1)), Relation::Enemy);
        assert_eq!(relation(2, &[2, 3], Some(2)), Relation::Own);
        assert_eq!(relation(2, &[2, 3], Some(3)), Relation::Ally);
    }
}
