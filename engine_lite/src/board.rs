use std::{
    borrow::Cow,
    collections::{
        BTreeMap, BTreeSet,
        btree_map::{Entry, VacantEntry},
    },
    error, fmt,
    num::NonZeroU32,
};

use crate::{
    hex::Hex,
    income::Upkeep,
    map::ScenarioMap,
    movement::{MovementTile, Occupancy, reachable_destinations},
    rng::ReplayRng,
    starting_units::{AuthoredStartingUnit, StartingUnit},
    terrain_aliases::TerrainAliases,
    trait_effects::{RecruitStats, increase_percent},
    turn::Turn,
    unit_type::UnitType,
    wml::Node,
};

mod attack;
mod concealment;
mod walk_route;
pub use attack::{Attack, AttackChoice, AttackError, StrikeModifiers};
pub(crate) use attack::{StrikeRole, combatant, within_range};

const REST_HEALING: i64 = 2;
pub(crate) const DEFAULT_EXPERIENCE_MODIFIER: i32 = 100;

/// Full referee state for the fixed map and its units.
///
/// Use a side-specific observation when supplying information to a policy.
#[derive(Debug)]
pub struct Battlefield<'rules> {
    map: ScenarioMap,
    units: BTreeMap<Hex, UnitState<'rules>>,
    recall: BTreeMap<usize, Vec<UnitState<'rules>>>,
    resting: BTreeSet<Hex>,
    unit_types: &'rules BTreeMap<String, UnitType>,
    experience_modifier: i32,
}

#[derive(Debug)]
pub struct UnitState<'rules> {
    pub side: usize,
    pub id: Option<String>,
    pub unit_type: &'rules UnitType,
    pub hitpoints: i32,
    pub poisoned: bool,
    pub(crate) unplagueable: bool,
    pub(crate) undead_variation: Option<String>,
    pub(crate) experience: i32,
    pub(crate) slowed: bool,
    uncovered: bool,
    base_max_experience: i32,
    default_amlas: u8,
    pub moves_left: i32,
    pub attacks_left: i32,
    pub can_recruit: bool,
    upkeep_override: Option<Upkeep>,
    recall_cost: i32,
    traits: Option<UnitTraits<'rules>>,
}

#[derive(Debug)]
struct UnitTraits<'rules> {
    definitions: Vec<Cow<'rules, Node>>,
    stats: RecruitStats,
}

impl<'rules> UnitTraits<'rules> {
    fn from_definitions(
        unit: &UnitType,
        definitions: Vec<Cow<'rules, Node>>,
        base_max_experience: i32,
    ) -> Option<Self> {
        (!definitions.is_empty()).then(|| Self {
            stats: RecruitStats::from_traits_with_base_experience(
                unit, definitions.iter().map(Cow::as_ref), base_max_experience,
            ),
            definitions,
        })
    }
}

impl UnitState<'_> {
    pub fn max_hitpoints(&self) -> i32 {
        let base = self.traits.as_ref().map_or(self.unit_type.hitpoints, |traits| traits.stats.max_hitpoints);
        base.saturating_add(3 * i32::from(self.default_amlas))
    }

    pub fn max_movement(&self) -> i32 {
        self.traits.as_ref().map_or(self.unit_type.movement, |traits| traits.stats.max_movement)
    }

    /// Sight points before terrain costs, independent of moves left this turn.
    pub fn vision_range(&self) -> u32 {
        let vision = self.traits.as_ref().map_or(self.unit_type.vision, |traits| traits.stats.vision);
        if vision < 0 { self.max_movement().max(0) as u32 } else { vision as u32 }
    }

    pub fn max_experience(&self) -> i32 {
        let base = self.traits.as_ref().map_or(self.base_max_experience, |traits| traits.stats.max_experience);
        (0..self.default_amlas).fold(base, |experience, _| increase_percent(experience, 20, 1))
    }

    pub fn traits(&self) -> impl Iterator<Item = &Node> {
        self.traits.iter().flat_map(|traits| traits.definitions.iter().map(Cow::as_ref))
    }

    pub fn recall_cost(&self, side_default: i32) -> i32 {
        if self.recall_cost < 0 { side_default } else { self.recall_cost }
    }

    pub(crate) fn upkeep(&self) -> i32 {
        if self.can_recruit {
            return 0;
        }
        let from_traits = self.traits.as_ref().map_or(self.unit_type.upkeep, |t| t.stats.upkeep);
        self.upkeep_override.unwrap_or(from_traits).amount(self.unit_type.level)
    }

    pub(crate) fn is_fearless(&self) -> bool {
        self.traits.as_ref().is_some_and(|traits| traits.stats.fearless)
    }

    pub(crate) fn is_unplagueable(&self) -> bool {
        self.unplagueable || self.traits.as_ref().is_some_and(|traits| traits.stats.unplagueable)
    }

    fn attack_damage(&self, index: usize) -> i32 {
        self.traits
            .as_ref()
            .map_or_else(
                || self.unit_type.attacks[index].damage,
                |traits| traits.stats.attack_damage[index],
            )
    }
}

impl<'rules> Battlefield<'rules> {
    /// Places authored units at their resolved locations and applies mandatory traits.
    pub fn new(
        map: ScenarioMap,
        starting_units: Vec<StartingUnit>,
        unit_types: &'rules BTreeMap<String, UnitType>,
    ) -> Result<Self, BoardError> {
        let starting_units = starting_units.into_iter().map(|unit| AuthoredStartingUnit {
            position: Some(unit.position),
            side: unit.side,
            id: unit.id,
            type_id: unit.type_id,
            can_recruit: unit.can_recruit,
            recall_cost: None,
            default_amlas: 0,
            upkeep: None,
            traits: Vec::new(),
            hitpoints: None,
            experience: 0,
            moves_left: None,
            attacks_left: None,
            poisoned: false,
            unplagueable: false,
            undead_variation: None,
            slowed: false,
            uncovered: false,
        });
        Self::from_authored_starting_units(
            map,
            starting_units,
            unit_types,
            DEFAULT_EXPERIENCE_MODIFIER,
        )
    }

    pub(crate) fn from_authored_starting_units(
        map: ScenarioMap,
        starting_units: impl IntoIterator<Item = AuthoredStartingUnit>,
        unit_types: &'rules BTreeMap<String, UnitType>,
        experience_modifier: i32,
    ) -> Result<Self, BoardError> {
        let mut board = Self {
            map,
            units: BTreeMap::new(),
            recall: BTreeMap::new(),
            resting: BTreeSet::new(),
            unit_types,
            experience_modifier,
        };
        for AuthoredStartingUnit {
            position,
            side,
            id,
            type_id,
            can_recruit,
            recall_cost,
            default_amlas,
            upkeep,
            traits: authored_traits,
            hitpoints,
            experience,
            moves_left,
            attacks_left,
            poisoned,
            unplagueable,
            undead_variation,
            slowed,
            uncovered,
        } in starting_units
        {
            let tile = match position {
                Some(hex) => Some(board.vacant_unit_slot(hex)?),
                None => None,
            };
            let unit_type = unit_types.get(&type_id).ok_or_else(|| match position {
                Some(position) => BoardError::UnknownUnitType { position, type_id },
                None => BoardError::UnknownRecallType { side, type_id },
            })?;
            let base_max_experience = scaled_experience(unit_type.experience, experience_modifier);
            let mut definitions: Vec<Cow<'_, Node>> = authored_traits.into_iter().map(Cow::Owned).collect();
            for mandatory in unit_type.traits.mandatory() {
                if !definitions.iter().any(|t| t.attribute("id") == mandatory.attribute("id")) {
                    definitions.push(Cow::Borrowed(mandatory));
                }
            }
            let traits = UnitTraits::from_definitions(unit_type, definitions, base_max_experience);
            let movement = traits.as_ref().map_or(unit_type.movement, |t| t.stats.max_movement);
            let mut unit = UnitState {
                side,
                id,
                unit_type,
                hitpoints: 0,
                default_amlas,
                poisoned,
                unplagueable,
                undead_variation,
                experience,
                slowed,
                uncovered,
                base_max_experience,
                moves_left: moves_left.unwrap_or(movement).max(0),
                attacks_left: attacks_left.unwrap_or(unit_type.max_attacks).max(0),
                can_recruit,
                upkeep_override: upkeep,
                recall_cost: recall_cost.unwrap_or(unit_type.recall_cost),
                traits,
            };
            unit.hitpoints = hitpoints.unwrap_or_else(|| unit.max_hitpoints());
            if let Some(tile) = tile {
                tile.insert(unit);
            } else {
                board.recall.entry(side).or_default().push(unit);
            }
        }
        Ok(board)
    }

    pub fn map(&self) -> &ScenarioMap {
        &self.map
    }

    pub fn unit_at(&self, position: Hex) -> Option<&UnitState<'rules>> {
        self.units.get(&position)
    }

    /// Authored reserve units for the requested side, in creation order.
    pub fn recall_roster(&self, side: usize) -> &[UnitState<'rules>] {
        self.recall.get(&side).map_or(&[], Vec::as_slice)
    }

    pub fn unit_type(&self, id: &str) -> Option<&'rules UnitType> {
        self.unit_types.get(id)
    }

    pub(crate) fn can_advance_to(
        &self, position: Hex, target: &str, supports_type: impl Fn(&UnitType) -> bool,
    ) -> bool {
        self.unit_at(position).is_some_and(|unit|
            advancement_target(self.unit_types, unit.unit_type, Some(target), &supports_type).is_some()
        )
    }

    /// Applies type advancements in WML declaration order unless the first choice is supplied.
    pub(crate) fn advance_after_combat(
        &mut self,
        position: Hex,
        mut preferred: Option<&str>,
        supports_type: impl Fn(&UnitType) -> bool,
    ) {
        let Some(unit) = self.units.get_mut(&position) else {
            return;
        };

        // Wesnoth limits successive advancements at one action boundary to 20.
        for _ in 0..20 {
            let required = unit.max_experience().max(1);
            if unit.experience < required {
                break;
            }
            if unit.unit_type.advances_to.is_empty() && unit.unit_type.default_amla && unit.default_amlas < 100 {
                unit.experience -= required;
                unit.default_amlas += 1;
                unit.hitpoints = unit.max_hitpoints();
                unit.slowed = false;
                unit.poisoned = false;
                continue;
            }
            let Some(next_type) = advancement_target(
                self.unit_types, unit.unit_type, preferred.take(), &supports_type,
            )
            else {
                break;
            };

            unit.experience -= required;
            unit.unit_type = next_type;
            unit.undead_variation = None;
            unit.upkeep_override = None;
            unit.recall_cost = next_type.recall_cost;
            unit.base_max_experience =
                scaled_experience(next_type.experience, self.experience_modifier);
            if let Some(traits) = &mut unit.traits {
                traits.stats = RecruitStats::from_traits_with_base_experience(
                    next_type,
                    traits.definitions.iter().map(Cow::as_ref),
                    unit.base_max_experience,
                );
            }
            unit.hitpoints = unit.max_hitpoints();
            unit.slowed = false;
            unit.poisoned = false;
            unit.moves_left = unit.moves_left.min(unit.max_movement().max(0));
            unit.attacks_left = unit.attacks_left.min(next_type.max_attacks.max(0));
        }
    }

    /// Places a base recruit after the referee checks recruitment eligibility.
    pub fn place_recruit(
        &mut self,
        side: usize,
        type_id: &str,
        position: Hex,
    ) -> Result<(), BoardError> {
        self.place_recruit_with(side, type_id, position, |_, _| None)
    }

    pub(crate) fn place_random_recruit(
        &mut self,
        side: usize,
        type_id: &str,
        position: Hex,
        rng: &mut ReplayRng,
    ) -> Result<(), BoardError> {
        self.place_recruit_with(side, type_id, position, |unit_type, base_max_experience| {
            UnitTraits::from_definitions(
                unit_type, unit_type.traits.select_for_recruit(rng).into_iter().map(Cow::Borrowed).collect(),
                base_max_experience,
            )
        })
    }

    pub(crate) fn place_plague_unit(
        &mut self, side: usize, base: &'rules UnitType, victim_variation: &str, position: Hex, rng: &mut ReplayRng,
    ) {
        self.place_random_recruit(side, &base.id, position, rng)
            .expect("validated plague type and newly vacated hex");
        let raised = self.units.get_mut(&position).expect("just placed plague unit");
        let variation = base.variation(victim_variation);
        if variation.variation_id != base.variation_id {
            raised.unit_type = variation;
            raised.recall_cost = variation.recall_cost;
            raised.base_max_experience = scaled_experience(variation.experience, self.experience_modifier);
            if let Some(traits) = &mut raised.traits {
                traits.stats = RecruitStats::from_traits_with_base_experience(
                    variation, traits.definitions.iter().map(Cow::as_ref), raised.base_max_experience,
                );
            }
            raised.hitpoints = raised.max_hitpoints();
        }
    }

    fn place_recruit_with(
        &mut self,
        side: usize,
        type_id: &str,
        position: Hex,
        select: impl FnOnce(&'rules UnitType, i32) -> Option<UnitTraits<'rules>>,
    ) -> Result<(), BoardError> {
        let experience_modifier = self.experience_modifier;
        let unit_type = self.unit_type(type_id);
        let tile = self.vacant_unit_slot(position)?;
        let unit_type = unit_type.ok_or_else(|| BoardError::UnknownUnitType {
            position,
            type_id: type_id.to_owned(),
        })?;
        let base_max_experience = scaled_experience(unit_type.experience, experience_modifier);
        let traits = select(unit_type, base_max_experience);
        tile.insert(UnitState {
            side,
            id: None,
            unit_type,
            default_amlas: 0,
            hitpoints: traits.as_ref().map_or(unit_type.hitpoints, |traits| traits.stats.max_hitpoints),
            poisoned: false,
            experience: 0,
            unplagueable: false,
            undead_variation: None,
            base_max_experience,
            slowed: false,
            uncovered: false,
            moves_left: 0,
            attacks_left: 0,
            can_recruit: false,
            upkeep_override: None,
            recall_cost: unit_type.recall_cost,
            traits,
        });
        Ok(())
    }

    fn vacant_unit_slot(
        &mut self,
        position: Hex,
    ) -> Result<VacantEntry<'_, Hex, UnitState<'rules>>, BoardError> {
        if !self.map.is_playable(position) {
            return Err(BoardError::OutsideMap(position));
        }
        match self.units.entry(position) {
            Entry::Vacant(tile) => Ok(tile),
            Entry::Occupied(_) => Err(BoardError::Occupied(position)),
        }
    }

    pub(crate) fn stop_movement(&mut self, position: Hex) {
        self.units
            .get_mut(&position)
            .expect("unit at move destination")
            .moves_left = 0;
    }

    /// Applies a current-turn move on a fully visible fixed battlefield.
    ///
    /// The referee supplies the mover's terrain cost and the active side's
    /// enemy relations. Zone of control comes from adjacent enemy base units.
    /// This handles ordinary walking without intermediate movement effects.
    pub fn move_unit(
        &mut self,
        active_side: usize,
        from: Hex,
        to: Hex,
        movement_cost: impl Fn(&str) -> Option<NonZeroU32>,
        is_enemy: impl Fn(usize) -> bool,
    ) -> Result<(), MoveError> {
        let remaining = self
            .move_destinations(active_side, from, movement_cost, is_enemy)?
            .get(&to)
            .copied()
            .ok_or(MoveError::Unreachable(to))?;

        self.finish_walk(from, to, remaining);
        Ok(())
    }

    pub(crate) fn finish_walk(&mut self, from: Hex, to: Hex, remaining: u32) {
        let mut unit = self.units.remove(&from).expect("validated move source");
        // The search can leave at most the unit's starting i32 movement budget.
        unit.moves_left = remaining as i32;
        let occupied = self.units.insert(to, unit);
        debug_assert!(occupied.is_none());
        if self.resting.remove(&from) {
            self.resting.insert(to);
        }
    }

    /// Returns legal destinations in hex order and the movement left at each.
    ///
    /// The referee must use this on a fully visible battlefield with the
    /// active side's terrain costs and enemy relations.
    pub fn move_destinations(
        &self,
        active_side: usize,
        from: Hex,
        movement_cost: impl Fn(&str) -> Option<NonZeroU32>,
        is_enemy: impl Fn(usize) -> bool,
    ) -> Result<BTreeMap<Hex, u32>, MoveError> {
        self.move_destinations_in_view(active_side, from, movement_cost, is_enemy, |_| true, |_| true)
    }

    /// Returns movement candidates using the active side's knowledge.
    ///
    /// Fogged terrain remains known. Shrouded terrain is never queried for a
    /// walking cost. Only own units and visible foreign units occupy hexes
    /// or exert zones of control for this search.
    pub fn move_destinations_in_view(
        &self,
        active_side: usize,
        from: Hex,
        movement_cost: impl Fn(&str) -> Option<NonZeroU32>,
        is_enemy: impl Fn(usize) -> bool,
        terrain_known: impl Fn(Hex) -> bool,
        unit_visible: impl Fn(Hex) -> bool,
    ) -> Result<BTreeMap<Hex, u32>, MoveError> {
        self.search_walking_map(
            active_side, from, movement_cost, is_enemy, terrain_known, unit_visible,
            |moves, tile_at| reachable_destinations(from, moves, |hex| Some(tile_at(hex)?.0)),
        )
    }

    /// Plans a single-turn walk in the mover's view and includes both endpoints.
    /// The mover's chance to be hit on each terrain breaks equal-MP paths.
    pub fn walk_route_in_view(
        &self,
        active_side: usize,
        from: Hex,
        to: Hex,
        movement_cost: impl Fn(&str) -> Option<NonZeroU32>,
        hit_chance: impl Fn(&str) -> u8,
        is_enemy: impl Fn(usize) -> bool,
        terrain_known: impl Fn(Hex) -> bool,
        unit_visible: impl Fn(Hex) -> bool,
    ) -> Result<Vec<Hex>, MoveError> {
        self.walk_steps_in_view(active_side, from, to, movement_cost, hit_chance, is_enemy,
            terrain_known, unit_visible)
            .map(|steps| steps.into_iter().map(|(hex, _)| hex).collect())
    }

    /// Movement remaining after each step of the side's chosen route, including its origin.
    pub(crate) fn walk_steps_in_view(
        &self,
        active_side: usize,
        from: Hex,
        to: Hex,
        movement_cost: impl Fn(&str) -> Option<NonZeroU32>,
        hit_chance: impl Fn(&str) -> u8,
        is_enemy: impl Fn(usize) -> bool,
        terrain_known: impl Fn(Hex) -> bool,
        unit_visible: impl Fn(Hex) -> bool,
    ) -> Result<Vec<(Hex, u32)>, MoveError> {
        self.search_walking_map(
            active_side, from, movement_cost, is_enemy, terrain_known, unit_visible,
            |moves, tile_at| walk_route::find(from, to, moves, |hex| {
                let (tile, terrain) = tile_at(hex)?;
                Some((tile, hit_chance(terrain)))
            }),
        )?.ok_or(MoveError::Unreachable(to))
    }

    fn search_walking_map<'board, T>(
        &'board self,
        active_side: usize,
        from: Hex,
        movement_cost: impl Fn(&str) -> Option<NonZeroU32>,
        is_enemy: impl Fn(usize) -> bool,
        terrain_known: impl Fn(Hex) -> bool,
        unit_visible: impl Fn(Hex) -> bool,
        search: impl FnOnce(u32, &dyn Fn(Hex) -> Option<(MovementTile, &'board str)>) -> T,
    ) -> Result<T, MoveError> {
        let known_unit_at = |hex| {
            if !self.map.is_playable(hex) {
                return None;
            }
            let visible = unit_visible(hex);
            self.unit_at(hex).filter(|unit| visible || unit.side == active_side)
        };
        let unit = known_unit_at(from).ok_or(MoveError::MissingUnit(from))?;
        if unit.side != active_side {
            return Err(MoveError::WrongSide(from));
        }
        let moves_left = unit.moves_left.max(0) as u32;
        let hostile = |unit: &UnitState<'rules>| unit.side != active_side && is_enemy(unit.side);

        Ok(search(moves_left, &|position| {
            if !self.map.is_playable(position) || !terrain_known(position) {
                return None;
            }
            let terrain = self.map.terrain_at(position)?;
            let cost = movement_cost(terrain)?;
            let cost = if unit.slowed {
                cost.checked_mul(NonZeroU32::new(2).unwrap())?
            } else {
                cost
            };
            let occupancy = match known_unit_at(position) {
                Some(unit) if hostile(unit) => Occupancy::Enemy,
                Some(_) => Occupancy::Ally,
                None => Occupancy::Empty,
            };
            let stops_movement = !unit.unit_type.skirmisher && position.neighbors().any(|neighbor| {
                known_unit_at(neighbor)
                    .is_some_and(|unit| hostile(unit) && unit.unit_type.emits_zoc)
            });
            Some((MovementTile {
                cost,
                occupancy,
                stops_movement,
            }, terrain))
        }))
    }

    /// Iterates over units in hex order.
    pub fn units(&self) -> impl Iterator<Item = (Hex, &UnitState<'rules>)> + '_ {
        self.units.iter().map(|(&position, unit)| (position, unit))
    }

    /// Advances the referee's side turn and refreshes the incoming side's actions.
    pub fn advance_turn(
        &mut self,
        current: Turn,
        active_sides: &[usize],
        last_round: Option<i32>,
    ) -> Option<Turn> {
        for unit in self.units.values_mut().filter(|unit| unit.side == current.side) {
            unit.slowed = false;
        }
        let next = current.next(active_sides, last_round)?;
        self.resting.retain(|hex| {
            let unit = &self.units[hex];
            unit.side != current.side || unit.moves_left == unit.max_movement().max(0)
        });
        self.refresh_units(next);
        Some(next)
    }

    pub(crate) fn refresh_units(&mut self, turn: Turn) {
        if turn.round <= 1 {
            return;
        }
        for unit in self.units.values_mut().filter(|unit| unit.side == turn.side) {
            unit.moves_left = unit.max_movement().max(0);
            unit.attacks_left = unit.unit_type.max_attacks.max(0);
            unit.uncovered = false;
        }
    }

    pub(crate) fn begin_resting(&mut self, side: usize) {
        self.resting.extend(
            self.units.iter().filter(|(_, unit)| unit.side == side).map(|(&hex, _)| hex),
        );
    }

    /// Applies terrain, adjacent healers and rest before marking a new turn's rest.
    pub(crate) fn heal_at_turn_start(
        &mut self,
        side: usize,
        terrain: &TerrainAliases,
        is_enemy: impl Fn(usize, usize) -> bool,
    ) {
        #[derive(Default)]
        struct Treatment {
            hitpoints: i32,
            suppresses_poison: bool,
            cures_poison: bool,
        }
        let mut treatments = BTreeMap::<Hex, Treatment>::new();
        for (&hex, healer) in &self.units {
            let amount = healer.unit_type.adjacent_healing;
            let cures = healer.unit_type.cures_adjacent;
            if amount == 0 && !cures {
                continue;
            }
            for neighbor in hex.neighbors() {
                let Some(patient) = self.units.get(&neighbor) else { continue };
                if (healer.side != side && patient.side != side) || is_enemy(healer.side, patient.side) {
                    continue;
                }
                let treatment = treatments.entry(neighbor).or_default();
                treatment.suppresses_poison = true;
                if healer.side == side {
                    treatment.hitpoints = treatment.hitpoints.max(amount);
                    treatment.cures_poison |= cures;
                }
            }
        }
        for (&hex, unit) in &mut self.units {
            let active = unit.side == side;
            let treatment = treatments.remove(&hex).unwrap_or_default();
            let (terrain_healing, regeneration) = if active {
                let terrain_hp = self.map.terrain_at(hex).map_or(0, |code| terrain.features(code).healing);
                (terrain_hp, unit.unit_type.regeneration)
            } else {
                (0, 0)
            };
            let healthy = unit.traits.as_ref().is_some_and(|traits| traits.stats.healthy);
            let rest_healing = if active && (healthy || self.resting.contains(&hex)) { REST_HEALING } else { 0 };
            let main_healing = if unit.poisoned {
                if treatment.cures_poison || terrain_healing != 0 || regeneration > 0 {
                    unit.poisoned = false;
                    0
                } else if active && !treatment.suppresses_poison {
                    -i64::from(crate::combat::POISON_AMOUNT)
                } else {
                    0
                }
            } else {
                i64::from(terrain_healing.max(treatment.hitpoints).max(regeneration))
                    + i64::from(terrain_healing.min(0))
            };
            let healing = main_healing + rest_healing;
            let hitpoints = i64::from(unit.hitpoints);
            let max_heal = (i64::from(unit.max_hitpoints()) - hitpoints).max(0);
            let min_heal = (1 - hitpoints).min(0);
            unit.hitpoints = (hitpoints + healing.clamp(min_heal, max_heal)) as i32;
        }
        self.begin_resting(side);
    }
}

fn advancement_target<'a>(
    catalog: &'a BTreeMap<String, UnitType>,
    current: &UnitType,
    preferred: Option<&str>,
    supports_type: &impl Fn(&UnitType) -> bool,
) -> Option<&'a UnitType> {
    let id = match preferred {
        Some(id) if current.advances_to.iter().any(|target| target == id) => id,
        Some(_) => return None,
        None => current.advances_to.first()?,
    };
    let candidate = catalog.get(id)?.variation(&current.variation_id);
    supports_type(candidate).then_some(candidate)
}

fn scaled_experience(base: i32, modifier: i32) -> i32 {
    let scaled = (i64::from(base) * i64::from(modifier) + 50) / 100;
    scaled.clamp(1, i64::from(i32::MAX)) as i32
}

#[derive(Debug, PartialEq, Eq)]
pub enum BoardError {
    OutsideMap(Hex),
    Occupied(Hex),
    UnknownUnitType { position: Hex, type_id: String },
    UnknownRecallType { side: usize, type_id: String },
}

impl fmt::Display for BoardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutsideMap(hex) => write!(f, "placement hex {hex:?} is outside the playable map"),
            Self::Occupied(hex) => write!(f, "placement hex {hex:?} already has a unit"),
            Self::UnknownUnitType { position, type_id } => {
                write!(f, "unknown unit type {type_id:?} at placement hex {position:?}")
            }
            Self::UnknownRecallType { side, type_id } => {
                write!(f, "unknown unit type {type_id:?} in side {side}'s recall list")
            }
        }
    }
}

impl error::Error for BoardError {}

#[derive(Debug, PartialEq, Eq)]
pub enum MoveError {
    MissingUnit(Hex),
    WrongSide(Hex),
    Unreachable(Hex),
}

impl fmt::Display for MoveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingUnit(hex) => write!(f, "no unit to move at {hex:?}"),
            Self::WrongSide(hex) => write!(f, "unit at {hex:?} is not on the active side"),
            Self::Unreachable(hex) => write!(f, "cannot reach {hex:?} this turn"),
        }
    }
}

impl error::Error for MoveError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{starting_units::read_starting_units, wml};

    const WEST: Hex = Hex { x: 0, y: 0 };
    const EAST: Hex = Hex { x: 1, y: 0 };

    fn map() -> ScenarioMap {
        ScenarioMap::parse("Xv,Xv,Xv,Xv\nXv,2 Kh,1 Kh,Xv\nXv,Xv,Xv,Xv").unwrap()
    }

    fn types() -> BTreeMap<String, UnitType> {
        let config = wml::parse("id=Spearman\nhitpoints=36\nmovement=5\nattacks=2\nlevel=1").unwrap();
        let unit_type = UnitType::from_wml(config);
        BTreeMap::from([(unit_type.id.clone(), unit_type)])
    }

    #[test]
    fn sight_uses_vision_and_walking_tables_at_each_terrain_alias_leaf() {
        use crate::{fixed_match::Rules, movement_types::read_movement_types};

        let falconer = UnitType::from_wml(wml::parse(
            "id=Dune Falconer\nmovement_type=duneelusivefoot\nmovement=6\nvision=8\n\
             [movement_costs]\ndeep_water=99\nhills=5\nforest=4\n[/movement_costs]\n\
             [vision_costs]\ndeep_water=1\n[/vision_costs]",
        ).unwrap());
        let types = BTreeMap::from([(falconer.id.clone(), falconer)]);
        let mut board = Battlefield::new(map(), Vec::new(), &types).unwrap();
        board.place_recruit(1, "Dune Falconer", EAST).unwrap();
        let movement = read_movement_types(&wml::parse(
            "[movetype]\nname=duneelusivefoot\n\
             [movement_costs]\nflat=2\nforest=90\n[/movement_costs]\n\
             [vision_costs]\nhills=3\ndeep_water=7\n[/vision_costs]\n[/movetype]",
        ).unwrap());
        let terrain = TerrainAliases::from_wml(&wml::parse(
            "[terrain_type]\nid=deep_water\nstring=Wo\n[/terrain_type]\n\
             [terrain_type]\nid=hills\nstring=Hh\n[/terrain_type]\n\
             [terrain_type]\nid=forest\nstring=Ft\n[/terrain_type]\n\
             [terrain_type]\nid=flat\nstring=Gg\n[/terrain_type]\n\
             [terrain_type]\nstring=Mx\nmvt_alias=\"-,Wo,Hh,Ft,Gg\"\n[/terrain_type]",
        ).unwrap());
        let resistance = BTreeMap::new();
        let rules = Rules { terrain: &terrain, movement: &movement, resistance: &resistance };
        let unit = board.units.get_mut(&EAST).unwrap();
        assert_eq!(unit.moves_left, 0);
        assert_eq!(unit.vision_range(), 8);
        for (code, cost) in [("Wo", 1), ("Hh", 3), ("Ft", 4), ("Gg", 2), ("Mx", 4)] {
            assert_eq!(rules.sight_cost(unit, code).get(), cost, "{code}");
        }
        unit.slowed = true;
        assert_eq!(unit.vision_range(), 8);
        assert_eq!(rules.sight_cost(unit, "Mx").get(), 8);
        assert_eq!(rules.sight_cost(unit, "Xu").get(), 99);
    }

    #[test]
    fn advancing_to_a_new_type_cures_poison() {
        let mut types = types();
        types.get_mut("Spearman").unwrap().advances_to.push("Pikeman".into());
        types.insert("Pikeman".into(), UnitType::from_wml(wml::parse("id=Pikeman").unwrap()));
        let mut board = Battlefield::new(map(), Vec::new(), &types).unwrap();
        board.place_recruit(1, "Spearman", EAST).unwrap();
        let unit = board.units.get_mut(&EAST).unwrap();
        unit.poisoned = true;
        unit.experience = unit.max_experience();
        board.advance_after_combat(EAST, None, |_| true);
        assert_eq!(board.unit_at(EAST).unwrap().unit_type.id, "Pikeman");
        assert!(!board.unit_at(EAST).unwrap().poisoned);
    }

    #[test]
    fn initializes_authored_units_from_their_types() {
        let scenario = wml::parse(
            "[side]\nid=first\ntype=Spearman\n[/side]\n[side]\ntype=Spearman\n[/side]",
        )
        .unwrap();
        let map = map();
        let starts = read_starting_units(&scenario, &map).unwrap();
        let unit_types = types();
        let board = Battlefield::new(map, starts, &unit_types).unwrap();
        let first = board.unit_at(EAST).unwrap();

        assert_eq!(board.map().terrain_at(WEST), Some("Kh"));
        assert_eq!(first.side, 1);
        assert_eq!(first.id.as_deref(), Some("first"));
        assert!(std::ptr::eq(first.unit_type, &unit_types["Spearman"]));
        assert_eq!(
            (first.hitpoints, first.moves_left, first.attacks_left),
            (36, 5, 2)
        );
        assert!(first.can_recruit);
        let positions: Vec<_> = board.units().map(|(hex, unit)| (hex, unit.side)).collect();
        assert_eq!(positions, [(WEST, 2), (EAST, 1)]);
    }

    #[test]
    fn starting_units_apply_only_mandatory_traits_before_current_stats() {
        let definition = wml::parse("id=Guard\nhitpoints=10\nmovement=4\nnum_traits=2\n\
            [trait]\nid=optional\n[/trait]\n[trait]\nid=sturdy\navailability=musthave\n\
            [effect]\napply_to=hitpoints\nincrease_total=3\n[/effect]\n\
            [effect]\napply_to=movement\nincrease=1\n[/effect]\n[/trait]").unwrap();
        let types = BTreeMap::from([("Guard".to_owned(), UnitType::from_wml(definition))]);
        let starts = [EAST, WEST].map(|position| AuthoredStartingUnit {
            traits: Vec::new(),
            default_amlas: 0,
            upkeep: None,
            position: Some(position),
            side: 1,
            id: None,
            type_id: "Guard".into(),
            can_recruit: position == EAST,
            recall_cost: None,
            hitpoints: (position == WEST).then_some(8),
            moves_left: (position == WEST).then_some(2),
            attacks_left: None,
            experience: 0,
            poisoned: false,
            unplagueable: false,
            undead_variation: None,
            slowed: false,
            uncovered: false,
        });
        let board = Battlefield::from_authored_starting_units(map(), starts, &types, 100).unwrap();
        for (position, hp, moves) in [(EAST, 13, 5), (WEST, 8, 2)] {
            let unit = board.unit_at(position).unwrap();
            assert_eq!((unit.hitpoints, unit.moves_left), (hp, moves));
            assert_eq!((unit.max_hitpoints(), unit.max_movement()), (13, 5));
            assert_eq!(unit.vision_range(), 5);
            assert_eq!(unit.traits().map(|t| t.attribute("id")).collect::<Vec<_>>(), [Some("sturdy")]);
        }
    }

    #[test]
    fn rejects_unknown_types_and_invalid_placements() {
        let unit_types = types();
        let start = |position, type_id: &str| StartingUnit {
            side: 1,
            id: None,
            type_id: type_id.to_owned(),
            position,
            can_recruit: false,
        };
        let border = Hex { x: -1, y: 0 };

        assert_eq!(
            Battlefield::new(map(), vec![start(border, "Spearman")], &unit_types).unwrap_err(),
            BoardError::OutsideMap(border)
        );
        assert_eq!(
            Battlefield::new(map(), vec![start(WEST, "missing")], &unit_types).unwrap_err(),
            BoardError::UnknownUnitType {
                position: WEST,
                type_id: "missing".to_owned(),
            }
        );
        assert_eq!(
            Battlefield::new(
                map(),
                vec![start(WEST, "Spearman"), start(WEST, "Spearman")],
                &unit_types,
            )
            .unwrap_err(),
            BoardError::Occupied(WEST)
        );
    }

    #[test]
    fn recruited_unit_enters_at_full_health_and_acts_on_its_next_turn() {
        let unit_types = types();
        let mut board = Battlefield::new(map(), Vec::new(), &unit_types).unwrap();

        board.place_recruit(2, "Spearman", EAST).unwrap();
        let recruit = board.unit_at(EAST).unwrap();
        assert_eq!(recruit.vision_range(), 5);
        assert_eq!(recruit.side, 2);
        assert!(recruit.id.is_none());
        assert!(std::ptr::eq(recruit.unit_type, &unit_types["Spearman"]));
        assert_eq!(
            (recruit.hitpoints, recruit.moves_left, recruit.attacks_left),
            (36, 0, 0)
        );
        assert!(!recruit.can_recruit);

        let current = Turn { round: 2, side: 1 };
        assert_eq!(
            board.advance_turn(current, &[1, 2], None),
            Some(Turn { round: 2, side: 2 })
        );
        let recruit = board.unit_at(EAST).unwrap();
        assert_eq!(
            (recruit.hitpoints, recruit.moves_left, recruit.attacks_left),
            (36, 5, 2)
        );
    }

    #[test]
    fn regeneration_uses_the_strongest_healing_source_before_rest() {
        let config = wml::parse("id=Troll\nhitpoints=40\n[abilities]\n[regenerate]\n\
            id=regenerates\nvalue=8\naffect_self=yes\npoison=cured\n[/regenerate]\n[/abilities]").unwrap();
        let types = BTreeMap::from([("Troll".to_owned(), UnitType::from_wml(config))]);
        let starts = [EAST, WEST].map(|position| StartingUnit {
            position, side: 1, id: None, type_id: "Troll".into(), can_recruit: false,
        });
        let mut board = Battlefield::new(map(), starts.into(), &types).unwrap();
        let terrain = |value| TerrainAliases::from_wml(&wml::parse(format!(
            "[terrain_type]\nid=keep\nstring=Kh\nheals={value}\n[/terrain_type]"
        )).unwrap());
        for (terrain_hp, expected) in [(0, 18), (8, 18), (12, 22), (-3, 15)] {
            board.resting.clear();
            board.resting.insert(WEST);
            for unit in board.units.values_mut() { unit.hitpoints = 10; }
            board.heal_at_turn_start(1, &terrain(terrain_hp), |a, b| a != b);
            assert_eq!(board.unit_at(EAST).unwrap().hitpoints, expected);
            assert_eq!(board.unit_at(WEST).unwrap().hitpoints, expected + 2);
        }
    }

    #[test]
    fn invalid_recruit_placements_leave_the_board_unchanged() {
        let unit_types = types();
        let mut board = Battlefield::new(map(), Vec::new(), &unit_types).unwrap();
        board.place_recruit(1, "Spearman", WEST).unwrap();
        let border = Hex { x: -1, y: 0 };

        assert_eq!(
            board.place_recruit(2, "Spearman", WEST),
            Err(BoardError::Occupied(WEST))
        );
        assert_eq!(
            board.place_recruit(2, "Spearman", border),
            Err(BoardError::OutsideMap(border))
        );
        assert_eq!(
            board.place_recruit(2, "missing", EAST),
            Err(BoardError::UnknownUnitType {
                position: EAST,
                type_id: "missing".to_owned()
            })
        );
        let occupied: Vec<_> = board.units().map(|(hex, unit)| (hex, unit.side)).collect();
        assert_eq!(occupied, [(WEST, 1)]);
    }

    fn corridor_hex(y: i32) -> Hex {
        Hex { x: 0, y }
    }

    fn corridor<'rules>(
        unit_types: &'rules BTreeMap<String, UnitType>,
        positions: &[(i32, usize)],
    ) -> Battlefield<'rules> {
        let map = ScenarioMap::parse(
            "Xv,Xv,Xv\nXv,Gg,Xv\nXv,Gg,Xv\nXv,Hh,Xv\nXv,Gg,Xv\nXv,Gg,Xv\nXv,Xv,Xv",
        )
        .unwrap();
        let starts = positions
            .iter()
            .map(|&(y, side)| StartingUnit {
                side,
                id: Some(format!("unit_{y}")),
                type_id: "Spearman".to_owned(),
                position: corridor_hex(y),
                can_recruit: false,
            })
            .collect();
        Battlefield::new(map, starts, unit_types).unwrap()
    }

    fn walking_cost(terrain: &str) -> Option<NonZeroU32> {
        NonZeroU32::new(if terrain == "Hh" { 2 } else { 1 })
    }

    #[test]
    fn a_visible_opponent_shows_both_ailments_and_a_concealed_one_stays_hidden() {
        use crate::{observation::project, visibility::VisibilityMask};
        let unit_types = types();
        let mut board = corridor(&unit_types, &[(0, 1), (1, 2), (2, 2)]);
        for unit in board.units.values_mut().filter(|unit| unit.side == 2) {
            unit.poisoned = true;
            unit.slowed = true;
        }
        let sight = VisibilityMask::new(1, 5, false, false);
        let seen = corridor_hex(1);
        let view = project(&board, 1, &sight, |hex| hex == seen);
        assert_eq!(view.units.len(), 2);
        let enemy = view.units.iter().find(|unit| unit.position == seen).unwrap();
        assert!(enemy.poisoned && enemy.slowed);
        assert_eq!((enemy.moves_left, enemy.attacks_left), (None, None));
    }

    #[test]
    fn adjacent_healers_use_the_strongest_treatment_and_add_rest() {
        let mut unit_types = types();
        for (id, value) in [("Mage", 8), ("Herbalist", 4)] {
            let config = wml::parse(format!(
                "id={id}\nhitpoints=36\n[abilities]\n[heals]\nid=healing\nvalue={value}\n\
                 affect_allies=yes\naffect_self=no\npoison=slowed\n[affect_adjacent]\n\
                 [/affect_adjacent]\n[/heals]\n[/abilities]"
            )).unwrap();
            unit_types.insert(id.into(), UnitType::from_wml(config));
        }
        let map = ScenarioMap::parse(
            "Xv,Xv,Xv\nXv,Gg,Xv\nXv,Gg,Xv\nXv,Hh,Xv\nXv,Gg,Xv\nXv,Gg,Xv\nXv,Xv,Xv",
        ).unwrap();
        let roster = [(1, "Spearman"), (1, "Herbalist"), (1, "Spearman"), (1, "Mage"), (2, "Mage")];
        let starts = roster
            .into_iter()
            .enumerate()
            .map(|(y, (side, id))| StartingUnit {
                position: corridor_hex(y as i32),
                side,
                type_id: id.into(),
                id: None,
                can_recruit: false,
            })
            .collect();
        let mut board = Battlefield::new(map, starts, &unit_types).unwrap();
        for unit in board.units.values_mut() {
            unit.hitpoints = 10;
        }
        board.units.get_mut(&corridor_hex(0)).unwrap().hitpoints = 35;
        let terrain = TerrainAliases::from_wml(
            &wml::parse("[terrain_type]\nid=hills\nstring=Hh\nheals=6\n[/terrain_type]").unwrap(),
        );
        board.begin_resting(1);
        board.heal_at_turn_start(1, &terrain, |a, b| a != b);
        let hp: Vec<_> = board.units().map(|(_, unit)| unit.hitpoints).collect();
        assert_eq!(hp, [36, 12, 20, 12, 10]);
        board.units.get_mut(&corridor_hex(0)).unwrap().hitpoints = 10;
        board.heal_at_turn_start(1, &terrain, |a, b| a != b);
        assert_eq!(board.unit_at(corridor_hex(0)).unwrap().hitpoints, 16);
    }

    #[test]
    fn an_adjacent_curer_removes_poison_and_preserves_only_rest_healing() {
        let config = wml::parse("id=Curer\nhitpoints=30\n[abilities]\n[heals]\n\
            id=curing\naffect_allies=yes\naffect_self=no\npoison=cured\n\
            [affect_adjacent]\n[/affect_adjacent]\n[/heals]\n[/abilities]").unwrap();
        let mut types = types();
        types.insert("Curer".into(), UnitType::from_wml(config));
        let mut board = corridor(&types, &[(0, 1), (1, 1), (2, 2)]);
        board.units.get_mut(&corridor_hex(0)).unwrap().unit_type = &types["Curer"];
        for unit in board.units.values_mut() {
            unit.poisoned = true;
            unit.hitpoints = 10;
        }
        board.begin_resting(1);
        board.heal_at_turn_start(1, &TerrainAliases::from_wml(&Node::default()), |a, b| a != b);
        let state: Vec<_> = board.units().map(|(_, u)| (u.poisoned, u.hitpoints)).collect();
        assert_eq!(state, [(true, 4), (false, 12), (true, 10)]);
    }

    #[test]
    fn healthy_units_receive_rest_healing_even_after_moving() {
        let make_type = |id: &str, disable| {
            let reset = if disable { "[effect]\napply_to=healthy\nset=no\n[/effect]\n" } else { "" };
            UnitType::from_wml(wml::parse(format!("id={id}\nhitpoints=20\nmovement=5\n\
                [trait]\nid=sturdy\navailability=musthave\n[effect]\napply_to=healthy\n\
                [/effect]\n{reset}[/trait]")).unwrap())
        };
        let types = BTreeMap::from([
            ("Healthy".into(), make_type("Healthy", false)),
            ("Ordinary".into(), make_type("Ordinary", true)),
        ]);
        let starts = [(EAST, "Healthy"), (WEST, "Ordinary")].map(|(position, kind)| StartingUnit {
            side: 1, id: None, type_id: kind.into(), position, can_recruit: false,
        });
        let mut board = Battlefield::new(map(), starts.into(), &types).unwrap();
        for unit in board.units.values_mut() { unit.hitpoints = 5; }
        let terrain = TerrainAliases::from_wml(&Node::default());
        board.heal_at_turn_start(1, &terrain, |a, b| a != b);
        assert_eq!(board.unit_at(EAST).unwrap().hitpoints, 7);
        assert_eq!(board.unit_at(WEST).unwrap().hitpoints, 5);
        board.heal_at_turn_start(1, &terrain, |a, b| a != b);
        assert_eq!(board.unit_at(EAST).unwrap().hitpoints, 9);
    }

    #[test]
    fn allied_healer_suppresses_poison_on_the_patients_turn_and_treats_on_its_own() {
        let mut types = types();
        let mut healer = UnitType::from_wml(wml::parse("id=Healer\nhitpoints=30").unwrap());
        healer.adjacent_healing = 8;
        healer.cures_adjacent = true;
        types.insert("Healer".into(), healer);
        let mut board = corridor(&types, &[(0, 3), (1, 1), (2, 2)]);
        board.units.get_mut(&corridor_hex(1)).unwrap().unit_type = &types["Healer"];
        for y in [0, 2] {
            let patient = board.units.get_mut(&corridor_hex(y)).unwrap();
            patient.hitpoints = 10;
            patient.poisoned = true;
        }
        let terrain = TerrainAliases::from_wml(&Node::default());
        let enemies = |a, b| a != b && (a == 2 || b == 2);
        let state = |board: &Battlefield<'_>, y| {
            let patient = board.unit_at(corridor_hex(y)).unwrap();
            (patient.hitpoints, patient.poisoned)
        };

        board.heal_at_turn_start(3, &terrain, enemies);
        assert_eq!(state(&board, 0), (10, true));
        board.heal_at_turn_start(1, &terrain, enemies);
        assert_eq!(state(&board, 0), (10, false));
        assert_eq!(state(&board, 2), (10, true));
        board.heal_at_turn_start(1, &terrain, enemies);
        assert_eq!(state(&board, 0), (18, false));
        board.heal_at_turn_start(2, &terrain, enemies);
        assert_eq!(state(&board, 2), (2, true));
        assert_eq!(state(&board, 0), (18, false));
    }

    #[test]
    fn standard_advancement_clears_slow() {
        let mut types = types();
        types.get_mut("Spearman").unwrap().advances_to.push("Veteran".into());
        types.insert("Veteran".into(), UnitType::from_wml(wml::parse("id=Veteran").unwrap()));
        let hex = corridor_hex(0);
        let mut board = corridor(&types, &[(0, 1)]);
        let unit = board.units.get_mut(&hex).unwrap();
        unit.slowed = true;
        unit.experience = unit.max_experience();
        board.advance_after_combat(hex, None, |_| true);
        let advanced = board.unit_at(hex).unwrap();
        assert_eq!(advanced.unit_type.id, "Veteran");
        assert!(!advanced.slowed);
    }

    #[test]
    fn slow_doubles_entry_cost_until_the_units_own_turn_ends() {
        let types = types();
        let from = corridor_hex(0);
        let to = corridor_hex(1);
        let mut board = corridor(&types, &[(0, 2)]);
        board.units.get_mut(&from).unwrap().slowed = true;
        let first = Turn { round: 1, side: 1 };
        let second = board.advance_turn(first, &[1, 2], None).unwrap();
        let options = board.move_destinations(2, from, walking_cost, |_| false).unwrap();
        assert_eq!(options, BTreeMap::from([(to, 3)]));
        board.move_unit(2, from, to, walking_cost, |_| false).unwrap();
        assert_eq!(board.unit_at(to).unwrap().moves_left, 3);
        board.advance_turn(second, &[1, 2], None).unwrap();
        assert!(!board.unit_at(to).unwrap().slowed);
        let options = board.move_destinations(2, to, walking_cost, |_| false).unwrap();
        assert_eq!(options[&corridor_hex(2)], 1);
    }

    #[test]
    fn lists_legal_destinations_with_their_execution_costs() {
        let unit_types = types();
        let from = corridor_hex(0);
        let occupied = [(0, 1), (1, 2), (4, 3)];
        let board = corridor(&unit_types, &occupied);
        let is_enemy = |side| side == 3;

        let destinations = board
            .move_destinations(1, from, walking_cost, is_enemy)
            .unwrap();
        assert_eq!(
            destinations,
            BTreeMap::from([(corridor_hex(2), 2), (corridor_hex(3), 0)]),
        );
        assert_eq!(board.unit_at(from).unwrap().moves_left, 5);
        assert_eq!(
            board.move_destinations(1, corridor_hex(1), walking_cost, is_enemy),
            Err(MoveError::WrongSide(corridor_hex(1))),
        );
        assert_eq!(
            board.move_destinations(1, corridor_hex(2), walking_cost, is_enemy),
            Err(MoveError::MissingUnit(corridor_hex(2))),
        );

        for (to, remaining) in destinations {
            let mut board = corridor(&unit_types, &occupied);
            board.move_unit(1, from, to, walking_cost, is_enemy).unwrap();
            assert_eq!(board.unit_at(to).unwrap().moves_left, remaining as i32);
        }
    }

    #[test]
    fn concealed_hostiles_do_not_change_move_candidates_or_remaining_movement() {
        let types = types();
        let empty = corridor(&types, &[(0, 1), (1, 1)]);
        let guarded = corridor(&types, &[(0, 1), (1, 1), (3, 2)]);
        let from = corridor_hex(0);
        let enemy = corridor_hex(3);
        let is_enemy = |side| side == 2;
        let concealed = guarded
            .move_destinations_in_view(1, from, walking_cost, is_enemy, |_| true, |_| false)
            .unwrap();
        assert_eq!(concealed, empty.move_destinations(1, from, walking_cost, is_enemy).unwrap());
        assert_eq!(concealed.get(&enemy), Some(&1));
        assert_eq!(concealed.get(&corridor_hex(2)), Some(&2));
        assert!(!concealed.contains_key(&corridor_hex(1)));
        assert_eq!(
            guarded.move_destinations_in_view(1, enemy, walking_cost, is_enemy, |_| true, |_| false),
            Err(MoveError::MissingUnit(enemy)),
        );

        let revealed = guarded
            .move_destinations_in_view(1, from, walking_cost, is_enemy, |_| true, |hex| hex == enemy)
            .unwrap();
        assert_eq!(revealed, BTreeMap::from([(corridor_hex(2), 0)]));
    }

    #[test]
    fn shrouded_terrain_never_reaches_the_cost_query_or_opens_a_route() {
        let types = types();
        let from = corridor_hex(0);
        let unknown = corridor_hex(2);
        for terrain in ["Hh", "Rd"] {
            let mut board = corridor(&types, &[(0, 1)]);
            board.map = ScenarioMap::parse(&format!(
                "Xv,Xv,Xv\nXv,Gg,Xv\nXv,Gg,Xv\nXv,{terrain},Xv\nXv,Gg,Xv\nXv,Gg,Xv\nXv,Xv,Xv"
            )).unwrap();
            let moves = board.move_destinations_in_view(
                1, from,
                |code| {
                    assert_eq!(code, "Gg");
                    walking_cost(code)
                },
                |_| false, |hex| hex != unknown, |_| true,
            ).unwrap();
            assert_eq!(moves, BTreeMap::from([(corridor_hex(1), 4)]));
        }
    }

    #[test]
    fn walk_route_shows_each_step_and_uses_only_the_movers_view() {
        let unit_types = types();
        let board = corridor(&unit_types, &[(0, 1), (1, 1), (3, 2)]);
        let from = corridor_hex(0);
        let to = corridor_hex(4);
        let route = |shrouded: Option<Hex>, visible: bool| board.walk_route_in_view(
            1, from, to, walking_cost, |_| 60, |side| side == 2,
            |hex| Some(hex) != shrouded, |_| visible,
        );
        assert_eq!(route(None, false).unwrap(), (0..=4).map(corridor_hex).collect::<Vec<_>>());
        assert_eq!(route(Some(corridor_hex(2)), false), Err(MoveError::Unreachable(to)));
        assert_eq!(route(None, true), Err(MoveError::Unreachable(to)));
    }

    #[test]
    fn moves_through_an_ally_and_spends_the_route_cost() {
        let unit_types = types();
        let from = corridor_hex(0);
        let ally = corridor_hex(1);
        let to = corridor_hex(2);
        let mut board = corridor(&unit_types, &[(0, 1), (1, 2)]);

        board.move_unit(1, from, to, walking_cost, |_| false).unwrap();

        assert!(board.unit_at(from).is_none());
        assert_eq!(board.unit_at(ally).unwrap().id.as_deref(), Some("unit_1"));
        let moved = board.unit_at(to).unwrap();
        assert_eq!(moved.id.as_deref(), Some("unit_0"));
        assert_eq!(
            (moved.moves_left, moved.hitpoints, moved.attacks_left),
            (2, 36, 2)
        );
    }

    #[test]
    fn rejected_moves_leave_units_and_movement_in_place() {
        let unit_types = types();
        let from = corridor_hex(0);
        let ally = corridor_hex(1);
        let to = corridor_hex(3);
        let mut board = corridor(&unit_types, &[(0, 1), (1, 2)]);

        assert_eq!(
            board.move_unit(2, from, to, walking_cost, |_| false),
            Err(MoveError::WrongSide(from)),
        );
        assert_eq!(
            board.move_unit(1, from, ally, walking_cost, |_| false),
            Err(MoveError::Unreachable(ally)),
        );
        assert_eq!(
            board.move_unit(1, from, to, |_| NonZeroU32::new(2), |_| false),
            Err(MoveError::Unreachable(to)),
        );
        assert_eq!(board.unit_at(from).unwrap().moves_left, 5);
        assert_eq!(board.unit_at(ally).unwrap().side, 2);
        assert!(board.unit_at(to).is_none());
    }

    #[test]
    fn enemy_occupancy_blocks_entry_and_zone_of_control_ends_movement() {
        let unit_types = types();
        let from = corridor_hex(0);
        let controlled = corridor_hex(2);
        let enemy = corridor_hex(3);
        let mut board = corridor(&unit_types, &[(0, 1), (3, 2)]);

        assert_eq!(
            board.move_unit(1, from, enemy, walking_cost, |side| side == 2),
            Err(MoveError::Unreachable(enemy)),
        );
        assert_eq!(board.unit_at(from).unwrap().moves_left, 5);
        board
            .move_unit(1, from, controlled, walking_cost, |side| side == 2)
            .unwrap();

        assert_eq!(board.unit_at(controlled).unwrap().moves_left, 0);
        assert_eq!(board.unit_at(enemy).unwrap().side, 2);
        assert_eq!(
            board.move_unit(1, controlled, corridor_hex(1), walking_cost, |side| side == 2),
            Err(MoveError::Unreachable(corridor_hex(1))),
        );
    }

    #[test]
    fn skirmishers_pay_terrain_cost_through_controlled_hexes() {
        let scenario = wml::parse(
            "[side]\ntype=Fencer\n[/side]\n[side]\ntype=Spearman\n[/side]",
        ).unwrap();
        let map = ScenarioMap::parse(
            "Xv,Xv,Xv,Xv,Xv,Xv\nXv,1 Gg,Hh,Gg,Gg,Xv\nXv,Gg,2 Gg,Gg,Gg,Xv\nXv,Xv,Xv,Xv,Xv,Xv",
        ).unwrap();
        let mut types = types();
        let fencer = UnitType::from_wml(wml::parse(
            "id=Fencer\nmovement=6\n[abilities]\n[skirmisher]\nid=skirmisher\naffect_self=yes\n[/skirmisher]\n[/abilities]",
        ).unwrap());
        types.insert(fencer.id.clone(), fencer);
        let starts = read_starting_units(&scenario, &map).unwrap();
        let mut board = Battlefield::new(map, starts, &types).unwrap();
        let from = Hex { x: 0, y: 0 };
        let to = Hex { x: 3, y: 0 };
        let enemy = Hex { x: 1, y: 1 };
        let is_enemy = |side| side == 2;
        let moves = board.move_destinations(1, from, walking_cost, is_enemy).unwrap();
        assert!(!moves.contains_key(&enemy));
        assert_eq!(moves.get(&to), Some(&2));
        board.move_unit(1, from, to, walking_cost, is_enemy).unwrap();
        assert_eq!(board.unit_at(to).unwrap().moves_left, 2);
    }

    #[test]
    fn first_round_keeps_authored_actions_and_later_rounds_refresh_only_the_incoming_side() {
        let scenario = wml::parse(
            "[side]\ntype=Spearman\n[/side]\n[side]\ntype=Spearman\n[/side]",
        )
        .unwrap();
        let map = map();
        let starts = read_starting_units(&scenario, &map).unwrap();
        let unit_types = types();
        let mut board = Battlefield::new(map, starts, &unit_types).unwrap();
        for unit in board.units.values_mut() {
            unit.hitpoints = 17;
            unit.moves_left = 0;
            unit.attacks_left = 0;
        }

        let first = Turn { round: 1, side: 1 };
        let second = Turn { round: 1, side: 2 };
        assert_eq!(board.advance_turn(first, &[1, 2], Some(2)), Some(second));
        let incoming = board.unit_at(WEST).unwrap();
        assert_eq!(
            (incoming.moves_left, incoming.attacks_left, incoming.hitpoints),
            (0, 0, 17)
        );
        assert_eq!(board.unit_at(EAST).unwrap().moves_left, 0);
        assert_eq!(board.advance_turn(second, &[1, 2], Some(2)), Some(Turn { round: 2, side: 1 }));
        let refreshed = board.unit_at(EAST).unwrap();
        assert_eq!((refreshed.moves_left, refreshed.attacks_left, refreshed.hitpoints), (5, 2, 17));
        assert_eq!(board.unit_at(WEST).unwrap().attacks_left, 0);
    }
}
