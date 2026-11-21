//! Visible enemy ranges in the native Actions menu. These are display data only.
use super::FixedMatch;
use crate::{hex::Hex, movement::{MovementTile, Occupancy, reachable_with_teleports}};
use std::{collections::{BTreeMap, BTreeSet}, num::NonZeroU32};

#[derive(Debug, Default, PartialEq, Eq)]
pub struct EnemyReach {
    /// The number of visible opponents able to reach each hex next turn.
    pub counts: BTreeMap<Hex, usize>,
    /// Opponents able to reach the cursor hex when the command was invoked.
    pub threatening: BTreeSet<Hex>,
}

impl FixedMatch<'_> {
    pub fn enemy_reach(&self, viewer: usize, only: Option<Hex>, goal: Option<Hex>) -> EnemyReach {
        let mut result = EnemyReach::default();
        let Some(view) = self.observation_for(viewer) else { return result };
        let tiles: BTreeMap<_, _> = view.tiles.iter().map(|tile| (tile.position, tile)).collect();
        let visible: BTreeMap<_, _> = view.units.iter().map(|u| (u.position, u)).collect();

        for enemy in view.units.iter().filter(|u| !u.petrified && !view.allied_sides.contains(&u.side)) {
            if only.is_some_and(|source| source != enemy.position) { continue; }
            // The projected unit is visible. Its public type and max movement are also
            // shown in its unit report. Never query the opponent's vision or move list.
            let Some(unit) = self.scenario.board.unit_at(enemy.position) else { continue };
            let side = &self.scenario.sides[enemy.side - 1];
            let hostile = |other: usize| side.is_enemy(&self.scenario.sides[other - 1]);
            let villages: Vec<_> = if unit.unit_type.village_teleport {
                tiles.values().filter(|t| t.village_owner == Some(enemy.side)
                    && (t.position == enemy.position || !visible.contains_key(&t.position)))
                    .map(|t| t.position).collect()
            } else { Vec::new() };
            let reachable = reachable_with_teleports(enemy.position, unit.max_movement().max(0) as u32,
                &villages, |hex| {
                    let tile = tiles.get(&hex)?;
                    let base = self.rules.walking_cost(unit.unit_type, &tile.terrain)?;
                    let cost = if enemy.slowed { base.checked_mul(NonZeroU32::new(2)?)? } else { base };
                    // Native path highlights include pass-through allies. Their hexes
                    // are traversable even though the mover cannot end an order there.
                    let blocked = visible.get(&hex).is_some_and(|occupant| hostile(occupant.side));
                    let stops_movement = !unit.unit_type.skirmisher && hex.neighbors().any(|next| {
                        visible.get(&next).is_some_and(|occupant| occupant.emits_zoc && hostile(occupant.side))
                    });
                    Some(MovementTile { cost, occupancy: if blocked { Occupancy::Enemy } else { Occupancy::Empty }, stops_movement })
                });
            if goal.is_some_and(|at| at == enemy.position || reachable.contains_key(&at)) {
                result.threatening.insert(enemy.position);
            }
            for at in std::iter::once(enemy.position).chain(reachable.into_keys()) {
                *result.counts.entry(at).or_default() += 1;
            }
        }
        result
    }
}
