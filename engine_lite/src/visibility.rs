use std::collections::BTreeSet;
use std::num::NonZeroU32;

use crate::hex::Hex;
use crate::map::ScenarioMap;
use crate::movement::{reachable_with_teleports, teleport_exits, MovementTile, Occupancy};
use crate::scenario_sides::{ShareVision, SideConfig};
use crate::scenario_time::location_ranges;

/// Playable hexes one viewer uncovers using its effective vision costs and range.
/// A hex too costly to enter is still visible from an adjacent reached hex.
pub fn sight_from(
    map: &ScenarioMap,
    origin: Hex,
    budget: u32,
    cost: impl Fn(&str) -> Option<NonZeroU32>,
) -> BTreeSet<Hex> {
    let mut sight = sight_with_teleports(map, origin, budget, &[], cost);
    sight.retain(|&at| map.is_playable(at));
    sight
}

pub(crate) fn sight_with_teleports(
    map: &ScenarioMap,
    origin: Hex,
    budget: u32,
    villages: &[Hex],
    cost: impl Fn(&str) -> Option<NonZeroU32>,
) -> BTreeSet<Hex> {
    if !map.is_playable(origin) {
        return BTreeSet::new();
    }
    let reached = reachable_with_teleports(origin, budget, villages, |hex| {
        if !map.is_playable(hex) {
            return None;
        }
        Some(MovementTile {
            cost: cost(map.terrain_at(hex)?)?,
            occupancy: Occupancy::Empty,
            stops_movement: false,
        })
    });

    let mut sight = BTreeSet::from([origin]);
    for hex in std::iter::once(origin).chain(reached.into_keys()) {
        sight.extend(hex.neighbors());
        // Native vision_path includes every tunnel exit in the visible edge,
        // even when entering that village would exceed the sight budget.
        sight.extend(teleport_exits(villages, hex));
    }
    // Native clear_loc also uncovers these staggered corners when their edge
    // tile enters the vision fringe. They cannot all be reached by adjacency.
    let w = map.width();
    let h = map.height();
    let right = if w % 2 == 1 { (h - 1, h) } else { (0, -1) };
    for (edge, corner) in [
        (Hex { x: 0, y: h - 1 }, Hex { x: -1, y: h }),
        (Hex { x: w - 1, y: right.0 }, Hex { x: w, y: right.1 }),
    ] {
        if sight.contains(&edge) { sight.insert(corner); }
    }
    sight
}

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
    /// Saved overrides belong to the viewer and survive refogging.
    fog_override: BTreeSet<Hex>,
    explored: BTreeSet<Hex>,
}

#[cfg(test)]
std::thread_local! {
    pub(crate) static FULL_VISION_BUILDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
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
            fog_override: BTreeSet::new(),
            explored: BTreeSet::new(),
        }
    }

    pub(crate) fn from_side(map: &ScenarioMap, side: &SideConfig) -> Self {
        let mut mask = Self::new(map.width(), map.height(), side.fog, side.shroud);
        if side.fog {
            mask.sight = read_cleared_tiles(&side.fog_data, map);
            if let Some((x, y)) = &side.fog_override {
                for (columns, rows) in location_ranges(x, y, map) {
                    for column in columns {
                        for row in rows.clone() {
                            let hex = Hex { x: column - 1, y: row - 1 };
                            if map.is_playable(hex) {
                                mask.fog_override.insert(hex);
                            }
                        }
                    }
                }
            }
        }
        if side.shroud {
            mask.explored = read_cleared_tiles(&side.shroud_data, map);
        }
        mask
    }

    /// Updates current sight and preserves the terrain uncovered by earlier sight.
    pub fn replace_sight(&mut self, sight: impl IntoIterator<Item = Hex>) {
        self.sight.clear();
        self.add_sight(sight);
    }

    /// Uncovers tiles while keeping earlier sight from this turn, including a moving unit's trail.
    pub fn add_sight(&mut self, sight: impl IntoIterator<Item = Hex>) {
        for hex in sight {
            if (-1..=self.width).contains(&hex.x) && (-1..=self.height).contains(&hex.y) {
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
        #[cfg(test)]
        FULL_VISION_BUILDS.set(FULL_VISION_BUILDS.get() + 1);
        let mut view = self.clone();
        for (donor, mask) in sides {
            let (explored, sight) = self.shared_layers(viewer, donor, mask);
            if explored {
                view.explored.extend(&mask.explored);
            }
            if sight {
                view.sight.extend(&mask.sight);
            }
        }
        view
    }

    fn shared_layers(&self, viewer: &SideConfig, donor: &SideConfig, mask: &Self) -> (bool, bool) {
        if donor.side == viewer.side || viewer.is_enemy(donor) {
            return (false, false);
        }
        (self.shroud && mask.shroud && donor.share_vision != ShareVision::None,
         self.fog && mask.fog && donor.share_vision == ShareVision::All)
    }

    /// Point lookup for display reports that only need the hovered hex and its neighbors.
    pub(crate) fn at_with_allies<'a>(
        &self, hex: Hex, viewer: &SideConfig,
        sides: impl IntoIterator<Item = (&'a SideConfig, &'a VisibilityMask)>,
    ) -> Option<HexVisibility> {
        if !self.is_playable(hex) { return None; }
        Some(self.visibility_with_allies(hex, viewer, sides))
    }

    /// Looks up a single stored hex, including the authored map border.
    pub(crate) fn visibility_with_allies<'a>(
        &self, hex: Hex, viewer: &SideConfig,
        sides: impl IntoIterator<Item = (&'a SideConfig, &'a VisibilityMask)>,
    ) -> HexVisibility {
        let mut explored = !self.shroud || self.explored.contains(&hex);
        let mut seen = !self.fog || self.sight.contains(&hex) || self.fog_override.contains(&hex);
        for (donor, mask) in sides {
            if explored && seen { break; }
            let (share_map, share_sight) = self.shared_layers(viewer, donor, mask);
            explored |= share_map && mask.explored.contains(&hex);
            seen |= share_sight && mask.sight.contains(&hex);
        }
        if !explored { HexVisibility::Shrouded }
            else if !seen { HexVisibility::Fogged }
            else { HexVisibility::Visible }
    }

    /// Returns the visibility of a playable tile. Map borders lie outside this mask.
    pub fn at(&self, hex: Hex) -> Option<HexVisibility> {
        if !self.is_playable(hex) {
            return None;
        }
        Some(self.visibility(hex))
    }

    pub(crate) fn visibility(&self, hex: Hex) -> HexVisibility {
        if self.shroud && !self.explored.contains(&hex) {
            HexVisibility::Shrouded
        } else if self.fog && !self.sight.contains(&hex) && !self.fog_override.contains(&hex) {
            HexVisibility::Fogged
        } else {
            HexVisibility::Visible
        }
    }

    fn is_playable(&self, hex: Hex) -> bool {
        (0..self.width).contains(&hex.x) && (0..self.height).contains(&hex.y)
    }
}

fn read_cleared_tiles(data: &str, map: &ScenarioMap) -> BTreeSet<Hex> {
    let mut cleared = BTreeSet::new();
    // Each pipe starts a WML column. Column zero and row zero are border cells.
    for (x, column) in data.split('|').skip(1).take(map.width() as usize + 2).enumerate() {
        let bits = column.bytes().filter(|&b| matches!(b, b'0' | b'1'));
        for (y, bit) in bits.take(map.height() as usize + 2).enumerate() {
            if bit == b'1' {
                cleared.insert(Hex { x: x as i32 - 1, y: y as i32 - 1 });
            }
        }
    }
    cleared
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{scenario_sides::read_sides, wml};

    #[test]
    fn saved_fog_overrides_survive_refog_for_their_owner_on_explored_tiles() {
        let map = ScenarioMap::parse(concat!(
            "Xv,Xv,Xv,Xv,Xv,Xv\nXv,Gg,Gg,Gg,Gg,Xv\n",
            "Xv,Gg,Gg,Gg,Gg,Xv\nXv,Xv,Xv,Xv,Xv,Xv",
        )).unwrap();
        let sides = read_sides(&wml::parse(br#"
[side]
    team_name=allies
    fog=yes
    shroud=yes
    share_vision=all
    shroud_data="|000|011|011|011|010"
    [fog_override]
        x=0-2,4-8
        y=2,1-2
    [/fog_override]
[/side]
[side]
    team_name=allies
    fog=yes
[/side]
"#).unwrap());
        let mut owner = VisibilityMask::from_side(&map, &sides[0]);
        let ally = VisibilityMask::from_side(&map, &sides[1]);
        let at = |x, y| Hex::from_wml(x, y).unwrap();
        owner.replace_sight([at(3, 2)]);
        owner.replace_sight([]);
        assert_eq!(owner.at(at(1, 2)), Some(HexVisibility::Visible));
        assert_eq!(owner.at(at(2, 1)), Some(HexVisibility::Fogged));
        assert_eq!(owner.at(at(3, 2)), Some(HexVisibility::Fogged));
        assert_eq!(owner.at(at(4, 1)), Some(HexVisibility::Visible));
        assert_eq!(owner.at(at(4, 2)), Some(HexVisibility::Shrouded));
        assert_eq!(owner.at(at(5, 2)), None);
        let shared = ally.with_allied_vision(&sides[1], [(&sides[0], &owner)]);
        assert_eq!(shared.at(at(1, 2)), Some(HexVisibility::Fogged));
    }

    const LEFT: Hex = Hex { x: 0, y: 0 };
    const RIGHT: Hex = Hex { x: 1, y: 0 };
    const BORDER: Hex = Hex { x: 2, y: 0 };

    #[test]
    fn shroud_data_uses_wml_columns_and_independent_exploration_without_fog() {
        let map = ScenarioMap::parse("Xv,Xv,Xv,Xv\nXv,Gg,Gg,Xv\nXv,Gg,Gg,Xv\nXv,Xv,Xv,Xv").unwrap();
        let sides = read_sides(&wml::parse(r#"
[side]
    shroud=yes
    shroud_data="junk1|111|1 0 1 1|0
10|111"
    fog_data="|111|111|111"
[/side]
"#).unwrap());
        let mask = VisibilityMask::from_side(&map, &sides[0]);
        assert_eq!(mask.explored, [
            (-1,-1), (-1,0), (-1,1), (0,-1), (0,1), (0,2), (1,0), (2,-1), (2,0), (2,1),
        ].into_iter().map(|(x,y)| Hex { x, y }).collect());
        assert_eq!(mask.at(Hex { x: 0, y: 1 }), Some(HexVisibility::Visible));
        assert_eq!(mask.at(Hex { x: 0, y: 0 }), Some(HexVisibility::Shrouded));
    }

    #[test]
    fn sight_reveals_the_front_of_a_costly_or_impassable_tile() {
        let map = ScenarioMap::parse(
            "Xx,Xx,Xx,Xx,Xx,Xx\n\
             Xx,Gg,Gg,Hh,Gg,Xx\n\
             Xx,Xx,Xx,Xx,Xx,Xx",
        )
        .unwrap();
        let hill = Hex { x: 2, y: 0 };
        for hill_cost in [None, NonZeroU32::new(4)] {
            let cost = |terrain: &str| match terrain {
                "Hh" => hill_cost,
                _ => NonZeroU32::new(1),
            };
            assert_eq!(
                sight_from(&map, LEFT, 3, cost),
                BTreeSet::from([LEFT, RIGHT, hill])
            );
            assert_eq!(
                sight_from(&map, LEFT, 0, cost),
                BTreeSet::from([LEFT, RIGHT])
            );
        }
    }

    #[test]
    fn a_cheaper_route_reveals_the_fringe_beyond_its_last_tile() {
        let map = ScenarioMap::parse(
            "Xx,Xx,Xx,Xx,Xx,Xx,Xx\n\
             Xx,Xx,Gg,Gg,Xx,Xx,Xx\n\
             Xx,Gg,Hh,Gg,Gg,Gg,Xx\n\
             Xx,Xx,Xx,Xx,Xx,Xx,Xx",
        )
        .unwrap();
        let cost = |terrain: &str| match terrain {
            "Hh" => NonZeroU32::new(3),
            "Gg" => NonZeroU32::new(1),
            _ => None,
        };
        let origin = Hex { x: 0, y: 1 };
        assert!(sight_from(&map, origin, 3, cost).contains(&Hex { x: 4, y: 1 }));
        assert!(!sight_from(&map, origin, 2, cost).contains(&Hex { x: 4, y: 1 }));
    }

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
        donor.replace_sight([RIGHT, BORDER]);

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
        fog_only.add_sight([RIGHT, BORDER]);
        let with_fog_only = raw.with_allied_vision(viewer, [(&sides[2], &fog_only)]);
        assert_eq!(with_fog_only.at(RIGHT), Some(HexVisibility::Shrouded));
        let mut shroud_only = VisibilityMask::new(2, 1, false, true);
        shroud_only.add_sight([RIGHT, BORDER]);
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

        donor.fog_override.insert(LEFT);
        let all_donors = [(&sides[1], &shroud_only), (&sides[2], &fog_only)];
        for (viewer_side, donors) in [
            (viewer, &all_donors[..]), (viewer, &[(&sides[1], &donor)][..]),
            (viewer, &[(&sides[2], &donor)][..]), (viewer, &[(&sides[3], &donor)][..]),
            (&sides[2], &[(viewer, &donor)][..]),
        ] {
            for (fog, shroud) in [(true, true), (false, true), (true, false), (false, false)] {
                let own = VisibilityMask::new(2, 1, fog, shroud);
                let shared = own.with_allied_vision(viewer_side, donors.iter().copied());
                for hex in [LEFT, RIGHT, BORDER] {
                    assert_eq!(own.at_with_allies(hex, viewer_side, donors.iter().copied()), shared.at(hex));
                    assert_eq!(
                        own.visibility_with_allies(hex, viewer_side, donors.iter().copied()),
                        shared.visibility(hex),
                    );
                }
            }
        }
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
