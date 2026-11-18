//! Build a skirmish from a raw multiplayer map and its chosen factions.

use std::error::Error;

use crate::{
    map::ScenarioMap,
    starting_units::read_starting_units,
    wml::{self, Node, Value},
};

/// Map visibility chosen for all sides in a skirmish.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Visibility {
    #[default]
    Open,
    Fog,
    Shroud,
    FogAndShroud,
}

impl std::str::FromStr for Visibility {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "open" => Ok(Self::Open),
            "fog" => Ok(Self::Fog),
            "shroud" => Ok(Self::Shroud),
            "both" => Ok(Self::FogAndShroud),
            _ => Err("visibility must be open, fog, shroud, or both"),
        }
    }
}

#[derive(Debug)]
pub struct Faction {
    pub id: String,
    pub leaders: Vec<String>,
    pub recruits: Vec<String>,
}

impl Faction {
    /// Read a single Default-era faction file from data/multiplayer/factions.
    pub fn parse(wml: &[u8]) -> Result<Self, Box<dyn Error>> {
        let root = wml::parse(wml)?;
        let mut factions = root.children.iter().filter(|(tag, _)| tag == "multiplayer_side");
        let (_, faction) = factions.next().ok_or("missing [multiplayer_side]")?;
        if factions.next().is_some() {
            return Err("expected one [multiplayer_side] per faction file".into());
        }
        let id = faction.attribute("id")
            .filter(|s| !s.is_empty())
            .ok_or("missing faction id")?;
        let list = |field| -> Result<Vec<String>, Box<dyn Error>> {
            let values: Vec<_> = faction.attribute(field).unwrap_or_default()
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect();
            if values.is_empty() {
                return Err(format!("faction {id} has no {field} entries").into());
            }
            Ok(values)
        };
        Ok(Self { id: id.to_owned(), leaders: list("leader")?, recruits: list("recruit")? })
    }
}

/// Return a complete WML document with an embedded map and sides in numbered start order.
/// Each side uses its chosen leader, or the first leader listed in its faction.
pub fn scenario(
    name: &str,
    map_data: &str,
    turns: i32,
    sides: &[(&Faction, Option<&str>)],
) -> Result<Node, Box<dyn Error>> {
    scenario_with_visibility(name, map_data, turns, sides, Visibility::Open)
}

/// Build the same skirmish with the chosen fog and shroud settings on every side.
pub fn scenario_with_visibility(
    name: &str,
    map_data: &str,
    turns: i32,
    sides: &[(&Faction, Option<&str>)],
    visibility: Visibility,
) -> Result<Node, Box<dyn Error>> {
    if sides.len() < 2 || turns < 1 {
        return Err("a skirmish requires at least two factions and a positive turn limit".into());
    }
    let map = ScenarioMap::parse(map_data)?;
    let turns = turns.to_string();
    let mut scenario = attributes(&[
        ("id", "rustnoth_skirmish"), ("name", name), ("map_data", map_data), ("turns", &turns),
    ]);

    // Wesnoth 1.18.8 DEFAULT_SCHEDULE, starting at dawn.
    for (id, bonus) in [
        ("dawn", "0"), ("morning", "25"), ("afternoon", "25"),
        ("dusk", "0"), ("first_watch", "-25"), ("second_watch", "-25"),
    ] {
        scenario.children.push(("time".into(), attributes(&[("id", id), ("lawful_bonus", bonus)])));
    }
    let (fog, shroud) = match visibility {
        Visibility::Open => ("no", "no"),
        Visibility::Fog => ("yes", "no"),
        Visibility::Shroud => ("no", "yes"),
        Visibility::FogAndShroud => ("yes", "yes"),
    };
    for (index, &(faction, selected_leader)) in sides.iter().enumerate() {
        let number = (index + 1).to_string();
        let leader = match selected_leader {
            Some(id) if faction.leaders.iter().any(|listed| listed == id) => id,
            Some(id) => return Err(format!("{} has no declared leader {id}", faction.id).into()),
            None => faction.leaders.first().ok_or("faction has no declared leaders")?,
        };
        let recruits = faction.recruits.join(",");
        let side = attributes(&[
            ("side", &number), ("team_name", &number), ("faction", &faction.id),
            ("type", leader), ("recruit", &recruits), ("controller", "human"),
            ("random_traits", "no"), ("fog", fog), ("shroud", shroud),
        ]);
        scenario.children.push(("side".into(), side));
    }
    read_starting_units(&scenario, &map)?;
    Ok(Node { children: vec![("scenario".into(), scenario)], ..Node::default() })
}

fn attributes(fields: &[(&str, &str)]) -> Node {
    Node {
        attributes: fields.iter().map(|&(key, text)| {
            (key.to_owned(), Value { text: text.to_owned(), ..Value::default() })
        }).collect(),
        ..Node::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{scenario_sides::read_sides, scenario_time::read_time_schedule, wml_emit};

    const MAP: &str = "Xv,Xv,Xv,Xv,Xv\nXv,2 Kh,3 Kh,1 Kh,Xv\nXv,Xv,Xv,Xv,Xv";

    fn faction() -> Faction {
        Faction::parse(br#"#textdomain wesnoth-multiplayer
            [multiplayer_side]
                id=Loyalists
                name= _"Loyalists"
                type=random
                leader=Lieutenant,White Mage
                random_leader=Lieutenant
                recruit=Spearman,Heavy Infantryman
            [/multiplayer_side]"#).unwrap()
    }

    #[test]
    fn skirmish_round_trips_through_standard_map_roster_and_schedule_readers() {
        let faction = faction();
        let sides = [(&faction, Some("White Mage")), (&faction, None), (&faction, None)];
        let root = scenario("River", MAP, 25, &sides).unwrap();
        let mut encoded = Vec::new();
        wml_emit::write_config(&mut encoded, &root).unwrap();
        let decoded = wml::parse(encoded).unwrap();
        let [(tag, scenario)] = decoded.children.as_slice() else { panic!("expected one scenario") };
        assert_eq!(tag, "scenario");
        assert_eq!(scenario.attribute("turns"), Some("25"));
        assert_eq!(scenario.attribute("map_data"), Some(MAP));
        let map = ScenarioMap::parse(MAP).unwrap();
        let starts = read_starting_units(scenario, &map).unwrap();
        let leaders: Vec<_> = starts.iter()
            .map(|u| (u.side, u.position.x, u.type_id.as_str()))
            .collect();
        assert_eq!(leaders, [(1, 2, "White Mage"), (2, 0, "Lieutenant"), (3, 1, "Lieutenant")]);
        let sides = read_sides(scenario);
        assert!(sides.iter().all(|side| {
            !side.fog && !side.shroud && side.recruits.contains("Heavy Infantryman")
        }));
        assert!(sides[0].is_enemy(&sides[1]));
        let schedule = read_time_schedule(scenario);
        let bonuses: Vec<_> = (1..=7).map(|turn| schedule.at_turn(turn).lawful_bonus).collect();
        assert_eq!(bonuses, [0, 25, 25, 0, -25, -25, 0]);
    }

    #[test]
    fn every_side_uses_the_selected_skirmish_visibility() {
        let faction = faction();
        let sides = [(&faction, None); 3];
        for (name, fog, shroud) in [
            ("open", false, false), ("fog", true, false),
            ("shroud", false, true), ("both", true, true),
        ] {
            let config = scenario_with_visibility("River", MAP, 25, &sides, name.parse().unwrap()).unwrap();
            let scenario = &config.children[0].1;
            for (_, side) in scenario.children.iter().filter(|(tag, _)| tag == "side") {
                assert_eq!(side.attribute("fog"), Some(if fog { "yes" } else { "no" }));
                assert_eq!(side.attribute("shroud"), Some(if shroud { "yes" } else { "no" }));
            }
            let roster = read_sides(scenario);
            assert_eq!(roster.len(), 3);
            assert!(roster.iter().all(|side| (side.fog, side.shroud) == (fog, shroud)), "{name}");
        }
        assert!("mist".parse::<Visibility>().is_err());
    }

    #[test]
    fn rejects_unlisted_leader_and_side_without_playable_start() {
        let faction = faction();
        assert!(scenario("River", MAP, 5, &[(&faction, Some("Mage")), (&faction, None)]).is_err());
        assert!(scenario("River", MAP, 5, &[(&faction, None); 4]).is_err());
    }
}
