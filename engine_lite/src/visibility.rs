use std::collections::BTreeSet;

use crate::{
    hex::Hex,
    scenario_sides::{ShareVision, SideConfig},
};

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
#[derive(Clone)]
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
        self.add_sight(sight);
    }

    /// Uncovers tiles while keeping earlier sight from this turn, including a moving unit's trail.
    pub fn add_sight(&mut self, sight: impl IntoIterator<Item = Hex>) {
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

    /// Builds the viewer's effective mask from sides' individual, unshared masks.
    /// The roster may include the viewer and enemies. An ally contributes only the layers
    /// it has enabled and offers through its own `share_vision` setting.
    pub fn with_allied_vision<'a>(
        &self,
        viewer: &SideConfig,
        sides: impl IntoIterator<Item = (&'a SideConfig, &'a VisibilityMask)>,
    ) -> Self {
        let mut view = self.clone();
        for (donor, mask) in sides {
            if donor.side == viewer.side || viewer.is_enemy(donor) {
                continue;
            }
            if view.shroud && mask.shroud && donor.share_vision != ShareVision::None {
                view.explored.extend(&mask.explored);
            }
            if view.fog && mask.fog && donor.share_vision == ShareVision::All {
                view.sight.extend(&mask.sight);
            }
        }
        view
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
    use crate::{scenario_sides::read_sides, wml};

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
    fn movement_trail_stays_in_sight_until_refogging() {
        let mut mask = VisibilityMask::new(2, 1, true, true);
        mask.add_sight([LEFT]);
        mask.add_sight([RIGHT]);
        assert_eq!(mask.at(LEFT), Some(HexVisibility::Visible));
        assert_eq!(mask.at(RIGHT), Some(HexVisibility::Visible));

        mask.replace_sight([RIGHT]);
        assert_eq!(mask.at(LEFT), Some(HexVisibility::Fogged));
        assert_eq!(mask.at(RIGHT), Some(HexVisibility::Visible));
    }

    #[test]
    fn only_allied_donors_share_the_enabled_layers_they_offer() {
        let roster = wml::parse(
            "[side]\nteam_name=red\nshare_vision=none\n[/side]\n\
             [side]\nteam_name=red\nshare_vision=shroud\n[/side]\n\
             [side]\nteam_name=red\nshare_vision=all\n[/side]\n\
             [side]\nteam_name=blue\nshare_vision=all\n[/side]",
        )
        .unwrap();
        let sides = read_sides(&roster);
        let viewer = &sides[0];
        let raw = VisibilityMask::new(2, 1, true, true);
        let mut donor = VisibilityMask::new(2, 1, true, true);
        donor.add_sight([LEFT]);
        donor.replace_sight([RIGHT]);

        let only_map = raw.with_allied_vision(viewer, [(&sides[1], &donor)]);
        assert_eq!(only_map.at(LEFT), Some(HexVisibility::Fogged));
        assert_eq!(only_map.at(RIGHT), Some(HexVisibility::Fogged));
        let full = raw.with_allied_vision(viewer, [(&sides[2], &donor)]);
        assert_eq!(full.at(LEFT), Some(HexVisibility::Fogged));
        assert_eq!(full.at(RIGHT), Some(HexVisibility::Visible));
        let enemy = raw.with_allied_vision(viewer, [(&sides[3], &donor)]);
        assert_eq!(enemy.at(RIGHT), Some(HexVisibility::Shrouded));
        let none = raw.with_allied_vision(&sides[2], [(viewer, &donor)]);
        assert_eq!(none.at(RIGHT), Some(HexVisibility::Shrouded));
        assert_eq!(raw.at(RIGHT), Some(HexVisibility::Shrouded));

        let mut fog_only = VisibilityMask::new(2, 1, true, false);
        fog_only.add_sight([RIGHT]);
        let with_fog_only = raw.with_allied_vision(viewer, [(&sides[2], &fog_only)]);
        assert_eq!(with_fog_only.at(RIGHT), Some(HexVisibility::Shrouded));
        let mut shroud_only = VisibilityMask::new(2, 1, false, true);
        shroud_only.add_sight([RIGHT]);
        let with_shroud_only = raw.with_allied_vision(viewer, [(&sides[2], &shroud_only)]);
        assert_eq!(with_shroud_only.at(RIGHT), Some(HexVisibility::Fogged));
        let both =
            raw.with_allied_vision(viewer, [(&sides[1], &shroud_only), (&sides[2], &fog_only)]);
        assert_eq!(both.at(RIGHT), Some(HexVisibility::Visible));
        let view_without_shroud = VisibilityMask::new(2, 1, true, false)
            .with_allied_vision(viewer, [(&sides[2], &fog_only)]);
        assert_eq!(view_without_shroud.at(LEFT), Some(HexVisibility::Fogged));
        assert_eq!(view_without_shroud.at(RIGHT), Some(HexVisibility::Visible));
        let view_without_fog = VisibilityMask::new(2, 1, false, true)
            .with_allied_vision(viewer, [(&sides[1], &shroud_only)]);
        assert_eq!(view_without_fog.at(RIGHT), Some(HexVisibility::Visible));
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
