//! Referee snapshots for replay audits and differential tests.

use crate::hex::Hex;

use super::{FixedMatch, MatchState};

/// Match state recorded by the referee at an action boundary.
#[derive(Debug, Eq, PartialEq)]
pub struct Snapshot {
    pub state: MatchState,
    /// Gold for sides 1 and 2 in that order.
    pub gold: [i64; 2],
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
}

impl FixedMatch<'_> {
    /// Records both sides for referee use, including after the match ends.
    pub fn audit_snapshot(&self) -> Snapshot {
        Snapshot {
            state: self.state,
            gold: self.gold,
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
        fixed_match::{Action, Rules},
        movement_types::read_movement_types,
        scenario_start,
        terrain_aliases::TerrainAliases,
        turn::Turn,
        unit_type::UnitType,
        wml,
    };

    use super::*;

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
