use std::{collections::BTreeMap, error, fmt, num::NonZeroU32};

use crate::{
    board::{Attack, AttackChoice, AttackError, MoveError, StrikeModifiers, UnitState},
    hex::Hex,
    movement_types::MovementType,
    observation::{Observation, project},
    resistance::DamageTable,
    rng::ReplayRng,
    scenario_start::ScenarioStart,
    terrain_aliases::TerrainAliases,
    turn::Turn,
    visibility::VisibilityMask,
    weapon::WeaponProfile,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Action {
    Move { from: Hex, to: Hex },
    Attack(AttackChoice),
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

/// A headless match on a fixed, fully visible map with two opposing sides.
///
/// This match uses base unit stats and ordinary weapons. It ends when a
/// side loses its last leader or the scenario reaches the round limit.
pub struct FixedMatch<'data> {
    scenario: ScenarioStart<'data>,
    rules: Rules<'data>,
    rng: ReplayRng,
    state: MatchState,
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
            || scenario.board.units().any(|(_, unit)| {
                !matches!(unit.side, 1 | 2)
                    || unit.unit_type.attacks.iter().any(|weapon| {
                        weapon
                            .specials
                            .as_ref()
                            .is_some_and(|specials| !specials.children.is_empty())
                    })
            })
        {
            return Err(MatchError::UnsupportedScenario);
        }
        let mut game = Self {
            state: MatchState::Playing(Turn {
                round: scenario.start_round,
                side: 1,
            }),
            scenario,
            rules,
            rng: ReplayRng::new(seed),
        };
        game.check_leaders();
        if matches!(game.state, MatchState::Playing(turn)
            if game.scenario.last_round.is_some_and(|limit| turn.round > limit))
        {
            game.state = MatchState::Draw;
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
        let observation = project(board, turn.side, &vision, |_| true);
        let mut actions = Vec::new();
        for (from, unit) in board.units().filter(|(_, unit)| unit.side == turn.side) {
            let destinations = board
                .move_destinations(
                    turn.side,
                    from,
                    |terrain| self.walking_cost(unit, terrain),
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
                let unit = board.unit_at(from).ok_or(MoveError::MissingUnit(from))?;
                let profile = self.rules.movement.get(&unit.unit_type.movement_type);
                let terrain_aliases = self.rules.terrain;
                board.move_unit(
                    turn.side,
                    from,
                    to,
                    |terrain| profile?.movement_costs.cost(terrain_aliases, terrain, false),
                    |side| side == opponent,
                )?;
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
            Action::EndTurn => {
                self.state = self
                    .scenario
                    .board
                    .advance_turn(turn, &[1, 2], self.scenario.last_round)
                    .map_or(MatchState::Draw, MatchState::Playing);
            }
        }
        Ok(self.state)
    }

    fn walking_cost(&self, unit: &UnitState<'_>, terrain: &str) -> Option<NonZeroU32> {
        self.rules
            .movement
            .get(&unit.unit_type.movement_type)?
            .movement_costs
            .cost(self.rules.terrain, terrain, false)
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
        let retaliation = defender
            .unit_type
            .attacks
            .iter()
            .enumerate()
            .find(|(_, response)| {
                response.range == weapon.range
                    && (i64::from(response.min_range)..=i64::from(response.max_range))
                        .contains(&distance)
            })
            .map(|(index, response)| {
                (index, self.modifiers(turn, defender, attacker, response, choice.from))
            });
        Ok(Attack {
            from: choice.from,
            to: choice.to,
            weapon: choice.weapon,
            modifiers: self.modifiers(turn, attacker, defender, weapon, choice.to),
            retaliation,
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
        }
    }
}

impl error::Error for MatchError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{movement_types::read_movement_types, scenario_start, unit_type::UnitType, wml};

    #[test]
    fn plays_a_seeded_fixed_scenario_to_a_winner() {
        let scenario = wml::parse(
            r#"
            turns=2
            map_data="Xv,Xv,Xv,Xv,Xv,Xv
Xv,1 Gg,Gg,Gg,2 Gg,Xv
Xv,Xv,Xv,Xv,Xv,Xv"
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
            &wml::parse("[terrain_type]\nid=flat\nstring=Gg\n[/terrain_type]").unwrap(),
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
        play(attack, 1);
        play(Action::EndTurn, 1);
        assert_eq!(play(Action::EndTurn, 2), MatchState::Playing(Turn { round: 2, side: 1 }));
        assert_eq!(play(attack, 1), MatchState::Won(1));
        assert!(game.decision().is_none());
        assert_eq!(game.step(Action::EndTurn), Err(MatchError::Finished));
    }
}
