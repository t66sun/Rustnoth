use std::collections::BTreeSet;

use crate::hex::Hex;

/// What one side can see at a playable hex.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HexVisibility {
    /// The tile is in view, subject to unit-specific concealment rules.
    Visible,
    /// The terrain is known, but the side cannot see units here.
    Fogged,
    /// Neither the terrain nor the units are visible to this side.
    Shrouded,
}

/// A side's current sight and its exploration of the playable map.
pub struct VisibilityMask {
    width: i32,
    height: i32,
    fog: bool,
    shroud: bool,
    sight: BTreeSet<Hex>,
    explored: BTreeSet<Hex>,
}

impl VisibilityMask {
    /// Creates a mask before any tiles have been uncovered by sight.
    pub fn new(width: i32, height: i32, fog: bool, shroud: bool) -> Self {
        Self {
            width,
            height,
            fog,
            shroud,
            sight: BTreeSet::new(),
            explored: BTreeSet::new(),
        }
    }

    /// Updates current sight and preserves the terrain uncovered by earlier sight.
    pub fn replace_sight(&mut self, sight: impl IntoIterator<Item = Hex>) {
        self.sight.clear();
        for hex in sight {
            if self.is_playable(hex) {
                if self.fog {
                    self.sight.insert(hex);
                }
                if self.shroud {
                    self.explored.insert(hex);
                }
            }
        }
    }

    /// Returns the visibility of a playable tile. Map borders lie outside this mask.
    pub fn at(&self, hex: Hex) -> Option<HexVisibility> {
        if !self.is_playable(hex) {
            return None;
        }
        Some(if self.shroud && !self.explored.contains(&hex) {
            HexVisibility::Shrouded
        } else if self.fog && !self.sight.contains(&hex) {
            HexVisibility::Fogged
        } else {
            HexVisibility::Visible
        })
    }

    fn is_playable(&self, hex: Hex) -> bool {
        (0..self.width).contains(&hex.x) && (0..self.height).contains(&hex.y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEFT: Hex = Hex { x: 0, y: 0 };
    const RIGHT: Hex = Hex { x: 1, y: 0 };
    const BORDER: Hex = Hex { x: 2, y: 0 };

    #[test]
    fn refogging_preserves_earlier_exploration() {
        let mut mask = VisibilityMask::new(2, 1, true, true);
        assert_eq!(mask.at(LEFT), Some(HexVisibility::Shrouded));

        mask.replace_sight([LEFT]);
        assert_eq!(mask.at(LEFT), Some(HexVisibility::Visible));
        assert_eq!(mask.at(RIGHT), Some(HexVisibility::Shrouded));

        mask.replace_sight([RIGHT, BORDER]);
        assert_eq!(mask.at(LEFT), Some(HexVisibility::Fogged));
        assert_eq!(mask.at(RIGHT), Some(HexVisibility::Visible));
        assert_eq!(mask.at(BORDER), None);
    }

    #[test]
    fn shroud_without_fog_keeps_explored_hexes_visible() {
        let mut mask = VisibilityMask::new(2, 1, false, true);
        mask.replace_sight([LEFT]);
        mask.replace_sight([]);

        assert_eq!(mask.at(LEFT), Some(HexVisibility::Visible));
        assert_eq!(mask.at(RIGHT), Some(HexVisibility::Shrouded));
    }

    #[test]
    fn fog_without_shroud_leaves_terrain_known_outside_sight() {
        let mut mask = VisibilityMask::new(2, 1, true, false);
        assert_eq!(mask.at(LEFT), Some(HexVisibility::Fogged));
        mask.replace_sight([LEFT]);
        assert_eq!(mask.at(LEFT), Some(HexVisibility::Visible));
        assert_eq!(mask.at(RIGHT), Some(HexVisibility::Fogged));

        let unrestricted = VisibilityMask::new(2, 1, false, false);
        assert_eq!(unrestricted.at(RIGHT), Some(HexVisibility::Visible));
        assert_eq!(unrestricted.at(BORDER), None);
    }
}
