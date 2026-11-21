use std::{
    collections::{BTreeMap, BTreeSet},
    error, fmt,
    num::NonZeroU32,
};

use crate::{
    board::{
        Attack, AttackChoice, AttackError, BoardError, MoveError, StrikeModifiers, UnitState,
        StrikeRole, combatant, within_range,
    },
    hex::Hex,
    income::{DEFAULT_BASE_INCOME, net_income},
    movement_cost::MovementCosts,
    movement_types::MovementType,
    observation::{Observation, project},
    resistance::DamageTable,
    rng::ReplayRng,
    scenario_events::{BoardChange, EventState, ScenarioEvents},
    scenario_start::ScenarioStart,
    terrain_aliases::TerrainAliases,
    turn::Turn,
    unit_type::UnitType,
    visibility::VisibilityMask,
    weapon::WeaponProfile,
};

pub mod audit;
pub mod dialogue;
pub mod carryover;
#[cfg(test)]
mod amla_tests;
mod illumination;
mod teleport;
pub mod walk;
mod vision;
mod recruit;
mod plague;
mod kill;
mod retaliation;
pub mod attack_preview;
pub mod combat_display;
pub mod move_preview;
pub mod unit_report;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Action {
    Move { from: Hex, to: Hex },
    Attack(AttackChoice),
    Recruit { unit_type: String, to: Hex },
    Recall { unit_id: String, to: Hex },
    Disband { unit_id: String },
    EndTurn,
}

/// Information the active side can inspect when choosing one of its reserves.
pub struct RecallInfo<'a> {
    pub type_id: &'a str,
    pub hitpoints: i32,
    pub experience: i32,
    pub cost: i32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MatchState {
    Playing(Turn),
    Won(usize),
    /// Surviving sides, in ascending order, when none are enemies of one another.
    AlliedVictory(Vec<usize>),
    /// Local campaign outcome declared by WML instead of the surviving coalition.
    CampaignVictory,
    CampaignDefeat,
    Draw,
}

pub struct Rules<'data> {
    pub terrain: &'data TerrainAliases,
    pub movement: &'data BTreeMap<String, MovementType>,
    pub resistance: &'data BTreeMap<String, DamageTable>,
}

impl Rules<'_> {
    fn walking_cost(&self, unit_type: &UnitType, terrain: &str) -> Option<NonZeroU32> {
        let inherited = self
            .movement
            .get(&unit_type.movement_type)
            .map(|profile| &profile.movement_costs);
        unit_type
            .movement_cost_overrides
            .cost_with_fallback(self.terrain, terrain, false, inherited)
    }

    /// Cost of seeing through a hex, including slow. Unreachable terrain costs 99.
    pub fn sight_cost(&self, unit: &UnitState<'_>, terrain: &str) -> NonZeroU32 {
        let unit_type = unit.unit_type;
        let profile = self.movement.get(&unit_type.movement_type);
        MovementCosts::layered_cost(
            self.terrain,
            terrain,
            unit.slowed,
            &[
                Some(&unit_type.vision_cost_overrides),
                profile.map(|p| &p.vision_costs),
                Some(&unit_type.movement_cost_overrides),
                profile.map(|p| &p.movement_costs),
            ],
        )
    }
}

/// A headless match on a scenario map with each side's own map knowledge.
///
/// It ends when no surviving leaders are enemies or the scenario reaches the round limit.
pub struct FixedMatch<'data> {
    scenario: ScenarioStart<'data>,
    rules: Rules<'data>,
    rng: ReplayRng,
    state: MatchState,
    show_end_report: bool,
    replay_save: bool,
    current_round: i32,
    kill_stack: Vec<Hex>,
    village_owners: BTreeMap<Hex, usize>,
    vision: Vec<VisibilityMask>,
    side_order: Vec<usize>,
    dialogue: Vec<(Vec<usize>, dialogue::DialogueLine)>,
    gold: Vec<i64>,
}

impl<'data> FixedMatch<'data> {
    pub fn new(
        scenario: ScenarioStart<'data>,
        rules: Rules<'data>,
        seed: u32,
    ) -> Result<Self, MatchError> {
        let side_count = scenario.sides.len();
        if side_count < 2
            || scenario.sides.iter().enumerate().any(|(index, side)| side.side != index + 1)
            || scenario
                .board
                .units()
                .any(|(_, unit)| {
                    !(1..=side_count).contains(&unit.side) || !supports_unit(unit.unit_type)
                })
        {
            return Err(MatchError::UnsupportedScenario);
        }
        let gold = scenario.sides.iter().map(|side| i64::from(side.starting_gold)).collect();
        let mut village_owners = BTreeMap::new();
        let map = scenario.board.map();
        for side in &scenario.sides {
            for &hex in &side.starting_villages {
                if map.is_playable(hex)
                    && map.terrain_at(hex).is_some_and(|code| rules.terrain.features(code).village)
                {
                    village_owners.entry(hex).or_insert(side.side);
                }
            }
        }
        let mut game = Self {
            show_end_report: true,
            replay_save: true,
            current_round: scenario.start_round,
            kill_stack: Vec::new(),
            vision: scenario.sides.iter().map(|side| VisibilityMask::from_side(map, side)).collect(),
            state: MatchState::Playing(Turn {
                round: scenario.start_round,
                side: 1,
            }),
            scenario,
            rules,
            rng: ReplayRng::new(seed),
            village_owners,
            dialogue: Vec::new(),
            side_order: (1..=side_count).collect(),
            gold,
        };
        for side in 1..=side_count {
            game.update_side_sight(side, false);
        }
        game.with_events(|events, state| {
            events.fire("preload", state);
            events.fire("prestart", state);
            events.fire("start", state);
        })?;
        game.check_victory();
        if let MatchState::Playing(turn) = game.state {
            if game.scenario.last_round.is_some_and(|limit| turn.round > limit) {
                game.state = MatchState::Draw;
            } else {
                game.with_events(|events, state| events.begin_turn(turn, events.new_round_at_start, state))?;
                if !matches!(game.state, MatchState::Playing(_)) { return Ok(game); }
                game.settle_income(turn);
                game.scenario.board.refresh_units(turn);
                game.scenario.board.begin_resting(turn.side);
                game.with_events(|events, state| events.refresh(turn, state))?;
                game.refog(turn.side);
            }
        }
        Ok(game)
    }

    fn with_events(&mut self, handle: impl FnOnce(&mut ScenarioEvents, &mut EventState<'_>)) -> Result<(), MatchError> {
        self.with_events_at(None, handle)
    }

    fn with_events_at(&mut self, primary: Option<Hex>, handle: impl FnOnce(&mut ScenarioEvents, &mut EventState<'_>)) -> Result<(), MatchError> {
        if !matches!(self.state, MatchState::Playing(_)) { return Ok(()); }
        let mut state = EventState { gold: &mut self.gold, sides: &mut self.scenario.sides, board_changes: Vec::new() };
        handle(&mut self.scenario.events, &mut state);
        let mut killed = false;
        let mut pending = std::collections::VecDeque::from(state.board_changes);
        while let Some(change) = pending.pop_front() {
            let capture = match change {
                BoardChange::HealUnit(heal) => {
                    heal.apply(&mut self.scenario.board, primary);
                    continue;
                }
                BoardChange::Dialogue(message) => {
                    self.queue_dialogue(message);
                    continue;
                }
                BoardChange::Kill(kill, primary) => {
                    killed |= self.kill_units(kill, primary)?;
                    continue;
                }
                BoardChange::Resume(action, position) => {
                    let mut state = EventState { gold: &mut self.gold, sides: &mut self.scenario.sides, board_changes: Vec::new() };
                    self.scenario.events.resume(&action, position, &self.scenario.board, &mut state);
                    for next in state.board_changes.into_iter().rev() { pending.push_front(next); }
                    continue;
                }
                BoardChange::TurnLimit(change) => {
                    self.scenario.last_round = change.apply(self.scenario.last_round);
                    continue;
                }
                BoardChange::EndLevel { victory, report, replay_save, next_scenario, gold } => {
                    if matches!(self.state, MatchState::Playing(_)) {
                        self.show_end_report = report;
                        self.replay_save = replay_save;
                        for (treasury, settings) in self.scenario.treasuries.iter_mut().zip(gold) {
                            treasury.override_with(settings);
                        }
                        if next_scenario.is_some() { self.scenario.next_scenario = next_scenario; }
                        self.state = if victory { MatchState::CampaignVictory } else { MatchState::CampaignDefeat };
                    }
                    continue;
                }
                BoardChange::PersonalRecruits(change) => {
                    self.scenario.board.change_personal_recruits(&change);
                    continue;
                }
                BoardChange::CaptureVillage(capture) => capture,
                BoardChange::TimeArea(area) => {
                    area.apply(&mut self.scenario.schedule, self.current_round);
                    continue;
                }
                BoardChange::Schedule { entries, current_time } => {
                    self.scenario.schedule.replace_global(entries, self.current_round, current_time);
                    continue;
                }
                BoardChange::PlaceUnit(unit) => {
                    let side = unit.side;
                    let recalled = unit.id.as_ref().and_then(|id|
                        self.scenario.board.recall_roster(side).iter().find(|u| u.id.as_ref() == Some(id))
                    );
                    let kind = recalled.map(|veteran| veteran.unit_type)
                        .or_else(|| self.scenario.board.unit_type(&unit.type_id).map(|kind| kind.variation(&unit.variation)));
                    if kind.is_some_and(|kind| !supports_unit(kind)) {
                        return Err(MatchError::UnsupportedScenario);
                    }
                    if let Some(position) = self.scenario.board.place_event_unit(*unit).map_err(MatchError::OpeningUnit)? {
                        self.capture_village(side, position)?;
                        self.update_side_sight(side, false);
                    }
                    continue;
                }
                BoardChange::Terrain { locations, code, layer, fallback } => {
                    let map = self.scenario.board.map();
                    let (width, height) = (map.width(), map.height());
                    for (xs, ys) in locations {
                        for x in (*xs.start()).max(1)..=(*xs.end()).min(width) {
                            for y in (*ys.start()).max(1)..=(*ys.end()).min(height) {
                                let position = Hex { x: x - 1, y: y - 1 };
                                let old = self.scenario.board.map().terrain_at(position).expect("clipped map location");
                                let Some(merged) = self.rules.terrain.merge_terrain(old, &code, layer, fallback) else { continue };
                                let village = self.rules.terrain.features(&merged).village;
                                self.scenario.board.replace_terrain(position, &merged);
                                if !village { self.village_owners.remove(&position); }
                            }
                        }
                    }
                    continue;
                }
            };
            let owner = usize::try_from(capture.side).ok().filter(|s| (1..=self.side_order.len()).contains(s));
            if owner.is_some_and(|side| !self.side_survives(side)) {
                continue;
            }
            let map = self.scenario.board.map();
            capture.for_each_hex(map, |hex| {
                if !map.terrain_at(hex).is_some_and(|code| self.rules.terrain.features(code).village) { return; }
                if let Some(side) = owner {
                    self.village_owners.insert(hex, side);
                } else {
                    self.village_owners.remove(&hex);
                }
            });
        }
        if killed && self.kill_stack.is_empty() { self.check_victory(); }
        Ok(())
    }

    pub fn show_end_report(&self) -> bool {
        self.show_end_report && !matches!(self.state, MatchState::Playing(_))
    }

    pub fn state(&self) -> MatchState {
        self.state.clone()
    }

    /// Public scenario time for the active round, before terrain or unit illumination.
    pub fn global_time_of_day(&self) -> Option<&crate::time_of_day::TimeOfDay> {
        let MatchState::Playing(turn) = self.state else { return None };
        Some(self.scenario.schedule.at_turn(turn.round))
    }

    pub fn recall_info(&self, id: &str) -> Option<RecallInfo<'_>> {
        let MatchState::Playing(turn) = self.state else { return None };
        let unit = self.scenario.board.recall_roster(turn.side)
            .iter().find(|unit| unit.id.as_deref() == Some(id))?;
        Some(RecallInfo {
            type_id: &unit.unit_type.id,
            hitpoints: unit.hitpoints,
            experience: unit.experience,
            cost: unit.recall_cost(self.scenario.sides[turn.side - 1].recall_cost),
        })
    }

    /// Current WML objectives for the chosen side of the desktop match.
    pub fn objectives_for(&self, side: usize) -> Option<&crate::scenario_sides::ScenarioObjectives> {
        self.scenario.sides.get(side.checked_sub(1)?).map(|settings| &settings.objectives)
            .filter(|objectives| !objectives.is_empty())
    }

    /// Campaign destination after a scripted victory or a surviving human side's victory.
    pub fn next_scenario_after_victory(&self) -> Option<&str> {
        self.scenario.next_scenario.as_deref().filter(|id| self.human_won() && !id.is_empty() && *id != "null")
    }

    pub fn save_victory_replay(&self) -> bool { self.replay_save && self.human_won() }

    fn human_won(&self) -> bool {
        let human_won = |side: usize| self.scenario.sides[side - 1].human;
        match &self.state {
            MatchState::CampaignVictory => true,
            MatchState::Won(side) => human_won(*side),
            MatchState::AlliedVictory(winners) => winners.iter().copied().any(human_won),
            _ => false,
        }
    }

    /// Original map item images visible to one side, in hex overlay draw order.
    pub fn map_item_images(&self, side: usize) -> Vec<(Hex, &str)> {
        let Some(settings) = side.checked_sub(1).and_then(|i| self.scenario.sides.get(i)) else {
            return Vec::new();
        };
        let items = &self.scenario.events.items;
        if items.is_empty() { return Vec::new(); }
        items.visible_images(&self.effective_vision(side), &settings.team_names)
    }

    /// Projects the current board for a match side, including after the match ends.
    pub fn observation_for(&self, side: usize) -> Option<Observation> {
        let gold = *self.gold.get(side.checked_sub(1)?)?;
        let board = &self.scenario.board;
        let vision = self.effective_vision(side);
        let mut observation = project(board, side, &vision, |hex| self.unit_visible_on_known_hex(side, hex));
        observation.allied_sides = self.scenario.sides.iter()
            .filter(|other| !self.scenario.sides[side - 1].is_enemy(other))
            .map(|other| other.side)
            .collect();
        observation.gold = Some(gold);
        for tile in &mut observation.tiles {
            tile.time_color = self.scenario.schedule.at_hex(self.current_round, tile.position).color;
            tile.village_owner = self.village_owners.get(&tile.position).copied().filter(|owner|
                tile.visible || observation.allied_sides.contains(owner)
            );
        }
        Some(observation)
    }

    /// The active side's observation and actions, without referee state.
    pub fn decision(&self) -> Option<(Observation, Vec<Action>)> {
        let MatchState::Playing(turn) = self.state else {
            return None;
        };
        let observation = self.observation_for(turn.side).expect("active match side");
        let board = &self.scenario.board;
        let sides = &self.scenario.sides;
        let is_enemy = |other: usize| sides[turn.side - 1].is_enemy(&sides[other - 1]);
        let terrain: BTreeSet<_> = observation.tiles.iter().map(|tile| tile.position).collect();
        let units: BTreeSet<_> = observation.units.iter().map(|unit| unit.position).collect();
        let terrain_known = |hex| terrain.contains(&hex);
        let unit_visible = |hex| units.contains(&hex);
        let mut actions = Vec::new();
        for (from, unit) in board.units().filter(|(_, unit)| unit.side == turn.side) {
            let villages = self.teleport_villages(from, unit, terrain_known, unit_visible);
            let destinations = board
                .move_destinations_with_teleports(
                    turn.side,
                    from,
                    |terrain| self.rules.walking_cost(unit.unit_type, terrain),
                    is_enemy,
                    terrain_known,
                    unit_visible,
                    &villages,
                )
                .expect("owned move origin");
            actions.extend(destinations.into_keys().map(|to| Action::Move { from, to }));
        }
        actions.extend(
            board
                .attack_choices(turn.side, is_enemy)
                .into_iter()
                .filter(|choice| unit_visible(choice.to))
                .map(Action::Attack),
        );
        self.extend_recruit_choices_in_view(turn.side, &mut actions, terrain_known, unit_visible);
        actions.extend(board.recall_roster(turn.side).iter().filter_map(|unit| {
            unit.id.as_ref().map(|id| Action::Disband { unit_id: id.clone() })
        }));
        actions.push(Action::EndTurn);
        Some((observation, actions))
    }

    pub fn step(&mut self, action: Action) -> Result<MatchState, MatchError> {
        self.step_with_choices(action, None, None, None, None)
    }

    /// Supported next types of an active-side unit, in WML declaration order.
    pub fn advancement_choices(&self, position: Hex) -> Vec<String> {
        let MatchState::Playing(turn) = self.state else { return Vec::new() };
        let board = &self.scenario.board;
        let Some(unit) = board.unit_at(position).filter(|unit| unit.side == turn.side) else {
            return Vec::new();
        };
        unit.unit_type.advances_to.iter()
            .filter(|id| board.can_advance_to(position, id, supports_unit))
            .cloned()
            .collect()
    }

    /// Apply an attack with each combatant's preferred next type if it advances.
    /// Omitted choices and subsequent levels use the first declared type.
    /// Invalid preferences are rejected before combat or RNG consumption.
    pub fn attack_with_advancements(
        &mut self, attack: AttackChoice, attacker: Option<&str>, defender: Option<&str>,
    ) -> Result<MatchState, MatchError> {
        self.step_with_choices(Action::Attack(attack), attacker, defender, None, None)
    }

    /// Execute this side's movement once, recording the reached route for presentation.
    pub fn move_with_trace(&mut self, from: Hex, to: Hex, trace: &mut Vec<Hex>) -> Result<MatchState, MatchError> {
        trace.clear();
        self.step_with_choices(Action::Move { from, to }, None, None, None, Some(walk::Trace::Own(trace)))
    }

    fn step_with_choices(
        &mut self, action: Action, attacker: Option<&str>, defender: Option<&str>,
        display: Option<(usize, &mut Option<combat_display::VisibleCombat>)>,
        movement_trace: Option<walk::Trace<'_>>,
    ) -> Result<MatchState, MatchError> {
        let MatchState::Playing(turn) = self.state else {
            return Err(MatchError::Finished);
        };
        self.dialogue.clear();
        let sides = &self.scenario.sides;
        let viewer = &sides[turn.side - 1];
        if (viewer.fog || viewer.shroud || attacker.is_some() || defender.is_some()) && !matches!(action, Action::EndTurn)
            && !self.decision().expect("playing side").1.contains(&action)
        {
            return Err(MatchError::ActionUnavailable);
        }
        match action {
            Action::Move { from, to } => self.walk(turn.side, from, to, movement_trace)?,
            Action::Attack(choice) => {
                if self.scenario.board.unit_at(choice.to).is_some()
                    && !self.unit_visible_on_known_hex(turn.side, choice.to)
                {
                    return Err(MatchError::ActionUnavailable);
                }
                let attack = self.prepare_attack(turn, choice)?;
                let board = &mut self.scenario.board;
                for (position, preferred) in [(choice.from, attacker), (choice.to, defender)] {
                    if preferred.is_some_and(|id| !board.can_advance_to(position, id, supports_unit)) {
                        return Err(MatchError::InvalidAdvancement);
                    }
                }
                let defender_side = board.unit_at(choice.to).ok_or(AttackError::MissingDefender)?.side;
                if self.apply_scenario_attack(turn, attack, display)? {
                    let board = &mut self.scenario.board;
                    board.advance_after_combat(choice.from, attacker, supports_unit);
                    board.advance_after_combat(choice.to, defender, supports_unit);
                }
                self.refog(defender_side);
                self.reveal_travelled_hexes(choice.from, [choice.from]);
                self.check_victory();
            }
            Action::Recruit { unit_type, to } => {
                self.recruit(turn.side, &unit_type, to)?;
                self.reveal_travelled_hexes(to, [to]);
                self.fire_unit_event("recruit", to)?;
            }
            Action::Recall { unit_id, to } => {
                self.recall(turn.side, &unit_id, to)?;
                self.reveal_travelled_hexes(to, [to]);
                self.fire_unit_event("recall", to)?;
            }
            Action::Disband { unit_id } => {
                self.scenario.board.disband(turn.side, &unit_id).ok_or(MatchError::InvalidDisband)?;
            }
            Action::EndTurn => {
                let surviving_sides: BTreeSet<_> = self.surviving_sides().collect();
                self.village_owners.retain(|_, side| surviving_sides.contains(side));
                let candidate = turn.next(&self.side_order, None);
                let end_round = candidate.is_none_or(|next| next.round != turn.round);
                self.with_events(|events, state| events.end_turn(turn, end_round, state))?;
                if !matches!(self.state, MatchState::Playing(_)) { return Ok(self.state()); }
                if end_round && candidate.is_some_and(|next| self.scenario.last_round.is_some_and(|limit| next.round > limit)) {
                    self.current_round = candidate.expect("expired next round").round;
                    self.with_events(|events, state| events.fire("time_over", state))?;
                    if !matches!(self.state, MatchState::Playing(_)) { return Ok(self.state()); }
                }
                let limit = if end_round { self.scenario.last_round } else { None };
                let next = self.scenario.board.advance_turn(turn, &self.side_order, limit);
                self.refog(turn.side);
                if let Some(next) = next {
                    self.state = MatchState::Playing(next);
                    self.current_round = next.round;
                    self.with_events(|events, state| events.begin_turn(next, end_round, state))?;
                    if !matches!(self.state, MatchState::Playing(_)) { return Ok(self.state()); }
                    self.settle_income(next);
                    let sides = &self.scenario.sides;
                    self.scenario.board.heal_at_turn_start(next.side, self.rules.terrain, |a, b| {
                        sides[a - 1].is_enemy(&sides[b - 1])
                    });
                    self.with_events(|events, state| events.refresh(next, state))?;
                    self.refog(next.side);
                } else {
                    self.state = MatchState::Draw;
                }
            }
        }
        Ok(self.state())
    }

    fn fire_unit_event(&mut self, name: &str, position: Hex) -> Result<(), MatchError> {
        self.fire_units_event(name, position, None)
    }

    fn fire_units_event(&mut self, name: &str, position: Hex, second: Option<Hex>) -> Result<(), MatchError> {
        let context = |position| self.scenario.board.unit_at(position).map(|unit| crate::scenario_events::ArrivingUnit {
            position, side: unit.side, id: unit.id.clone(), type_id: unit.unit_type.id.as_str(),
        });
        let primary = context(position);
        let secondary = second.and_then(context);
        self.with_events_at(Some(position), |events, state| events.dispatch(name, primary, secondary, state))
    }

    fn capture_village(&mut self, side: usize, to: Hex) -> Result<bool, MatchError> {
        if self.village_owners.get(&to).copied() == Some(side)
            || !self
                .scenario
                .board
                .map()
                .terrain_at(to)
                .is_some_and(|code| self.rules.terrain.features(code).village)
        {
            return Ok(false);
        }
        if self.side_survives(side) {
            self.village_owners.insert(to, side);
            self.fire_unit_event("capture", to)?;
        } else if self.village_owners.get(&to).is_some_and(|owner|
            self.scenario.sides[side - 1].is_enemy(&self.scenario.sides[*owner - 1])
        ) {
            self.village_owners.remove(&to);
        }
        Ok(true)
    }

    fn side_costs(&self, side: usize) -> (usize, i32) {
        let villages = self
            .village_owners
            .values()
            .filter(|&&owner| owner == side)
            .count();
        let upkeep = self
            .scenario
            .board
            .units()
            .filter(|(_, unit)| unit.side == side)
            .map(|(_, unit)| unit.upkeep())
            .sum();
        (villages, upkeep)
    }

    pub fn economy_for(&self, view: &crate::observation::Observation) -> Option<crate::income::EconomyReport> {
        let side = self.scenario.sides.get(view.side.checked_sub(1)?)?;
        let (villages, upkeep) = self.side_costs(view.side);
        let known_villages = view.tiles.iter().filter(|tile| self.rules.terrain.features(&tile.terrain).village).count();
        Some(crate::income::EconomyReport {
            villages, known_villages, upkeep,
            expenses: crate::income::expenses(side, villages, upkeep),
            income: net_income(side, DEFAULT_BASE_INCOME, villages, upkeep),
            last_round: self.scenario.last_round,
        })
    }

    fn settle_income(&mut self, turn: Turn) {
        if turn.round <= 1 {
            return;
        }

        let (villages, upkeep) = self.side_costs(turn.side);
        let index = turn.side - 1;
        self.gold[index] += net_income(&self.scenario.sides[index], DEFAULT_BASE_INCOME, villages, upkeep);
    }

    fn prepare_attack(&self, turn: Turn, choice: AttackChoice) -> Result<Attack, AttackError> {
        self.prepare_attack_in_view(turn, choice, |_| true).map(|(attack, _)| attack)
    }

    fn prepare_attack_in_view(
        &self, turn: Turn, choice: AttackChoice, visible: impl Fn(Hex) -> bool + Copy,
    ) -> Result<(Attack, bool), AttackError> {
        let board = &self.scenario.board;
        let attacker = board.unit_at(choice.from).ok_or(AttackError::MissingAttacker)?;
        let defender = board.unit_at(choice.to).ok_or(AttackError::MissingDefender)?;
        let weapon = attacker
            .unit_type
            .attacks
            .get(choice.weapon)
            .ok_or(AttackError::UnavailableWeapon)?;
        let distance = choice.from.distance(choice.to) as i64;
        let attack_leadership = self.leadership_in_view(choice.from, attacker, visible);
        let defense_leadership = self.leadership_in_view(choice.to, defender, visible);
        let attack_light = self.lawful_bonus_in_view(turn.round, choice.from, visible);
        let defense_light = self.lawful_bonus_in_view(turn.round, choice.to, visible);
        let modifiers = self.modifiers(attack_light, attacker, defender, weapon, choice.to, attack_leadership, true);
        let backstab = board.backstabs(choice.from, choice.to, weapon, |at, side| {
            visible(at) && self.scenario.sides[defender.side - 1].is_enemy(&self.scenario.sides[side - 1])
        });
        let offensive_role = StrikeRole::Attack { backstab };
        let candidates = defender
            .unit_type
            .attacks
            .iter()
            .enumerate()
            .filter(|(_, response)| {
                response.can_retaliate
                    && response.range == weapon.range
                    && within_range(response, distance)
            })
            .map(|(index, response)| {
                let response_modifiers = self.modifiers(
                    defense_light, defender, attacker, response, choice.from, defense_leadership, false,
                );
                let attacker_stats = combatant(
                    attacker, choice.weapon, &modifiers, response.parry, offensive_role,
                );
                let defender_stats = combatant(
                    defender, index, &response_modifiers, weapon.parry, StrikeRole::Retaliate(weapon),
                );
                let rounds = weapon.combat_rounds().max(response.combat_rounds());
                ((index, response_modifiers), attacker_stats, defender_stats, rounds)
            });
        Ok((Attack {
            from: choice.from,
            to: choice.to,
            weapon: choice.weapon,
            retaliation: retaliation::select(candidates),
            modifiers,
        }, backstab))
    }

    fn leadership_bonus(&self, position: Hex, recipient: &UnitState<'_>) -> i64 {
        self.leadership_in_view(position, recipient, |_| true)
    }

    fn leadership_in_view(&self, position: Hex, recipient: &UnitState<'_>, visible: impl Fn(Hex) -> bool) -> i64 {
        position.neighbors().filter(|hex| visible(*hex))
            .filter_map(|hex| self.scenario.board.unit_at(hex))
            .filter(|unit| unit.side == recipient.side && unit.unit_type.leadership)
            .map(|unit| i64::from(unit.unit_type.level) - i64::from(recipient.unit_type.level))
            .max().unwrap_or(0).max(0) * 25
    }

    fn modifiers(
        &self,
        lawful_bonus: i32,
        striker: &UnitState<'_>,
        target: &UnitState<'_>,
        weapon: &WeaponProfile,
        target_hex: Hex,
        leadership: i64,
        target_defends: bool,
    ) -> StrikeModifiers {
        let terrain = self.scenario.board.map().terrain_at(target_hex).expect("unit on map");
        let target_movement = &target.unit_type.movement_type;
        let inherited_defense = self.rules.movement.get(target_movement).map(|profile| &profile.defense);
        let target_hit_chance = target.unit_type.defense_overrides.hit_chance_with_fallback(
            self.rules.terrain,
            terrain,
            inherited_defense,
        );
        let inherited_resistance = self.rules.resistance.get(target_movement);
        let damage_taken_for = |damage_type: &str|
            target.unit_type.combat_damage_taken(damage_type, inherited_resistance, target_defends);
        let base_damage_taken = damage_taken_for(&weapon.damage_type);
        let damage_taken = if weapon.has_arcane_alternative() {
            base_damage_taken.max(damage_taken_for("arcane"))
        } else {
            base_damage_taken
        };
        let schedule = &self.scenario.schedule;
        let alignment =
            schedule.combat_modifier(lawful_bonus, striker.unit_type.alignment, striker.is_fearless());
        let multiplier = (100 + alignment + leadership).saturating_mul(i64::from(damage_taken));
        StrikeModifiers {
            target_hit_chance,
            damage_multiplier: multiplier.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32,
        }
    }

    fn side_survives(&self, side: usize) -> bool {
        use crate::scenario_sides::DefeatCondition;
        match self.scenario.sides[side - 1].defeat_condition {
            DefeatCondition::Never => true,
            DefeatCondition::Always => false,
            DefeatCondition::NoUnitsLeft => self.scenario.board.units().any(|(_, unit)| unit.side == side),
            DefeatCondition::NoLeaderLeft => self.scenario.board.units().any(|(_, unit)| unit.side == side && unit.can_recruit),
        }
    }

    fn surviving_sides(&self) -> impl Iterator<Item = usize> + '_ {
        self.side_order.iter().copied().filter(|&side| self.side_survives(side))
    }

    fn check_victory(&mut self) {
        if !matches!(self.state, MatchState::Playing(_)) { return; }
        let living: BTreeSet<_> = self.surviving_sides().collect();
        let sides = &self.scenario.sides;
        let mut winners: Vec<usize> = Vec::with_capacity(living.len());
        for side in living {
            if winners.iter().any(|&other| sides[side - 1].is_enemy(&sides[other - 1])) {
                return;
            }
            winners.push(side);
        }
        if winners.iter().any(|side| self.scenario.manual_victory_sides.contains(side)) {
            return;
        }
        self.state = match winners.as_slice() {
            [] => MatchState::Draw,
            [side] => MatchState::Won(*side),
            _ => MatchState::AlliedVictory(winners),
        };
    }
}

fn supports_unit(unit_type: &UnitType) -> bool {
    unit_type.attacks.iter().all(WeaponProfile::supports_specials)
}

#[derive(Debug, Eq, PartialEq)]
pub enum MatchError {
    UnsupportedScenario,
    OpeningUnit(BoardError),
    ActionUnavailable,
    Finished,
    Move(MoveError),
    Attack(AttackError),
    InvalidRecruit,
    InvalidRecall,
    InvalidDisband,
    InvalidAdvancement,
}

impl From<MoveError> for MatchError {
    fn from(error: MoveError) -> Self {
        Self::Move(error)
    }
}

impl From<AttackError> for MatchError {
    fn from(error: AttackError) -> Self {
        Self::Attack(error)
    }
}

impl fmt::Display for MatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedScenario => f.write_str(
                "fixed match requires at least two sides in order and supported weapons",
            ),
            Self::OpeningUnit(error) => write!(f, "could not place a scenario event unit: {error}"),
            Self::ActionUnavailable => f.write_str("action is unavailable to the current side"),
            Self::Finished => f.write_str("match is over"),
            Self::Move(error) => error.fmt(f),
            Self::Attack(error) => error.fmt(f),
            Self::InvalidRecruit => {
                f.write_str("unit type or destination is unavailable for recruitment")
            }
            Self::InvalidRecall => f.write_str("unit or destination is unavailable for recall"),
            Self::InvalidDisband => f.write_str("unit is absent from the active side's recall list"),
            Self::InvalidAdvancement => f.write_str("unit type is unavailable for advancement"),
        }
    }
}

impl error::Error for MatchError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        movement_types::read_movement_types,
        scenario_start,
        unit_type::{UnitType, read_unit_types},
        wml,
    };

    #[test]
    fn advances_combat_survivors_using_their_chosen_or_first_declared_type() {
        let scenario = wml::parse(
            r#"
            experience_modifier=50
            map_data="Xv,Xv,Xv,Xv
Xv,1 Gg,2 Gg,Xv
Xv,Xv,Xv,Xv"
            [side]
                side=1
                id=West
                type=Fighter
                experience=29
                attacks_left=1
            [/side]
            [side]
                side=2
                id=East
                type=Fighter
                experience=9
                moves=1
            [/side]
            "#,
        )
        .unwrap();
        let roster = read_unit_types(
            wml::parse(
                r#"
                [unit_type]
                    id=Fighter
                    hitpoints=10
                    movement=4
                    attacks=4
                    level=1
                    experience=20
                    advances_to=Veteran,Champion,Unsupported
                    [attack]
                        name=sword
                        type=blade
                        range=melee
                        damage=5
                        number=1
                        movement_used=1
                    [/attack]
                [/unit_type]
                [unit_type]
                    id=Veteran
                    hitpoints=18
                    movement=2
                    attacks=2
                    level=2
                    experience=36
                    advances_to=Champion
                [/unit_type]
                [unit_type]
                    id=Champion
                    hitpoints=25
                    movement=3
                    attacks=3
                    level=3
                    experience=50
                    advances_to=null
                [/unit_type]
                [unit_type]
                    id=Unsupported
                    [attack]
                        [specials]
                            [dummy]
                                id=custom
                            [/dummy]
                        [/specials]
                    [/attack]
                [/unit_type]
                "#,
            )
            .unwrap(),
        );
        let terrain = TerrainAliases::from_wml(&wml::Node::default());
        let movement = BTreeMap::new();
        let resistance = BTreeMap::new();
        let start_game = || FixedMatch::new(
            scenario_start::load(&scenario, &roster, |_| unreachable!()).unwrap(),
            Rules { terrain: &terrain, movement: &movement, resistance: &resistance },
            5489,
        ).unwrap();
        let mut game = start_game();

        let from = Hex { x: 0, y: 0 };
        let to = Hex { x: 1, y: 0 };
        let attack = AttackChoice { from, to, weapon: 0 };
        assert_eq!(
            game.step(Action::Attack(attack)),
            Ok(MatchState::Playing(Turn { round: 1, side: 1 }))
        );

        let attacker = game.scenario.board.unit_at(from).unwrap();
        assert_eq!(attacker.unit_type.id, "Champion");
        assert_eq!(attacker.max_experience(), 25);
        assert_eq!((attacker.hitpoints, attacker.experience), (25, 2));
        assert_eq!((attacker.moves_left, attacker.attacks_left), (2, 0));
        assert_eq!(
            (attacker.id.as_deref(), attacker.side, attacker.can_recruit),
            (Some("West"), 1, true)
        );

        let defender = game.scenario.board.unit_at(to).unwrap();
        assert_eq!(defender.unit_type.id, "Veteran");
        assert_eq!((defender.hitpoints, defender.experience), (18, 0));
        assert_eq!((defender.moves_left, defender.attacks_left), (1, 2));
        assert_eq!(
            (defender.id.as_deref(), defender.side, defender.can_recruit),
            (Some("East"), 2, true)
        );

        let mut game = start_game();
        let before = game.audit_snapshot();
        let rng = game.rng.seed_and_calls();
        for (attacker, defender) in [(Some("Fighter"), None), (None, Some("Unsupported"))] {
            assert_eq!(game.attack_with_advancements(attack, attacker, defender), Err(MatchError::InvalidAdvancement));
            assert_eq!(game.audit_snapshot(), before);
            assert_eq!(game.rng.seed_and_calls(), rng);
        }
        game.attack_with_advancements(attack, Some("Champion"), Some("Champion")).unwrap();
        let attacker = game.scenario.board.unit_at(from).unwrap();
        let defender = game.scenario.board.unit_at(to).unwrap();
        assert_eq!((attacker.unit_type.id.as_str(), attacker.hitpoints, attacker.experience), ("Champion", 25, 20));
        assert_eq!((defender.unit_type.id.as_str(), defender.hitpoints, defender.experience), ("Champion", 25, 0));
    }

    #[test]
    fn plays_a_seeded_fixed_scenario_to_victory_or_the_round_limit() {
        let scenario = wml::parse(
            r#"
            turns=2
            map_data="Xv,Xv,Xv,Xv,Xv,Xv
Xv,1 Gg,Vh,Gg,2 Gg,Xv
Xv,Xv,Xv,Xv,Xv,Xv"
            [side]
                side=1
                type=Fighter
            [/side]
            [side]
                side=2
                type=Fighter
                [village]
                    x,y=2,1
                [/village]
                [village]
                    x,y=3,1
                [/village]
            [/side]
            "#,
        )
        .unwrap();
        let type_data = wml::parse(
            r#"
            id=Fighter
            hitpoints=10
            movement=3
            movement_type=foot
            level=1
            [attack]
                name=sword
                type=blade
                range=melee
                damage=6
                number=1
            [/attack]
            "#,
        )
        .unwrap();
        let roster = BTreeMap::from([("Fighter".to_owned(), UnitType::from_wml(type_data))]);
        let terrain = TerrainAliases::from_wml(
            &wml::parse(
                "[terrain_type]\nid=flat\nstring=Gg\n[/terrain_type]\n\
                 [terrain_type]\nstring=Vh\ngives_income=yes\nmvt_alias=Gg\n[/terrain_type]",
            )
            .unwrap(),
        );
        let movement = read_movement_types(
            &wml::parse("[movetype]\nname=foot\n[movement_costs]\nflat=1\n[/movement_costs]\n[/movetype]")
                .unwrap(),
        );
        let resistance = BTreeMap::new();
        let start = scenario_start::load(&scenario, &roster, |_| unreachable!()).unwrap();
        let mut game =
            FixedMatch::new(start, Rules { terrain: &terrain, movement: &movement, resistance: &resistance }, 5489)
                .unwrap();
        let mut play = |action, side| {
            let (observation, actions) = game.decision().unwrap();
            assert_eq!(observation.side, side);
            assert!(actions.contains(&action));
            game.step(action).unwrap()
        };
        let from = Hex { x: 2, y: 0 };
        let to = Hex { x: 3, y: 0 };
        let attack = Action::Attack(AttackChoice { from, to, weapon: 0 });
        assert_eq!(
            play(Action::Move { from: Hex { x: 0, y: 0 }, to: from }, 1),
            MatchState::Playing(Turn { round: 1, side: 1 })
        );
        play(attack.clone(), 1);
        play(Action::EndTurn, 1);
        assert_eq!(play(Action::EndTurn, 2), MatchState::Playing(Turn { round: 2, side: 1 }));
        assert_eq!(play(attack, 1), MatchState::Won(1));
        assert!(game.decision().is_none());
        assert_eq!(game.step(Action::EndTurn), Err(MatchError::Finished));

        let start = scenario_start::load(&scenario, &roster, |_| unreachable!()).unwrap();
        let mut game = FixedMatch::new(
            start,
            Rules { terrain: &terrain, movement: &movement, resistance: &resistance },
            5489,
        )
        .unwrap();
        for _ in 0..3 {
            assert!(matches!(game.step(Action::EndTurn), Ok(MatchState::Playing(_))));
        }
        assert_eq!(game.step(Action::EndTurn), Ok(MatchState::Draw));
        assert!(game.decision().is_none());

        let start = scenario_start::load(&scenario, &roster, |_| unreachable!()).unwrap();
        let mut game = FixedMatch::new(
            start,
            Rules { terrain: &terrain, movement: &movement, resistance: &resistance },
            5489,
        )
        .unwrap();
        let village = Hex { x: 1, y: 0 };
        let (observation, _) = game.decision().unwrap();
        assert_eq!(observation.tiles[1].village_owner, Some(2));
        assert_eq!(observation.tiles[2].village_owner, None);
        game.step(Action::Move { from: Hex { x: 0, y: 0 }, to: village }).unwrap();
        let (observation, _) = game.decision().unwrap();
        assert_eq!(observation.tiles[1].village_owner, Some(1));
        assert_eq!(observation.units[0].moves_left, Some(0));
    }

    #[test]
    fn unit_movement_costs_override_selected_inherited_terrains() {
        let bundle = crate::scenario_bundle::Bundle::parse(
            br#"
            [terrain_type]
                id=flat
                string=Gg
            [/terrain_type]
            [terrain_type]
                id=hills
                string=Hh
            [/terrain_type]
            [terrain_type]
                id=shallow_water
                string=Ww
            [/terrain_type]
            [units]
                [movetype]
                    name=foot
                    [movement_costs]
                        flat=1
                        hills=1
                        shallow_water=1
                    [/movement_costs]
                [/movetype]
                [unit_type]
                    id=Fighter
                    movement=4
                    movement_type=foot
                    [movement_costs]
                        flat=4
                        shallow_water=unknown
                    [/movement_costs]
                    [movement_costs]
                        flat=2
                    [/movement_costs]
                [/unit_type]
            [/units]
            [scenario]
                map_data="Xv,Xv,Xv,Xv,Xv,Xv,Xv,Xv
Xv,1 Gg,Gg,Hh,Ww,Gg,2 Gg,Xv
Xv,Xv,Xv,Xv,Xv,Xv,Xv,Xv"
                [side]
                    side=1
                    type=Fighter
                [/side]
                [side]
                    side=2
                    type=Fighter
                [/side]
            [/scenario]
            "#,
        )
        .unwrap();
        let mut game = bundle.start(5489, |_| unreachable!()).unwrap();
        let [home, grass, hill, water] = [0, 1, 2, 3].map(|x| Hex { x, y: 0 });
        let (_, actions) = game.decision().unwrap();
        assert!(actions.contains(&Action::Move { from: home, to: hill }));
        assert!(!actions.contains(&Action::Move { from: home, to: water }));

        game.step(Action::Move { from: home, to: grass }).unwrap();
        assert_eq!(game.scenario.board.unit_at(grass).unwrap().moves_left, 2);
        game.step(Action::Move { from: grass, to: hill }).unwrap();
        assert_eq!(game.scenario.board.unit_at(hill).unwrap().moves_left, 1);
        assert_eq!(
            game.step(Action::Move { from: hill, to: water }),
            Err(MatchError::Move(MoveError::Unreachable(water))),
        );
    }

    #[test]
    fn applies_terrain_healing_only_when_the_units_side_begins_its_turn() {
        let scenario = wml::parse(
            r#"
            map_data="Xv,Xv,Xv,Xv,Xv
Xv,1 Gg^Vh,2 Gg^Vh,Qq,Xv
Xv,Xv,Xv,Xv,Xv"
            [side]
                side=1
                type=Fighter
                [unit]
                    type=Fighter
                    x,y=3,1
                [/unit]
            [/side]
            [side]
                side=2
                type=Fighter
            [/side]
            "#,
        )
        .unwrap();
        let fighter = UnitType::from_wml(
            wml::parse(
                "id=Fighter\nhitpoints=10\n[attack]\nname=sword\ntype=blade\nrange=melee\ndamage=5\nnumber=1\n[/attack]",
            )
            .unwrap(),
        );
        let roster = BTreeMap::from([(fighter.id.clone(), fighter)]);
        let terrain = TerrainAliases::from_wml(
            &wml::parse(
                "[terrain_type]\nstring=Gg\n[/terrain_type]\n\
                 [terrain_type]\nstring=^Vh\ngives_income=yes\nheals=8\n[/terrain_type]\n\
                 [terrain_type]\nstring=Qq\nheals=-20\n[/terrain_type]",
            )
            .unwrap(),
        );
        let movement = BTreeMap::new();
        let resistance = BTreeMap::new();
        let start = scenario_start::load(&scenario, &roster, |_| unreachable!()).unwrap();
        let mut game = FixedMatch::new(
            start,
            Rules { terrain: &terrain, movement: &movement, resistance: &resistance },
            5489,
        )
        .unwrap();
        let hitpoints = |game: &FixedMatch<'_>| {
            [0, 1, 2].map(|x| game.scenario.board.unit_at(Hex { x, y: 0 }).unwrap().hitpoints)
        };
        assert_eq!(hitpoints(&game), [10, 10, 10]);
        game.step(Action::Attack(AttackChoice {
            from: Hex { x: 0, y: 0 },
            to: Hex { x: 1, y: 0 },
            weapon: 0,
        }))
        .unwrap();
        assert_eq!(hitpoints(&game), [5, 5, 10]);
        game.step(Action::EndTurn).unwrap();
        assert_eq!(hitpoints(&game), [5, 10, 10]);
        game.step(Action::EndTurn).unwrap();
        assert_eq!(hitpoints(&game), [10, 10, 1]);
    }

    #[test]
    fn chooses_the_best_eligible_retaliation_and_keeps_the_first_tie() {
        let scenario = wml::parse(
            r#"
            map_data="Xv,Xv,Xv,Xv
Xv,1 Gg,2 Gg,Xv
Xv,Xv,Xv,Xv"
            [side]
                side=1
                type=Attacker
            [/side]
            [side]
                side=2
                type=Defender
            [/side]
            "#,
        )
        .unwrap();
        let attacker = UnitType::from_wml(
            wml::parse(
                r#"
                id=Attacker
                hitpoints=10
                [attack]
                    range=melee
                    damage=1
                    number=1
                [/attack]
                "#,
            )
            .unwrap(),
        );
        let defender = UnitType::from_wml(
            wml::parse(
                r#"
                id=Defender
                hitpoints=10
                [attack]
                    range=melee
                    damage=3
                    number=1
                [/attack]
                [attack]
                    range=ranged
                    damage=10
                    number=1
                [/attack]
                [attack]
                    range=melee
                    min_range=2
                    max_range=2
                    damage=10
                    number=1
                [/attack]
                [attack]
                    range=melee
                    defense_weight=0
                    damage=10
                    number=1
                [/attack]
                [attack]
                    range=melee
                    defense_weight=0.25
                    damage=7
                    number=1
                [/attack]
                [attack]
                    range=melee
                    damage=7
                    number=1
                [/attack]
                "#,
            )
            .unwrap(),
        );
        let roster = BTreeMap::from([(attacker.id.clone(), attacker), (defender.id.clone(), defender)]);
        let start = scenario_start::load(&scenario, &roster, |_| unreachable!()).unwrap();
        let terrain = TerrainAliases::from_wml(&wml::Node::default());
        let movement = BTreeMap::new();
        let resistance = BTreeMap::new();
        let mut game = FixedMatch::new(
            start,
            Rules { terrain: &terrain, movement: &movement, resistance: &resistance },
            5489,
        )
        .unwrap();
        let from = Hex { x: 0, y: 0 };
        let to = Hex { x: 1, y: 0 };
        let choice = AttackChoice { from, to, weapon: 0 };
        let attack = game.prepare_attack(Turn { round: 1, side: 1 }, choice).unwrap();
        assert_eq!(attack.retaliation.map(|(index, _)| index), Some(4));
        game.step(Action::Attack(choice)).unwrap();
        assert_eq!(game.scenario.board.unit_at(from).unwrap().hitpoints, 3);
        assert_eq!(game.rng.seed_and_calls(), (5489, 2));
    }

    #[test]
    fn prefers_lethal_firststrike_retaliation_and_applies_it_before_the_attack() {
        let scenario = wml::parse(
            r#"
            map_data="Xv,Xv,Xv,Xv
Xv,1 Gg,2 Gg,Xv
Xv,Xv,Xv,Xv"
            [side]
                side=1
                type=Attacker
            [/side]
            [side]
                side=2
                type=Defender
            [/side]
            "#,
        )
        .unwrap();
        let ordinary_attack = r#"
            [attack]
                range=melee
                damage=10
                number=1
            [/attack]
        "#;
        let firststrike_attack = r#"
            [attack]
                range=melee
                damage=10
                number=1
                [specials]
                    [firststrike]
                        id=firststrike
                    [/firststrike]
                [/specials]
            [/attack]
        "#;
        let attacker = UnitType::from_wml(
            wml::parse(format!("id=Attacker\nhitpoints=10\n{ordinary_attack}")).unwrap(),
        );
        let defender = UnitType::from_wml(
            wml::parse(format!(
                "id=Defender\nhitpoints=10\n{ordinary_attack}{firststrike_attack}"
            ))
            .unwrap(),
        );
        let roster = BTreeMap::from([(attacker.id.clone(), attacker), (defender.id.clone(), defender)]);
        let start = scenario_start::load(&scenario, &roster, |_| unreachable!()).unwrap();
        let terrain = TerrainAliases::from_wml(&wml::Node::default());
        let movement = BTreeMap::new();
        let resistance = BTreeMap::new();
        let mut game = FixedMatch::new(
            start,
            Rules { terrain: &terrain, movement: &movement, resistance: &resistance },
            5489,
        )
        .unwrap();
        let from = Hex { x: 0, y: 0 };
        let to = Hex { x: 1, y: 0 };
        let choice = AttackChoice { from, to, weapon: 0 };
        let attack = game.prepare_attack(Turn { round: 1, side: 1 }, choice).unwrap();
        assert_eq!(attack.retaliation.map(|(index, _)| index), Some(1));
        assert_eq!(game.step(Action::Attack(choice)), Ok(MatchState::Won(2)));
        assert!(game.scenario.board.unit_at(from).is_none());
        assert_eq!(game.scenario.board.unit_at(to).unwrap().hitpoints, 10);
        assert_eq!(game.rng.seed_and_calls(), (5489, 1));
    }

    #[test]
    fn applies_the_strikers_fearless_effect_to_attack_and_retaliation() {
        let unit = |id, reset| format!(r#"[unit_type]
            id={id}
            hitpoints=40
            alignment=chaotic
            [trait]
                id=resolve
                availability=musthave
                [effect]
                    apply_to=fearless
                [/effect]
                {reset}
            [/trait]
            [attack]
                range=melee
                damage=8
                number=1
                accuracy=100
            [/attack]
        [/unit_type]"#);
        let reset = "[effect]\napply_to=fearless\nset=no\n[/effect]";
        let types = unit("Fearless", "") + &unit("Ordinary", reset);
        for (attacker, defender, daylight, hp) in [
            ("Fearless", "Ordinary", 25, (34, 32)),
            ("Ordinary", "Fearless", 25, (32, 34)),
            ("Fearless", "Ordinary", -25, (30, 30)),
        ] {
            let data = format!(r#"[units]
                {types}
            [/units]
            [scenario]
                map_data="Xv,Xv,Xv,Xv
Xv,1 Gg,2 Gg,Xv
Xv,Xv,Xv,Xv"
                [time]
                    id=light
                    lawful_bonus={daylight}
                [/time]
                [side]
                    side=1
                    type={attacker}
                [/side]
                [side]
                    side=2
                    type={defender}
                [/side]
            [/scenario]"#);
            let bundle = crate::scenario_bundle::Bundle::parse(data.as_bytes()).unwrap();
            let mut game = bundle.start(5489, |_| unreachable!()).unwrap();
            let from = Hex { x: 0, y: 0 };
            let to = Hex { x: 1, y: 0 };
            game.step(Action::Attack(AttackChoice { from, to, weapon: 0 })).unwrap();
            let board = &game.scenario.board;
            assert_eq!(
                (board.unit_at(from).unwrap().hitpoints, board.unit_at(to).unwrap().hitpoints),
                hp,
            );
        }
    }

    #[test]
    fn strongest_adjacent_same_side_leadership_adds_to_daylight_for_each_striker() {
        let leader = |id: &str, level: i32| format!(r#"[unit_type]
            id={id}
            level={level}
            [abilities]
                [leadership]
                    id=leadership
                    value="(25 * (level - other.level))"
                    cumulative=no
                    affect_self=no
                    [affect_adjacent]
                        [filter]
                            formula="level < other.level"
                        [/filter]
                    [/affect_adjacent]
                [/leadership]
            [/abilities]
        [/unit_type]"#);
        let data = format!(r#"[units]
            {strong}
            {weak}
            {enemy}
            [unit_type]
                id=Soldier
                level=1
                alignment=lawful
                hitpoints=40
                [attack]
                    range=melee
                    damage=8
                    number=1
                    accuracy=100
                [/attack]
            [/unit_type]
        [/units]
        [scenario]
            map_data="Xv,Xv,Xv,Xv,Xv
Xv,Gg,Gg,Gg,Xv
Xv,1 Gg,Gg,Gg,Xv
Xv,Gg,Gg,2 Gg,Xv
Xv,Xv,Xv,Xv,Xv"
            [time]
                id=day
                lawful_bonus=25
            [/time]
            [side]
                side=1
                type=Commander
                [unit]
                    type=Lieutenant
                    x,y=2,1
                [/unit]
                [unit]
                    type=Soldier
                    x,y=2,2
                [/unit]
            [/side]
            [side]
                side=2
                type=Marshal
                [unit]
                    type=Soldier
                    x,y=3,2
                [/unit]
            [/side]
        [/scenario]"#,
            strong = leader("Commander", 3), weak = leader("Lieutenant", 2),
            enemy = leader("Marshal", 4),
        );
        let bundle = crate::scenario_bundle::Bundle::parse(data.as_bytes()).unwrap();
        let mut game = bundle.start(5489, |_| unreachable!()).unwrap();
        let from = Hex { x: 1, y: 1 };
        let to = Hex { x: 2, y: 1 };
        game.step(Action::Attack(AttackChoice { from, to, weapon: 0 })).unwrap();
        assert_eq!(game.scenario.board.unit_at(from).unwrap().hitpoints, 24);
        assert_eq!(game.scenario.board.unit_at(to).unwrap().hitpoints, 26);
    }

    #[test]
    fn magical_hit_chance_drives_both_retaliation_selection_and_combat() {
        let bundle = crate::scenario_bundle::Bundle::parse(
            br#"
            [units]
                [unit_type]
                    id=Attacker
                    hitpoints=10
                    [attack]
                        range=melee
                        damage=1
                        number=1
                        parry=90
                    [/attack]
                [/unit_type]
                [unit_type]
                    id=Defender
                    hitpoints=10
                    [attack]
                        range=melee
                        damage=2
                        number=1
                    [/attack]
                    [attack]
                        range=melee
                        damage=8
                        number=1
                        accuracy=-100
                        [specials]
                            [chance_to_hit]
                                id=magical
                                value=70
                                cumulative=no
                            [/chance_to_hit]
                        [/specials]
                    [/attack]
                [/unit_type]
            [/units]
            [scenario]
                map_data="Xv,Xv,Xv,Xv
Xv,1 Gg,2 Gg,Xv
Xv,Xv,Xv,Xv"
                [side]
                    side=1
                    type=Attacker
                [/side]
                [side]
                    side=2
                    type=Defender
                [/side]
            [/scenario]
            "#,
        )
        .unwrap();
        let mut game = bundle.start(5489, |_| unreachable!()).unwrap();
        let from = Hex { x: 0, y: 0 };
        let to = Hex { x: 1, y: 0 };
        let choice = AttackChoice { from, to, weapon: 0 };
        let attack = game.prepare_attack(Turn { round: 1, side: 1 }, choice).unwrap();
        let (response, modifiers) = attack.retaliation.unwrap();
        assert_eq!(response, 1);
        let defender = game.scenario.board.unit_at(to).unwrap();
        let weapon = &game.scenario.board.unit_at(from).unwrap().unit_type.attacks[choice.weapon];
        for parry in [90, -200] {
            let stats = combatant(defender, response, &modifiers, parry, StrikeRole::Retaliate(weapon));
            assert_eq!(stats.hit_chance, 70);
        }

        game.step(Action::Attack(choice)).unwrap();
        assert_eq!(game.scenario.board.unit_at(from).unwrap().hitpoints, 2);
        assert_eq!(game.scenario.board.unit_at(to).unwrap().hitpoints, 9);
    }

    #[test]
    fn charge_doubles_both_base_damages_only_when_used_to_attack() {
        let scenario = wml::parse(
            r#"
            map_data="Xv,Xv,Xv,Xv
Xv,1 Gg,2 Gg,Xv
Xv,Xv,Xv,Xv"
            [side]
                side=1
                type=Attacker
            [/side]
            [side]
                side=2
                type=Defender
            [/side]
            "#,
        )
        .unwrap();
        let charge = r#"
            [specials]
                [damage]
                    id=charge
                    multiply=2
                    apply_to=both
                    active_on=offense
                [/damage]
            [/specials]
        "#;
        let attacker = || {
            UnitType::from_wml(
                wml::parse(format!(
                    r#"
                    id=Attacker
                    hitpoints=20
                    [attack]
                        type=pierce
                        range=melee
                        damage=3
                        number=1
                        {charge}
                    [/attack]
                    [attack]
                        type=pierce
                        range=melee
                        damage=3
                        number=1
                    [/attack]
                    "#,
                ))
                .unwrap(),
            )
        };
        let cases = [
            // Charge changes 3 to 6 before the 75% resistance rounds 4.5 up to 5.
            (20, 0, 1, 1, (12, Some(15))),
            // A lethal charge gives both weapons identical retaliation outcomes.
            (5, 0, 1, 0, (20, None)),
            // The defender's charge deals normal damage when it retaliates.
            (20, 1, 0, 0, (17, Some(18))),
            (20, 1, 1, 1, (16, Some(18))),
        ];
        for (defender_hp, weapon, stronger_weight, response, hitpoints) in cases {
            let defender = UnitType::from_wml(
                wml::parse(format!(
                    r#"
                    id=Defender
                    hitpoints={defender_hp}
                    [resistance]
                        pierce=75
                    [/resistance]
                    [attack]
                        type=blade
                        range=melee
                        damage=3
                        number=1
                        {charge}
                    [/attack]
                    [attack]
                        type=blade
                        range=melee
                        damage=4
                        number=1
                        defense_weight={stronger_weight}
                    [/attack]
                    "#,
                ))
                .unwrap(),
            );
            let attacker = attacker();
            let roster = BTreeMap::from([(attacker.id.clone(), attacker), (defender.id.clone(), defender)]);
            let start = scenario_start::load(&scenario, &roster, |_| unreachable!()).unwrap();
            let terrain = TerrainAliases::from_wml(&wml::Node::default());
            let movement = BTreeMap::new();
            let resistance = BTreeMap::new();
            let rules = Rules { terrain: &terrain, movement: &movement, resistance: &resistance };
            let mut game = FixedMatch::new(start, rules, 5489).unwrap();
            let from = Hex { x: 0, y: 0 };
            let to = Hex { x: 1, y: 0 };
            let choice = AttackChoice { from, to, weapon };
            let attack = game.prepare_attack(Turn { round: 1, side: 1 }, choice).unwrap();
            assert_eq!(attack.retaliation.map(|(index, _)| index), Some(response));
            game.step(Action::Attack(choice)).unwrap();
            let board = &game.scenario.board;
            assert_eq!(
                (board.unit_at(from).unwrap().hitpoints, board.unit_at(to).map(|unit| unit.hitpoints)),
                hitpoints,
            );
        }
    }

    #[test]
    fn berserk_on_either_engaged_weapon_repeats_combat_up_to_thirty_rounds() {
        let special = "[specials]\n[berserk]\nid=berserk\nvalue=30\n[/berserk]\n[/specials]";
        let weapon = |range, damage, special| format!(
            "[attack]\nrange={range}\ndamage={damage}\naccuracy=100\nnumber=1\n{special}\n[/attack]\n"
        );
        let defensive_choices = weapon("melee", 2, "") + &weapon("melee", 1, special);
        let cases = [
            // Offensive berserk runs all 30 rounds, even without retaliation.
            (100, weapon("melee", 1, special), weapon("ranged", 1, ""), Some(100), 70, 30),
            // Defender prefers the lethal berserk response over the stronger single strike.
            (25, weapon("melee", 1, ""), defensive_choices, None, 75, 50),
            // A berserk weapon outside the attack's range cannot prolong the exchange.
            (25, weapon("ranged", 1, ""), weapon("melee", 1, special), Some(25), 99, 1),
        ];
        for (hp, attack, retaliation, attacker_hp, defender_hp, draws) in cases {
            let scenario = format!(
                r#"[units]
                    [unit_type]
                        id=Attacker
                        hitpoints={hp}
                        {attack}
                    [/unit_type]
                    [unit_type]
                        id=Defender
                        hitpoints=100
                        {retaliation}
                    [/unit_type]
                [/units]
                [scenario]
                    map_data="Xv,Xv,Xv,Xv
Xv,1 Gg,2 Gg,Xv
Xv,Xv,Xv,Xv"
                    [side]
                        side=1
                        type=Attacker
                    [/side]
                    [side]
                        side=2
                        type=Defender
                    [/side]
                [/scenario]"#,
            );
            let bundle = crate::scenario_bundle::Bundle::parse(scenario.as_bytes()).unwrap();
            let mut game = bundle.start(5489, |_| unreachable!()).unwrap();
            let from = Hex { x: 0, y: 0 };
            let to = Hex { x: 1, y: 0 };
            game.step(Action::Attack(AttackChoice { from, to, weapon: 0 })).unwrap();
            assert_eq!(game.scenario.board.unit_at(from).map(|unit| unit.hitpoints), attacker_hp);
            assert_eq!(game.scenario.board.unit_at(to).unwrap().hitpoints, defender_hp);
            assert_eq!(game.rng.seed_and_calls(), (5489, draws));
        }
    }

    #[test]
    fn heals_at_next_turn_start_only_after_resting() {
        let scenario = wml::parse(
            r#"
            map_data="Xv,Xv,Xv,Xv
Xv,1 Gg,2 Gg,Xv
Xv,Gg,Gg,Xv
Xv,Xv,Xv,Xv"
            [side]
                side=1
                type=Fighter
            [/side]
            [side]
                side=2
                type=Fighter
            [/side]
            "#,
        )
        .unwrap();
        let fighter = UnitType::from_wml(
            wml::parse(
                r#"
                id=Fighter
                hitpoints=10
                movement=3
                movement_type=foot
                [attack]
                    name=sword
                    type=blade
                    range=melee
                    movement_used=0
                    accuracy=100
                    damage=5
                    number=1
                [/attack]
                "#,
            )
            .unwrap(),
        );
        let roster = BTreeMap::from([(fighter.id.clone(), fighter)]);
        let terrain = TerrainAliases::from_wml(
            &wml::parse("[terrain_type]\nid=flat\nstring=Gg\n[/terrain_type]").unwrap(),
        );
        let movement = read_movement_types(
            &wml::parse("[movetype]\nname=foot\n[movement_costs]\nflat=1\n[/movement_costs]\n[/movetype]")
                .unwrap(),
        );
        let resistance = BTreeMap::new();
        let start = scenario_start::load(&scenario, &roster, |_| unreachable!()).unwrap();
        let mut game = FixedMatch::new(
            start,
            Rules { terrain: &terrain, movement: &movement, resistance: &resistance },
            5489,
        )
        .unwrap();
        let west = Hex { x: 0, y: 0 };
        let east = Hex { x: 1, y: 0 };
        let south = Hex { x: 0, y: 1 };
        let hp = |game: &FixedMatch<'_>, position| {
            game.scenario.board.unit_at(position).unwrap().hitpoints
        };

        game.step(Action::EndTurn).unwrap();
        game.step(Action::Attack(AttackChoice { from: east, to: west, weapon: 0 })).unwrap();
        assert_eq!((hp(&game, west), hp(&game, east)), (5, 5));
        game.step(Action::EndTurn).unwrap();
        assert_eq!(hp(&game, west), 5);
        game.step(Action::Move { from: west, to: south }).unwrap();
        game.step(Action::EndTurn).unwrap();
        assert_eq!(hp(&game, east), 5);
        game.step(Action::EndTurn).unwrap();
        assert_eq!(hp(&game, south), 5);
        game.step(Action::EndTurn).unwrap();
        assert_eq!((hp(&game, south), hp(&game, east)), (5, 7));
        game.step(Action::EndTurn).unwrap();
        assert_eq!((hp(&game, south), hp(&game, east)), (7, 7));
    }

    #[test]
    fn settles_only_the_incoming_sides_income_from_round_two() {
        let scenario = wml::parse(
            r#"
            map_data="Xv,Xv,Xv,Xv,Xv,Xv
Xv,1 Gg,Vh,Gg,2 Gg,Xv
Xv,Gg,Gg,Gg,Gg,Xv
Xv,Xv,Xv,Xv,Xv,Xv"
            [side]
                side=1
                type=Fighter
                gold=23
                income=2
                village_gold=2
                village_support=2
                [unit]
                    type=Fighter
                    x,y=1,2
                [/unit]
            [/side]
            [side]
                side=2
                type=Fighter
                gold=41
                [village]
                    x,y=2,1
                [/village]
            [/side]
            "#,
        )
        .unwrap();
        let fighter = UnitType::from_wml(
            wml::parse("id=Fighter\nlevel=3\nhitpoints=10\nmovement=3\nmovement_type=foot")
                .unwrap(),
        );
        let roster = BTreeMap::from([(fighter.id.clone(), fighter)]);
        let terrain = TerrainAliases::from_wml(
            &wml::parse(
                "[terrain_type]\nid=flat\nstring=Gg\n[/terrain_type]\n\
                 [terrain_type]\nstring=Vh\ngives_income=yes\nmvt_alias=Gg\n[/terrain_type]",
            )
            .unwrap(),
        );
        let movement = read_movement_types(
            &wml::parse(
                "[movetype]\nname=foot\n[movement_costs]\nflat=1\n[/movement_costs]\n[/movetype]",
            )
            .unwrap(),
        );
        let resistance = BTreeMap::new();
        let load = || scenario_start::load(&scenario, &roster, |_| unreachable!()).unwrap();
        let rules = || Rules {
            terrain: &terrain,
            movement: &movement,
            resistance: &resistance,
        };
        let observed_gold = |game: &FixedMatch<'_>| {
            let (observation, _) = game.decision().unwrap();
            (observation.side, observation.gold)
        };

        let mut game = FixedMatch::new(load(), rules(), 5489).unwrap();
        assert_eq!(observed_gold(&game), (1, Some(23)));
        let mut alternate = load();
        alternate.sides[1].starting_gold = 99;
        assert_eq!(
            game.decision(),
            FixedMatch::new(alternate, rules(), 5489).unwrap().decision()
        );

        game.step(Action::Move {
            from: Hex { x: 0, y: 0 },
            to: Hex { x: 1, y: 0 },
        })
        .unwrap();
        assert_eq!(observed_gold(&game), (1, Some(23)));
        game.step(Action::EndTurn).unwrap();
        assert_eq!(observed_gold(&game), (2, Some(41)));
        game.step(Action::EndTurn).unwrap();
        assert_eq!(observed_gold(&game), (1, Some(28)));
        game.step(Action::EndTurn).unwrap();
        assert_eq!(observed_gold(&game), (2, Some(43)));

        let mut later_round = load();
        later_round.start_round = 2;
        let resumed = FixedMatch::new(later_round, rules(), 5489).unwrap();
        assert_eq!(observed_gold(&resumed), (1, Some(24)));
    }

    #[test]
    fn common_ally_allows_passage_but_does_not_link_hostile_teams() {
        let bundle = crate::scenario_bundle::Bundle::parse(br#"
            [terrain_type]
                id=flat
                string=Gg
            [/terrain_type]
            [terrain_type]
                id=village
                string=Vh
                gives_income=yes
                mvt_alias=Gg
            [/terrain_type]
            [units]
                [movetype]
                    name=foot
                    [movement_costs]
                        flat=1
                    [/movement_costs]
                [/movetype]
                [unit_type]
                    id=Fighter
                    level=1
                    hitpoints=10
                    movement=4
                    movement_type=foot
                    [attack]
                        range=melee
                        accuracy=100
                        damage=10
                        number=1
                    [/attack]
                [/unit_type]
            [/units]
            [scenario]
                map_data="Xv,Xv,Xv,Xv,Xv,Xv,Xv
Xv,1 Gg,3 Gg,Gg,Vh,2 Gg,Xv
Xv,Xv,Xv,Xv,Xv,Xv,Xv"
                [side]
                    side=1
                    team_name=blue
                    type=Fighter
                [/side]
                [side]
                    side=2
                    team_name=red
                    type=Fighter
                [/side]
                [side]
                    side=3
                    team_name=blue,red
                    type=Fighter
                    [village]
                        x,y=4,1
                    [/village]
                [/side]
            [/scenario]
        "#).unwrap();
        let mut game = bundle.start(5489, |_| unreachable!()).unwrap();
        let hex = |x| Hex { x, y: 0 };
        let ally_attack = Action::Attack(AttackChoice { from: hex(0), to: hex(1), weapon: 0 });
        let movement = Action::Move { from: hex(0), to: hex(3) };
        let (observation, actions) = game.decision().unwrap();
        assert_eq!(observation.allied_sides, [1, 3]);
        assert!(!actions.contains(&ally_attack));
        assert_eq!(game.step(ally_attack), Err(MatchError::Attack(AttackError::FriendlyTarget)));
        assert!(actions.contains(&movement));
        game.step(movement).unwrap();
        assert_eq!(game.village_owners[&hex(3)], 1);
        assert_eq!(game.scenario.board.unit_at(hex(3)).unwrap().moves_left, 0);
        let attack = Action::Attack(AttackChoice { from: hex(3), to: hex(4), weapon: 0 });
        assert!(game.decision().unwrap().1.contains(&attack));
        assert_eq!(game.step(attack), Ok(MatchState::AlliedVictory(vec![1, 3])));
        assert!(game.decision().is_none());
        let mut wire = Vec::new();
        crate::audit_trace::write_snapshot(&mut wire, 2, &game.audit_snapshot()).unwrap();
        assert!(wire.starts_with(b"step\t2\twon\t1\t3\n"));
    }

    #[test]
    fn rotates_three_hostile_sides_and_uses_each_sides_gold_and_actions() {
        let bundle = crate::scenario_bundle::Bundle::parse(
            br#"
            [terrain_type]
                string=Kh
                recruit_from=yes
                recruit_onto=yes
            [/terrain_type]
            [terrain_type]
                string=Ch
                recruit_onto=yes
            [/terrain_type]
            [units]
                [unit_type]
                    id=Fighter
                    hitpoints=10
                    attacks=2
                    cost=8
                    level=1
                    [attack]
                        range=melee
                        damage=10
                        number=1
                    [/attack]
                [/unit_type]
            [/units]
            [scenario]
                turns=2
                map_data="Xv,Xv,Xv,Xv,Xv
Xv,1 Gg,3 Kh,Ch,Xv
Xv,2 Gg,Gg,Gg,Xv
Xv,Xv,Xv,Xv,Xv"
                [side]
                    side=1
                    team_name=west
                    type=Fighter
                    gold=34
                [/side]
                [side]
                    side=2
                    team_name=south
                    type=Fighter
                    gold=38
                [/side]
                [side]
                    side=3
                    team_name=east
                    type=Fighter
                    gold=42
                    income=3
                    recruit=Fighter
                [/side]
            [/scenario]
            "#,
        )
        .unwrap();
        let mut game = bundle.start(5489, |_| unreachable!()).unwrap();
        let recruit = Action::Recruit {
            unit_type: "Fighter".to_owned(),
            to: Hex { x: 2, y: 0 },
        };
        for (round, side, gold) in [
            (1, 1, [34, 38, 42]),
            (1, 2, [34, 38, 42]),
            (1, 3, [34, 38, 42]),
            (2, 1, [36, 38, 34]),
            (2, 2, [36, 40, 34]),
            (2, 3, [36, 40, 38]),
        ] {
            assert_eq!(game.state(), MatchState::Playing(Turn { round, side }));
            assert_eq!(game.audit_snapshot().gold, gold);
            let (observation, actions) = game.decision().unwrap();
            assert_eq!((observation.side, observation.gold), (side, Some(gold[side - 1])));
            assert!(
                observation.units.iter().all(|unit| unit.moves_left.is_some() == (unit.side == side))
            );
            if (round, side) == (1, 3) {
                assert!(actions.contains(&recruit));
                game.step(recruit.clone()).unwrap();
                assert_eq!(game.audit_snapshot().gold, [34, 38, 34]);
            }
            game.step(Action::EndTurn).unwrap();
        }
        assert_eq!(game.state(), MatchState::Draw);
        assert_eq!(game.audit_snapshot().gold, [36, 40, 38]);
        let mut trace = Vec::new();
        crate::audit_trace::write_snapshot(&mut trace, 0, &game.audit_snapshot()).unwrap();
        assert!(String::from_utf8(trace).unwrap().contains("gold\t3\t38\n"));

        let mut game = bundle.start(5489, |_| unreachable!()).unwrap();
        game.step(Action::EndTurn).unwrap();
        game.step(Action::EndTurn).unwrap();
        let attack = |y| {
            Action::Attack(AttackChoice {
                from: Hex { x: 1, y: 0 },
                to: Hex { x: 0, y },
                weapon: 0,
            })
        };
        let (_, actions) = game.decision().unwrap();
        assert!(actions.contains(&attack(0)));
        assert!(actions.contains(&attack(1)));
        assert_eq!(game.step(attack(0)), Ok(MatchState::Playing(Turn { round: 1, side: 3 })));
        assert_eq!(game.step(attack(1)), Ok(MatchState::Won(3)));
    }

    #[test]
    fn scenario_defeat_rules_distinguish_leaders_armies_and_permanent_sides() {
        let scenario = r#"[units]
[unit_type]
id=Guard
hitpoints=12
[/unit_type]
[/units]
[scenario]
map_data="Xv,Xv,Xv,Xv,Xv
Xv,1 Kh,2 Kh,Gg,Xv
Xv,Xv,Xv,Xv,Xv"
[side]
type=Guard
[/side]
[side]
no_leader=yes
defeat_condition=CONDITION
TROOPS
[/side]
[/scenario]"#;
        let soldier = "[unit]\ntype=Guard\nx,y=3,1\n[/unit]";
        for (condition, troops, active) in [
            ("no_leader_left", soldier, false), ("no_units_left", soldier, true),
            ("no_units_left", "", false), ("never", "", true),
            ("always", "[leader]\ntype=Guard\n[/leader]", false),
        ] {
            let input = scenario.replace("CONDITION", condition).replace("TROOPS", troops);
            let bundle = crate::scenario_bundle::Bundle::parse(input.as_bytes()).unwrap();
            let game = bundle.start(3, |_| unreachable!()).unwrap();
            let expected = if active { MatchState::Playing(Turn { round: 1, side: 1 }) } else { MatchState::Won(1) };
            assert_eq!(game.state(), expected, "{condition} with {troops}");
        }
    }

    #[test]
    fn end_turn_releases_villages_after_the_last_leader_is_lost() {
        let bundle = crate::scenario_bundle::Bundle::parse(
            br#"
            [terrain_type]
                string=Gg^Vh
                gives_income=yes
            [/terrain_type]
            [units]
                [unit_type]
                    id=Fighter
                    hitpoints=10
                    [attack]
                        range=melee
                        damage=10
                        number=1
                    [/attack]
                [/unit_type]
            [/units]
            [scenario]
                turns=2
                map_data="Xv,Xv,Xv,Xv,Xv,Xv
Xv,1 Gg,2 Gg,3 Gg,Gg^Vh,Xv
Xv,Gg,Gg,Gg,Gg^Vh,Xv
Xv,Xv,Xv,Xv,Xv,Xv"
                [side]
                    side=1
                    type=Fighter
                [/side]
                [side]
                    side=2
                    type=Fighter
                    gold=20
                    [leader]
                        type=Fighter
                        x,y=1,2
                    [/leader]
                    [village]
                        x,y=4,1
                    [/village]
                [/side]
                [side]
                    side=3
                    type=Fighter
                    [village]
                        x,y=4,2
                    [/village]
                [/side]
            [/scenario]
            "#,
        )
        .unwrap();
        let mut game = bundle.start(5489, |_| unreachable!()).unwrap();
        let second_village = (Hex { x: 3, y: 0 }, 2);
        let third_village = (Hex { x: 3, y: 1 }, 3);
        let attack = |to| Action::Attack(AttackChoice { from: Hex { x: 0, y: 0 }, to, weapon: 0 });

        game.step(attack(Hex { x: 1, y: 0 })).unwrap();
        game.step(Action::EndTurn).unwrap();
        assert_eq!(game.audit_snapshot().villages, [second_village, third_village]);

        game.step(Action::EndTurn).unwrap();
        game.step(Action::EndTurn).unwrap();
        game.step(attack(Hex { x: 0, y: 1 })).unwrap();
        assert_eq!(game.audit_snapshot().villages, [second_village, third_village]);

        assert_eq!(
            game.step(Action::EndTurn),
            Ok(MatchState::Playing(Turn { round: 2, side: 2 }))
        );
        let refresh = game.audit_snapshot();
        assert_eq!(refresh.villages, [third_village]);
        assert_eq!(refresh.gold[1], 20 + i64::from(DEFAULT_BASE_INCOME));
    }
}
