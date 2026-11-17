use std::{collections::BTreeMap, error, fmt, num::NonZeroU32};

use crate::{
    board::{
        Attack, AttackChoice, AttackError, MoveError, StrikeModifiers, UnitState, combatant,
        within_range,
    },
    hex::Hex,
    income::{DEFAULT_BASE_INCOME, net_income},
    movement_types::MovementType,
    observation::{Observation, project},
    resistance::DamageTable,
    rng::ReplayRng,
    scenario_start::ScenarioStart,
    terrain_aliases::TerrainAliases,
    turn::Turn,
    unit_type::UnitType,
    visibility::VisibilityMask,
    weapon::WeaponProfile,
};

pub mod audit;
mod recruit;
mod retaliation;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Action {
    Move { from: Hex, to: Hex },
    Attack(AttackChoice),
    Recruit { unit_type: String, to: Hex },
    EndTurn,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MatchState {
    Playing(Turn),
    Won(usize),
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
}

/// A headless match on a fixed, fully visible map with two opposing sides.
///
/// This match uses base unit stats and ordinary weapons. It ends when a
/// side loses its last leader or the scenario reaches the round limit.
pub struct FixedMatch<'data> {
    scenario: ScenarioStart<'data>,
    rules: Rules<'data>,
    rng: ReplayRng,
    state: MatchState,
    village_owners: BTreeMap<Hex, usize>,
    gold: [i64; 2],
}

impl<'data> FixedMatch<'data> {
    pub fn new(
        scenario: ScenarioStart<'data>,
        rules: Rules<'data>,
        seed: u32,
    ) -> Result<Self, MatchError> {
        let [first, second] = scenario.sides.as_slice() else {
            return Err(MatchError::UnsupportedScenario);
        };
        if first.side != 1
            || second.side != 2
            || !first.is_enemy(second)
            || first.fog
            || first.shroud
            || second.fog
            || second.shroud
            || scenario
                .board
                .units()
                .any(|(_, unit)| !matches!(unit.side, 1 | 2) || !supports_unit(unit.unit_type))
        {
            return Err(MatchError::UnsupportedScenario);
        }
        let gold = [i64::from(first.starting_gold), i64::from(second.starting_gold)];
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
            state: MatchState::Playing(Turn {
                round: scenario.start_round,
                side: 1,
            }),
            scenario,
            rules,
            rng: ReplayRng::new(seed),
            village_owners,
            gold,
        };
        game.check_leaders();
        if let MatchState::Playing(turn) = game.state {
            if game.scenario.last_round.is_some_and(|limit| turn.round > limit) {
                game.state = MatchState::Draw;
            } else {
                game.settle_income(turn);
                game.scenario.board.begin_resting(turn.side);
            }
        }
        Ok(game)
    }

    pub fn state(&self) -> MatchState {
        self.state
    }

    /// The active side's observation and actions, without referee state.
    pub fn decision(&self) -> Option<(Observation, Vec<Action>)> {
        let MatchState::Playing(turn) = self.state else {
            return None;
        };
        let board = &self.scenario.board;
        let opponent = opposing_side(turn.side);
        let vision = VisibilityMask::new(board.map().width(), board.map().height(), false, false);
        let mut observation = project(board, turn.side, &vision, |_| true);
        observation.gold = Some(self.gold[turn.side - 1]);
        for tile in &mut observation.tiles {
            tile.village_owner = self.village_owners.get(&tile.position).copied();
        }
        let mut actions = Vec::new();
        for (from, unit) in board.units().filter(|(_, unit)| unit.side == turn.side) {
            let destinations = board
                .move_destinations(
                    turn.side,
                    from,
                    |terrain| self.rules.walking_cost(unit.unit_type, terrain),
                    |side| side == opponent,
                )
                .expect("owned move origin");
            actions.extend(destinations.into_keys().map(|to| Action::Move { from, to }));
        }
        actions.extend(
            board
                .attack_choices(turn.side, |side| side == opponent)
                .into_iter()
                .map(Action::Attack),
        );
        self.extend_recruit_choices(turn.side, &mut actions);
        actions.push(Action::EndTurn);
        Some((observation, actions))
    }

    pub fn step(&mut self, action: Action) -> Result<MatchState, MatchError> {
        let MatchState::Playing(turn) = self.state else {
            return Err(MatchError::Finished);
        };
        let opponent = opposing_side(turn.side);
        match action {
            Action::Move { from, to } => {
                let board = &mut self.scenario.board;
                let unit_type = board.unit_at(from).ok_or(MoveError::MissingUnit(from))?.unit_type;
                board.move_unit(
                    turn.side,
                    from,
                    to,
                    |terrain| self.rules.walking_cost(unit_type, terrain),
                    |side| side == opponent,
                )?;
                if self.capture_village(turn.side, to) {
                    self.scenario.board.stop_movement(to);
                }
            }
            Action::Attack(choice) => {
                let attack = self.prepare_attack(turn, choice)?;
                self.scenario.board.apply_attack(
                    turn.side,
                    attack,
                    |side| side == opponent,
                    &mut self.rng,
                )?;
                self.check_leaders();
            }
            Action::Recruit { unit_type, to } => self.recruit(turn.side, &unit_type, to)?,
            Action::EndTurn => {
                let next = self
                    .scenario
                    .board
                    .advance_turn(turn, &[1, 2], self.scenario.last_round);
                self.state = if let Some(next) = next {
                    self.settle_income(next);
                    self.scenario.board.heal_at_turn_start(next.side, self.rules.terrain);
                    MatchState::Playing(next)
                } else {
                    MatchState::Draw
                };
            }
        }
        Ok(self.state)
    }

    fn capture_village(&mut self, side: usize, to: Hex) -> bool {
        if self.village_owners.get(&to).copied() == Some(side)
            || !self
                .scenario
                .board
                .map()
                .terrain_at(to)
                .is_some_and(|code| self.rules.terrain.features(code).village)
        {
            return false;
        }
        self.village_owners.insert(to, side);
        true
    }

    fn settle_income(&mut self, turn: Turn) {
        if turn.round <= 1 {
            return;
        }

        let villages = self
            .village_owners
            .values()
            .filter(|&&side| side == turn.side)
            .count();
        let upkeep = self
            .scenario
            .board
            .units()
            .filter(|(_, unit)| unit.side == turn.side && !unit.can_recruit)
            .map(|(_, unit)| unit.unit_type.level)
            .sum();
        let index = turn.side - 1;
        let side = &self.scenario.sides[index];
        self.gold[index] += net_income(side, DEFAULT_BASE_INCOME, villages, upkeep);
    }

    fn prepare_attack(&self, turn: Turn, choice: AttackChoice) -> Result<Attack, AttackError> {
        let board = &self.scenario.board;
        let attacker = board.unit_at(choice.from).ok_or(AttackError::MissingAttacker)?;
        let defender = board.unit_at(choice.to).ok_or(AttackError::MissingDefender)?;
        let weapon = attacker
            .unit_type
            .attacks
            .get(choice.weapon)
            .ok_or(AttackError::UnavailableWeapon)?;
        let distance = choice.from.distance(choice.to) as i64;
        let modifiers = self.modifiers(turn, attacker, defender, weapon, choice.to);
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
                let response_modifiers = self.modifiers(turn, defender, attacker, response, choice.from);
                let attacker_stats = combatant(attacker, weapon, &modifiers, response.parry);
                let defender_stats = combatant(defender, response, &response_modifiers, weapon.parry);
                ((index, response_modifiers), attacker_stats, defender_stats)
            });
        Ok(Attack {
            from: choice.from,
            to: choice.to,
            weapon: choice.weapon,
            retaliation: retaliation::select(candidates),
            modifiers,
        })
    }

    fn modifiers(
        &self,
        turn: Turn,
        striker: &UnitState<'_>,
        target: &UnitState<'_>,
        weapon: &WeaponProfile,
        target_hex: Hex,
    ) -> StrikeModifiers {
        let terrain = self.scenario.board.map().terrain_at(target_hex).expect("unit on map");
        let target_movement = &target.unit_type.movement_type;
        let target_hit_chance = self.rules.movement.get(target_movement).map_or(100, |profile| {
            profile.defense.hit_chance(self.rules.terrain, terrain)
        });
        let damage_taken = self.rules.resistance.get(target_movement).map_or(100, |table| {
            table.damage_taken(&weapon.damage_type)
        });
        let schedule = &self.scenario.schedule;
        let lawful_bonus = schedule.at_turn(turn.round).lawful_bonus;
        let alignment = schedule.combat_modifier(lawful_bonus, striker.unit_type.alignment, false);
        let multiplier = (100 + alignment).saturating_mul(i64::from(damage_taken));
        StrikeModifiers {
            target_hit_chance,
            damage_multiplier: multiplier.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32,
        }
    }

    fn check_leaders(&mut self) {
        let mut leaders = [false; 2];
        for (_, unit) in self.scenario.board.units().filter(|(_, unit)| unit.can_recruit) {
            leaders[unit.side - 1] = true;
        }
        self.state = match leaders {
            [true, true] => self.state,
            [true, false] => MatchState::Won(1),
            [false, true] => MatchState::Won(2),
            [false, false] => MatchState::Draw,
        };
    }
}

fn supports_unit(unit_type: &UnitType) -> bool {
    unit_type.attacks.iter().all(|weapon| {
        weapon
            .specials
            .as_ref()
            .is_none_or(|specials| specials.children.is_empty())
    })
}

fn opposing_side(side: usize) -> usize {
    match side {
        1 => 2,
        _ => 1,
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum MatchError {
    UnsupportedScenario,
    Finished,
    Move(MoveError),
    Attack(AttackError),
    InvalidRecruit,
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
                "fixed match requires two hostile sides with full visibility and base weapons",
            ),
            Self::Finished => f.write_str("match is over"),
            Self::Move(error) => error.fmt(f),
            Self::Attack(error) => error.fmt(f),
            Self::InvalidRecruit => {
                f.write_str("unit type or destination is unavailable for recruitment")
            }
        }
    }
}

impl error::Error for MatchError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{movement_types::read_movement_types, scenario_start, unit_type::UnitType, wml};

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
}
