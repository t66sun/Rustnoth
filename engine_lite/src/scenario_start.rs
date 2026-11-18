use std::{collections::BTreeMap, error, fmt, io};

use crate::{
    board::{Battlefield, BoardError, DEFAULT_EXPERIENCE_MODIFIER},
    scenario_events::ScenarioEvents,
    scenario_map::{self, ScenarioMapError},
    scenario_sides::{SideConfig, read_sides},
    scenario_time::{integer, read_map_schedule},
    starting_units::{PlacementError, read_authored_starting_units},
    time_of_day::TimeSchedule,
    unit_type::UnitType,
    wml::Node,
};

/// Initial referee state for a fixed, preprocessed scenario.
#[derive(Debug)]
pub struct ScenarioStart<'rules> {
    pub board: Battlefield<'rules>,
    pub(crate) events: ScenarioEvents,
    pub sides: Vec<SideConfig>,
    pub schedule: TimeSchedule,
    pub start_round: i32,
    pub last_round: Option<i32>,
}

/// Loads the scenario map, sides, placed units, and time schedules.
pub fn load<'rules>(
    scenario: &Node,
    unit_types: &'rules BTreeMap<String, UnitType>,
    read_map_file: impl FnOnce(&str) -> io::Result<String>,
) -> Result<ScenarioStart<'rules>, ScenarioStartError> {
    let map = scenario_map::load(scenario, read_map_file).map_err(ScenarioStartError::Map)?;
    let schedule = read_map_schedule(scenario, &map);
    let units = read_authored_starting_units(scenario, &map)
        .map_err(ScenarioStartError::Placement)?;
    let experience_modifier = integer(scenario, "experience_modifier")
        .unwrap_or(DEFAULT_EXPERIENCE_MODIFIER);
    let board = Battlefield::from_authored_starting_units(map, units, unit_types, experience_modifier)
        .map_err(ScenarioStartError::Board)?;

    let sides = read_sides(scenario);
    Ok(ScenarioStart {
        board,
        events: ScenarioEvents::read(scenario, sides.len()),
        sides,
        schedule,
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
    fn explicit_unit_variations_apply_on_the_map_and_in_reserve() {
        let fighter = UnitType::from_wml(wml::parse("id=Fighter\nhitpoints=12\n[variation]\nvariation_id=mounted\ninherit=yes\nhitpoints=31\nmovement=8\n[/variation]").unwrap());
        let units = BTreeMap::from([(fighter.id.clone(), fighter)]);
        let scenario = wml::parse("[side]\ntype=Fighter\nvariation=mounted\n[unit]\nplacement=recall\ntype=Fighter\nvariation=mounted\n[/unit]\n[/side]\n\
            [side]\ntype=Fighter\nvariation=unknown\n[/side]\nmap_file=fixed.map").unwrap();
        let start = load(&scenario, &units, |_| Ok(MAP.into())).unwrap();
        let placed = start.board.unit_at(Hex { x: 0, y: 0 }).unwrap();
        assert_eq!((placed.hitpoints, placed.max_movement()), (31, 8));
        assert_eq!(start.board.recall_roster(1)[0].max_hitpoints(), 31);
        assert_eq!(start.board.unit_at(Hex { x: 1, y: 0 }).unwrap().hitpoints, 12);
    }

    #[test]
    fn reserves_keep_authored_state_and_belong_only_to_their_side() {
        let scenario = wml::parse(r#"
            map_file=fixed.map
            [side]
                type=Fighter
                persistent=no
                recall_cost=23
                [unit]
                    id=veteran
                    type=Fighter
                    placement=recall
                    x,y=1,1
                    canrecruit=yes
                    hitpoints=11
                    experience=17
                    recall_cost=6
                    [modifications]
                        [trait]
                            id=sturdy
                            [effect]
                                apply_to=hitpoints
                                increase_total=4
                            [/effect]
                        [/trait]
                    [/modifications]
                [/unit]
                [unit]
                    type=Fighter
                    x,y=recall,recall
                    recall_cost=-1
                [/unit]
            [/side]
            [side]
                persistent=yes
                [unit]
                    id=veteran
                    type=Fighter
                    placement=recall
                [/unit]
            [/side]
        "#).unwrap();
        let fighter = UnitType::from_wml(wml::parse("id=Fighter\nhitpoints=28\nrecall_cost=14").unwrap());
        let types = BTreeMap::from([(fighter.id.clone(), fighter)]);
        let start = load(&scenario, &types, |_| Ok(MAP.to_owned())).unwrap();
        let side_cost = start.sides[0].recall_cost;
        assert_eq!(start.board.units().count(), 1);
        let leader = start.board.unit_at(Hex { x: 0, y: 0 }).unwrap();
        assert!(leader.can_recruit);
        assert_eq!(leader.id, None);
        assert_eq!(leader.recall_cost(side_cost), 14);
        let first = start.board.recall_roster(1);
        assert_eq!(first.len(), 2);
        let veteran = &first[0];
        assert_eq!((veteran.side, veteran.id.as_deref()), (1, Some("veteran")));
        assert_eq!(veteran.unit_type.id, "Fighter");
        assert_eq!((veteran.hitpoints, veteran.max_hitpoints(), veteran.experience), (11, 32, 17));
        assert!(veteran.can_recruit);
        assert_eq!(veteran.traits().next().unwrap().attribute("id"), Some("sturdy"));
        assert_eq!(veteran.recall_cost(side_cost), 6);
        assert_eq!(first[1].id, None);
        assert_eq!(first[1].recall_cost(side_cost), 23);
        let other = start.board.recall_roster(2);
        assert_eq!(other.len(), 1);
        assert_eq!((other[0].side, other[0].id.as_deref()), (2, Some("veteran")));
        assert_eq!(other[0].recall_cost(20), 14);
        assert!(start.board.recall_roster(3).is_empty());
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
    fn loads_each_starting_units_own_poison_and_slow_status() {
        let scenario = wml::parse(
            r#"
            map_data="Xv,Xv,Xv,Xv,Xv,Xv,Xv
Xv,1 Kh,Gg,Gg,Gg,Gg,Xv
Xv,Xv,Xv,Xv,Xv,Xv,Xv"
            [side]
                type=Fighter
                [status]
                    poisoned=yes
                    slowed=false
                [/status]
                [leader]
                    type=Fighter
                    x,y=2,1
                    [status]
                        poisoned=no
                        slowed=true
                    [/status]
                [/leader]
                [unit]
                    type=Fighter
                    x,y=3,1
                    [status]
                        poisoned=true
                        slowed=yes
                    [/status]
                [/unit]
                [unit]
                    type=Fighter
                    x,y=4,1
                [/unit]
                [unit]
                    type=Fighter
                    x,y=5,1
                    [status]
                        poisoned=1
                        slowed=_ "yes"
                    [/status]
                [/unit]
            [/side]
            "#,
        )
        .unwrap();
        let fighter = UnitType::from_wml(wml::parse("id=Fighter").unwrap());
        let types = BTreeMap::from([(fighter.id.clone(), fighter)]);
        let start = load(&scenario, &types, |_| unreachable!()).unwrap();
        let states: Vec<_> = start.board.units()
            .map(|(_, unit)| (unit.poisoned, unit.slowed))
            .collect();
        assert_eq!(states, [
            (true, false), (false, true), (true, true), (false, false), (false, false),
        ]);
    }

    #[test]
    fn preserves_authored_starting_experience_and_defaults_to_zero() {
        let scenario = wml::parse(
            r#"
            map_data="Xv,Xv,Xv,Xv,Xv,Xv
Xv,1 Kh,Gg,Gg,Gg,Xv
Xv,Xv,Xv,Xv,Xv,Xv"
            [side]
                type=Fighter
                experience=7
                [leader]
                    type=Fighter
                    x,y=2,1
                    experience=57
                [/leader]
                [unit]
                    type=Fighter
                    x,y=3,1
                    experience=-3
                [/unit]
                [unit]
                    type=Fighter
                    x,y=4,1
                [/unit]
            [/side]
            "#,
        )
        .unwrap();
        let fighter = UnitType::from_wml(wml::parse("id=Fighter\nexperience=40").unwrap());
        let unit_types = BTreeMap::from([(fighter.id.clone(), fighter)]);
        let start = load(&scenario, &unit_types, |_| panic!("inline map requested a file"))
            .unwrap();

        for (x, experience) in [(0, 7), (1, 57), (2, -3), (3, 0)] {
            let unit = start.board.unit_at(Hex { x, y: 0 }).unwrap();
            assert_eq!((unit.experience, unit.unit_type.experience), (experience, 40));
            assert_eq!(unit.max_experience(), 40);
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
