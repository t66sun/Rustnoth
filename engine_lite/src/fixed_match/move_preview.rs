//! Single-turn route and destination marks available to the moving side.
use super::*;
use crate::visibility::HexVisibility;

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
        // The route search checks every entry but starts at the mover even in shroud.
        if !matches!(self.visibility_at(turn.side, from), Some(HexVisibility::Visible | HexVisibility::Fogged)) {
            return None;
        }
        let route = self.planned_route(turn.side, from, to).ok()?;
        let board = &self.scenario.board;
        let unit = board.unit_at(from)?;
        let map = board.map();
        let steps = route
            .into_iter()
            .map(|(hex, _)| {
                let code = map.terrain_at(hex)?;
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
        let terrain = map.terrain_at(to)?;
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
        let occupant = |hex| board.unit_at(hex).filter(|other| {
            other.side == turn.side
                || (self.visibility_at(turn.side, hex) == Some(HexVisibility::Visible)
                    && self.unit_visible_on_known_hex(turn.side, hex))
        });
        let sides = &self.scenario.sides;
        let mut enemies = to.neighbors().filter_map(occupant).filter(|other| {
            sides[turn.side - 1].is_enemy(&sides[other.side - 1])
        });
        let first_enemy = enemies.next();
        let zone_of_control = !unit.unit_type.skirmisher
            && (first_enemy.is_some_and(UnitState::emits_zoc) || enemies.any(UnitState::emits_zoc));
        let hidden = first_enemy.is_none() && !unit.unit_type.hiding.is_empty() && unit.hides_on(
            terrain,
            self.lawful_bonus_in_view(turn.round, to, |hex| occupant(hex).is_some()),
            false,
        );
        Some(MovePreview {
            steps,
            defense: 100u8.saturating_sub(hit),
            // Own village ownership is known even in fog.
            capture: self.rules.terrain.features(terrain).village
                && self.village_owners.get(&to) != Some(&turn.side),
            zone_of_control,
            hidden,
        })
    }
}
