//! Single-turn route and destination marks available to the moving side.
use super::*;

#[derive(Debug, PartialEq, Eq)]
pub struct MovePreview {
    /// Terrain movement cost on each step, including the origin.
    pub steps: Vec<(Hex, u32)>,
    pub defense: u8,
    pub capture: bool,
    pub zone_of_control: bool,
    pub hidden: bool,
}

impl FixedMatch<'_> {
    pub fn move_preview(&self, from: Hex, to: Hex) -> Option<MovePreview> {
        let MatchState::Playing(turn) = self.state else {
            return None;
        };
        if from == to {
            return None;
        }
        let route = self.planned_route(turn.side, from, to).ok()?;
        let view = self.observation_for(turn.side)?;
        let tiles: BTreeMap<_, _> = view
            .tiles
            .iter()
            .map(|tile| (tile.position, tile))
            .collect();
        let unit = self.scenario.board.unit_at(from)?;
        let steps = route
            .into_iter()
            .map(|(hex, _)| {
                let code = &tiles.get(&hex)?.terrain;
                let cost = self
                    .rules
                    .walking_cost(unit.unit_type, code)
                    .map_or(99, NonZeroU32::get);
                Some((
                    hex,
                    if unit.slowed {
                        cost.saturating_mul(2)
                    } else {
                        cost
                    },
                ))
            })
            .collect::<Option<Vec<_>>>()?;
        let tile = tiles.get(&to)?;
        let terrain = &tile.terrain;
        let inherited = self
            .rules
            .movement
            .get(&unit.unit_type.movement_type)
            .map(|m| &m.defense);
        let hit = unit.unit_type.defense_overrides.hit_chance_with_fallback(
            self.rules.terrain,
            terrain,
            inherited,
        );
        let mut enemies = view.units.iter().filter(|other| {
            !view.allied_sides.contains(&other.side) && other.position.distance(to) == 1
        });
        let zone_of_control =
            !unit.unit_type.skirmisher && enemies.clone().any(|other| other.emits_zoc);
        let lawful = self.lawful_bonus_in_view(turn.round, to, |hex| {
            view.units.iter().any(|other| other.position == hex)
        });
        Some(MovePreview {
            steps,
            defense: 100u8.saturating_sub(hit),
            capture: self.rules.terrain.features(terrain).village
                && tile.village_owner != Some(turn.side),
            zone_of_control,
            hidden: enemies.next().is_none() && unit.hides_on(terrain, lawful, false),
        })
    }
}
