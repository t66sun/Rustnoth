use crate::{
    board::Battlefield,
    hex::Hex,
    visibility::{HexVisibility, VisibilityMask},
};

/// Owned data that the referee can hand to a single side's policy.
#[derive(Debug, PartialEq, Eq)]
pub struct Observation {
    pub side: usize,
    /// Side numbers that share a team with the viewer, including its own side.
    pub allied_sides: Vec<usize>,
    /// Gold available to the observing side, when supplied by the referee.
    pub gold: Option<i64>,
    pub width: i32,
    pub height: i32,
    pub tiles: Vec<ObservedTile>,
    pub units: Vec<ObservedUnit>,
}

/// A playable hex whose terrain is known to the observing side.
#[derive(Debug, PartialEq, Eq)]
pub struct ObservedTile {
    pub position: Hex,
    pub terrain: String,
    pub visible: bool,
    pub village_owner: Option<usize>,
    /// Scheduled terrain color at this explored location.
    pub time_color: [i32; 3],
}

#[derive(Debug, PartialEq, Eq)]
pub struct ObservedUnit {
    pub position: Hex,
    pub side: usize,
    pub type_id: String,
    pub hitpoints: i32,
    pub experience: i32,
    pub max_experience: i32,
    pub poisoned: bool,
    pub slowed: bool,
    pub petrified: bool,
    pub moves_left: Option<i32>,
    pub attacks_left: Option<i32>,
}

/// Projects referee state using the observing side's visibility mask.
///
/// The referee's `can_see_other_unit` predicate decides concealment for a
/// foreign unit on a visible hex. Own units are always available to the side.
pub fn project(
    board: &Battlefield<'_>,
    side: usize,
    vision: &VisibilityMask,
    can_see_other_unit: impl Fn(Hex) -> bool,
) -> Observation {
    let map = board.map();
    let width = map.width();
    let height = map.height();
    let mut tiles = Vec::new();
    for y in 0..height {
        for x in 0..width {
            let position = Hex { x, y };
            if let Some(visibility @ (HexVisibility::Visible | HexVisibility::Fogged)) =
                vision.at(position)
                && let Some(terrain) = map.terrain_at(position)
            {
                tiles.push(ObservedTile {
                    position,
                    terrain: terrain.to_owned(),
                    visible: visibility == HexVisibility::Visible,
                    village_owner: None,
                    time_color: [0; 3],
                });
            }
        }
    }

    let mut units = Vec::new();
    for (position, unit) in board.units() {
        let own = unit.side == side;
        if own
            || (vision.at(position) == Some(HexVisibility::Visible)
                && can_see_other_unit(position))
        {
            units.push(ObservedUnit {
                position,
                side: unit.side,
                type_id: unit.unit_type.id.clone(),
                hitpoints: unit.hitpoints,
                experience: unit.experience,
                max_experience: unit.max_experience(),
                poisoned: unit.poisoned,
                slowed: unit.slowed,
                petrified: unit.petrified,
                moves_left: own.then_some(unit.movement_left()),
                attacks_left: own.then_some(unit.attacks_left()),
            });
        }
    }

    Observation {
        side,
        allied_sides: vec![side],
        gold: None,
        width,
        height,
        tiles,
        units,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::{map::ScenarioMap, starting_units::read_authored_starting_units, unit_type::UnitType, wml};

    #[test]
    fn projects_only_known_terrain_and_visible_units_with_own_action_points() {
        const OWN: Hex = Hex { x: 0, y: 0 };
        const SEEN: Hex = Hex { x: 1, y: 0 };
        const CONCEALED: Hex = Hex { x: 2, y: 0 };
        const FOGGED: Hex = Hex { x: 3, y: 0 };

        let map = ScenarioMap::parse(
            "Xv,Xv,Xv,Xv,Xv,Xv,Xv\nXv,1 Kh,Gg,Ff,Hh,2 Mm,Xv\nXv,Xv,Xv,Xv,Xv,Xv,Xv",
        )
        .unwrap();
        let config = wml::parse(
            r#"
            [side]
                type=Fighter
                id=own_unit
                experience=11
            [/side]
            [side]
                type=Fighter
                id=shrouded_unit
                [unit]
                    type=Fighter
                    x,y=2,1
                    id=seen_unit
                    experience=19
                    [status]
                        petrified=yes
                    [/status]
                    [modifications]
                        [trait]
                            id=intelligent
                            [effect]
                                apply_to=max_experience
                                increase=-20%
                            [/effect]
                        [/trait]
                    [/modifications]
                [/unit]
                [unit]
                    type=Fighter
                    x,y=3,1
                    id=concealed_unit
                [/unit]
                [unit]
                    type=Fighter
                    x,y=4,1
                    id=fogged_unit
                [/unit]
            [/side]
            "#,
        )
        .unwrap();
        let starts = read_authored_starting_units(&config, &map).unwrap();
        let unit_types = BTreeMap::from([(
            "Fighter".to_owned(),
            UnitType::from_wml(
                wml::parse("id=Fighter\nhitpoints=24\nmovement=5\nattacks=2\nexperience=100").unwrap(),
            ),
        )]);
        let board = Battlefield::from_authored_starting_units(map, starts, &unit_types, 50).unwrap();
        let mut vision = VisibilityMask::new(5, 1, true, true);
        vision.replace_sight([FOGGED]);
        vision.replace_sight([SEEN, CONCEALED]);

        let view = project(&board, 1, &vision, |position| {
            assert!(position == SEEN || position == CONCEALED);
            position == SEEN
        });
        let tiles = view
            .tiles
            .iter()
            .map(|tile| (tile.position, tile.terrain.as_str(), tile.visible))
            .collect::<Vec<_>>();

        assert_eq!((view.side, view.width, view.height), (1, 5, 1));
        assert_eq!(view.gold, None);
        assert_eq!(
            tiles,
            [(SEEN, "Gg", true), (CONCEALED, "Ff", true), (FOGGED, "Hh", false)]
        );
        assert_eq!(
            view.units,
            [
                ObservedUnit {
                    position: OWN,
                    side: 1,
                    type_id: "Fighter".to_owned(),
                    hitpoints: 24,
                    experience: 11,
                    max_experience: 50,
                    poisoned: false,
                    slowed: false,
                    petrified: false,
                    moves_left: Some(5),
                    attacks_left: Some(2),
                },
                ObservedUnit {
                    position: SEEN,
                    side: 2,
                    type_id: "Fighter".to_owned(),
                    hitpoints: 24,
                    experience: 19,
                    max_experience: 40,
                    poisoned: false,
                    slowed: false,
                    petrified: true,
                    moves_left: None,
                    attacks_left: None,
                },
            ]
        );
    }
}
