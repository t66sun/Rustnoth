//! Referee snapshots for replay audits and differential tests.

use std::mem;

use crate::{hex::Hex, rng::ReplayRng};

use super::{Action, FixedMatch, MatchError, MatchState};

/// Match state recorded by the referee at an action boundary.
#[derive(Debug, Eq, PartialEq)]
pub struct Snapshot {
    pub state: MatchState,
    /// Gold in ascending side order, starting with side 1.
    pub gold: Vec<i64>,
    /// Units in hex order.
    pub units: Vec<SnapshotUnit>,
    /// Owned village hexes and their owners in hex order.
    pub villages: Vec<(Hex, usize)>,
}

/// Comparable values for a unit on the referee board.
#[derive(Debug, Eq, PartialEq)]
pub struct SnapshotUnit {
    pub position: Hex,
    pub side: usize,
    pub type_id: String,
    pub hitpoints: i32,
    pub moves_left: i32,
    pub attacks_left: i32,
    pub can_recruit: bool,
    pub experience: i32,
    pub slowed: bool,
    pub poisoned: bool,
    /// Applied trait IDs in effect order.
    pub traits: Vec<String>,
}

impl FixedMatch<'_> {
    /// Executes a referee audit step with a temporary stream at the given checkpoint.
    /// Returns the number of draws made by the normal action path.
    pub(crate) fn step_with_rng_checkpoint(
        &mut self,
        action: Action,
        seed: u32,
        calls: u32,
    ) -> Result<(MatchState, u32), MatchError> {
        let original = mem::replace(&mut self.rng, ReplayRng::restore(seed, calls));
        let result = self.step(action);
        let (_, after) = self.rng.seed_and_calls();
        self.rng = original;
        result.map(|state| (state, after.wrapping_sub(calls)))
    }

    /// Records all sides for referee use, including after the match ends.
    pub fn audit_snapshot(&self) -> Snapshot {
        Snapshot {
            state: self.state(),
            gold: self.gold.clone(),
            units: self
                .scenario
                .board
                .units()
                .map(|(position, unit)| SnapshotUnit {
                    position,
                    side: unit.side,
                    type_id: unit.unit_type.id.clone(),
                    hitpoints: unit.hitpoints,
                    moves_left: unit.moves_left,
                    attacks_left: unit.attacks_left,
                    can_recruit: unit.can_recruit,
                    experience: unit.experience,
                    slowed: unit.slowed,
                    poisoned: unit.poisoned,
                    traits: unit
                        .traits()
                        .map(|trait_| trait_.attribute("id").unwrap_or_default().to_owned())
                        .collect(),
                })
                .collect(),
            villages: self
                .village_owners
                .iter()
                .map(|(&hex, &side)| (hex, side))
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::{
        board::{AttackChoice, AttackError},
        fixed_match::Rules,
        movement_types::read_movement_types,
        scenario_bundle::Bundle,
        scenario_start,
        terrain_aliases::TerrainAliases,
        turn::Turn,
        unit_type::UnitType,
        wml,
    };

    use super::*;

    #[test]
    fn scoped_rng_uses_the_given_draw_offset_and_restores_the_normal_stream() {
        let bundle = Bundle::parse(
            br#"
            [terrain_type]
                id=flat
                string=Gg
            [/terrain_type]
            [units]
                [movetype]
                    name=foot
                    [defense]
                        flat=50
                    [/defense]
                [/movetype]
                [unit_type]
                    id=Fighter
                    hitpoints=10
                    movement_type=foot
                    [attack]
                        name=sword
                        type=blade
                        range=melee
                        damage=3
                        number=1
                        [specials]
                            [slow]
                                id=slow
                            [/slow]
                            [poison]
                                id=poison
                            [/poison]
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
        let mut game = bundle.start(22, |_| unreachable!()).unwrap();
        game.rng.next_u32();
        let attack = AttackChoice {
            from: Hex { x: 0, y: 0 },
            to: Hex { x: 1, y: 0 },
            weapon: 0,
        };

        let before = game.audit_snapshot();
        let invalid = Action::Attack(AttackChoice { weapon: 1, ..attack });
        assert_eq!(
            game.step_with_rng_checkpoint(invalid, 5489, 3),
            Err(MatchError::Attack(AttackError::UnavailableWeapon))
        );
        assert_eq!(game.rng.seed_and_calls(), (22, 1));
        assert_eq!(game.audit_snapshot(), before);

        assert_eq!(
            game.step_with_rng_checkpoint(Action::Attack(attack), 5489, 3),
            Ok((before.state, 2))
        );
        // At this offset the first roll is 85 (miss) and the second is 4 (hit).
        let after = game.audit_snapshot();
        assert_eq!(after.units[0].hitpoints, 7);
        assert_eq!(after.units[1].hitpoints, 10);
        assert!(after.units[0].slowed);
        assert!(!after.units[1].slowed);
        assert!(after.units[0].poisoned);
        assert!(!after.units[1].poisoned);
        assert_eq!(game.rng.next_u32(), ReplayRng::restore(22, 1).next_u32());
    }

    #[test]
    fn captures_both_sides_before_and_after_an_action_and_at_game_end() {
        let config = wml::parse(
            r#"
            turns=1
            map_data="Xv,Xv,Xv,Xv,Xv
Xv,1 Gg,Vh,2 Gg,Xv
Xv,Xv,Xv,Xv,Xv"
            [side]
                side=1
                team_name=west
                type=Fighter
                gold=17
            [/side]
            [side]
                side=2
                team_name=east
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
            wml::parse("id=Fighter\nhitpoints=10\nmovement=3\nmovement_type=foot").unwrap(),
        );
        let unit_types = BTreeMap::from([(fighter.id.clone(), fighter)]);
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
        let rules = Rules {
            terrain: &terrain,
            movement: &movement,
            resistance: &resistance,
        };
        let start = scenario_start::load(&config, &unit_types, |_| unreachable!()).unwrap();
        let mut game = FixedMatch::new(start, rules, 5489).unwrap();
        let west = Hex { x: 0, y: 0 };
        let village = Hex { x: 1, y: 0 };
        let east = Hex { x: 2, y: 0 };

        let before = game.audit_snapshot();
        let fighter = |position, side| SnapshotUnit {
            position,
            side,
            type_id: "Fighter".to_owned(),
            hitpoints: 10,
            moves_left: 3,
            attacks_left: 1,
            can_recruit: true,
            experience: 0,
            slowed: false,
            poisoned: false,
            traits: Vec::new(),
        };
        assert_eq!(before.state, MatchState::Playing(Turn { round: 1, side: 1 }));
        assert_eq!(before.gold, [17, 41]);
        assert_eq!(before.units, [fighter(west, 1), fighter(east, 2)]);
        assert_eq!(before.villages, [(village, 2)]);
        let (observation, _) = game.decision().unwrap();
        assert_eq!(observation.gold, Some(17));

        game.step(Action::Move { from: west, to: village }).unwrap();
        let after = game.audit_snapshot();
        assert_eq!(after.state, before.state);
        assert_eq!(after.gold, before.gold);
        assert_eq!(
            after.units,
            [SnapshotUnit { moves_left: 0, ..fighter(village, 1) }, fighter(east, 2)]
        );
        assert_eq!(after.villages, [(village, 1)]);

        game.step(Action::EndTurn).unwrap();
        game.step(Action::EndTurn).unwrap();
        assert!(game.decision().is_none());
        assert_eq!(
            game.audit_snapshot(),
            Snapshot { state: MatchState::Draw, ..after }
        );
    }
}
