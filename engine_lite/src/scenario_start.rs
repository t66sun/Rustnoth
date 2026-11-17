use std::{collections::BTreeMap, error, fmt, io};

use crate::{
    board::{Battlefield, BoardError},
    scenario_map::{self, ScenarioMapError},
    scenario_sides::{SideConfig, read_sides},
    scenario_time::{integer, read_time_schedule},
    starting_units::{PlacementError, read_authored_starting_units},
    time_of_day::TimeSchedule,
    unit_type::UnitType,
    wml::Node,
};

/// Initial referee state for a fixed, preprocessed scenario.
#[derive(Debug)]
pub struct ScenarioStart<'rules> {
    pub board: Battlefield<'rules>,
    pub sides: Vec<SideConfig>,
    pub schedule: TimeSchedule,
    pub start_round: i32,
    pub last_round: Option<i32>,
}

/// Loads the scenario map, sides, placed units, and global time schedule.
pub fn load<'rules>(
    scenario: &Node,
    unit_types: &'rules BTreeMap<String, UnitType>,
    read_map_file: impl FnOnce(&str) -> io::Result<String>,
) -> Result<ScenarioStart<'rules>, ScenarioStartError> {
    let map = scenario_map::load(scenario, read_map_file).map_err(ScenarioStartError::Map)?;
    let units = read_authored_starting_units(scenario, &map)
        .map_err(ScenarioStartError::Placement)?;
    let board = Battlefield::from_authored_starting_units(map, units, unit_types)
        .map_err(ScenarioStartError::Board)?;

    Ok(ScenarioStart {
        board,
        sides: read_sides(scenario),
        schedule: read_time_schedule(scenario),
        start_round: integer(scenario, "turn_at").unwrap_or(1),
        last_round: integer(scenario, "turns").filter(|&turns| turns != -1),
    })
}

#[derive(Debug)]
pub enum ScenarioStartError {
    Map(ScenarioMapError),
    Placement(PlacementError),
    Board(BoardError),
}

impl fmt::Display for ScenarioStartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Map(source) => fmt::Display::fmt(source, f),
            Self::Placement(source) => fmt::Display::fmt(source, f),
            Self::Board(source) => fmt::Display::fmt(source, f),
        }
    }
}

impl error::Error for ScenarioStartError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::Map(source) => Some(source),
            Self::Placement(source) => Some(source),
            Self::Board(source) => Some(source),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{hex::Hex, wml};

    const MAP: &str = "Xv,Xv,Xv,Xv\nXv,1 Kh,2 Kh,Xv\nXv,Xv,Xv,Xv";

    #[test]
    fn starts_a_fixed_scenario_with_its_authored_units_and_time() {
        let scenario = wml::parse(
            r#"
            map_file=fixed.map
            turn_at=" 7 saved"
            turns=11
            current_time=1
            [time]
                id=morning
                lawful_bonus=25
            [/time]
            [time]
                id=night
                lawful_bonus=-25
            [/time]
            [side]
                side=1
                type=Fighter
                gold=120
            [/side]
            [side]
                side=2
                type=Fighter
            [/side]
            "#,
        )
        .unwrap();
        let fighter = UnitType::from_wml(wml::parse("id=Fighter\nhitpoints=28").unwrap());
        let unit_types = BTreeMap::from([(fighter.id.clone(), fighter)]);

        let start = load(&scenario, &unit_types, |file| {
            assert_eq!(file, "fixed.map");
            Ok(MAP.to_owned())
        })
        .unwrap();

        assert_eq!((start.start_round, start.last_round), (7, Some(11)));
        assert_eq!(start.schedule.at_turn(start.start_round).id, "night");
        assert_eq!(start.schedule.at_turn(start.start_round + 1).id, "morning");
        assert_eq!(start.sides.len(), 2);
        assert_eq!(start.sides[0].starting_gold, 120);
        let second = start.board.unit_at(Hex { x: 1, y: 0 }).unwrap();
        assert_eq!((second.side, second.hitpoints), (2, 28));
    }

    #[test]
    fn starting_units_use_authored_current_hitpoints_with_type_as_the_default() {
        let scenario = wml::parse(
            r#"
            map_data="Xv,Xv,Xv,Xv,Xv,Xv
Xv,1 Kh,Gg,Gg,Gg,Xv
Xv,Xv,Xv,Xv,Xv,Xv"
            [side]
                type=Fighter
                hitpoints=3
                [leader]
                    type=Fighter
                    x=2
                    y=1
                    hitpoints=32
                [/leader]
                [unit]
                    type=Fighter
                    x=3
                    y=1
                    hitpoints=-2
                [/unit]
                [unit]
                    type=Fighter
                    x=4
                    y=1
                [/unit]
            [/side]
            "#,
        )
        .unwrap();
        let fighter = UnitType::from_wml(wml::parse("id=Fighter\nhitpoints=28").unwrap());
        let unit_types = BTreeMap::from([(fighter.id.clone(), fighter)]);
        let start = load(&scenario, &unit_types, |_| panic!("inline map requested a file"))
            .unwrap();

        for (hex, expected) in [3, 32, -2, 28].into_iter().enumerate() {
            let unit = start.board.unit_at(Hex { x: hex as i32, y: 0 }).unwrap();
            assert_eq!((unit.hitpoints, unit.unit_type.hitpoints), (expected, 28));
        }
    }

    #[test]
    fn starting_units_use_authored_action_counters_with_a_zero_floor() {
        let scenario = wml::parse(
            r#"
            map_data="Xv,Xv,Xv,Xv,Xv,Xv
Xv,1 Kh,Gg,Gg,Gg,Xv
Xv,Xv,Xv,Xv,Xv,Xv"
            [side]
                type=Fighter
                moves=1
                attacks_left=0
                [leader]
                    type=Fighter
                    x,y=2,1
                    moves=7
                    attacks_left=3
                [/leader]
                [unit]
                    type=Fighter
                    x,y=3,1
                    moves=-3
                    attacks_left=-1
                [/unit]
                [unit]
                    type=Fighter
                    x,y=4,1
                    attacks_left=invalid
                [/unit]
            [/side]
            "#,
        )
        .unwrap();
        let fighter = UnitType::from_wml(
            wml::parse("id=Fighter\nmovement=4\nattacks=2").unwrap(),
        );
        let unit_types = BTreeMap::from([(fighter.id.clone(), fighter)]);
        let start = load(&scenario, &unit_types, |_| panic!("inline map requested a file"))
            .unwrap();

        for (x, expected) in [(0, (1, 0)), (1, (7, 3)), (2, (0, 0)), (3, (4, 2))] {
            let unit = start.board.unit_at(Hex { x, y: 0 }).unwrap();
            assert_eq!((unit.moves_left, unit.attacks_left), expected);
        }
    }

    #[test]
    fn defaults_to_round_one_without_a_round_limit() {
        for turns in ["", "turns=-1\n"] {
            let scenario = wml::parse(format!("{turns}map_data=\"{MAP}\"")).unwrap();
            let unit_types = BTreeMap::new();
            let start = load(&scenario, &unit_types, |_| panic!("inline map requested a file"))
                .unwrap();

            assert_eq!((start.start_round, start.last_round), (1, None));
        }
    }
}
