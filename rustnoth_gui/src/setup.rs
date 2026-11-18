use std::{error::Error, fs, path::{Path, PathBuf}};

use engine_lite::{map::ScenarioMap, skirmish::{self, Faction, Visibility}, wml_emit};
use macroquad::prelude::*;

use crate::{read_file, show_board, BoardData};

mod view;

// The final position of the original 1..=100 turns control means unlimited play.
const UNLIMITED_INDEX: usize = 99;

fn round_limit(index: usize) -> i32 {
    if index == UNLIMITED_INDEX { -1 } else { index as i32 + 1 }
}
const VISIBILITY_OPTIONS: [(Visibility, &str); 4] = [
    (Visibility::Open, "Open map"),
    (Visibility::Fog, "Fog of war"),
    (Visibility::Shroud, "Shroud"),
    (Visibility::FogAndShroud, "Fog and shroud"),
];

struct MapEntry {
    name: String,
    board: Result<(String, usize), String>,
}

impl MapEntry {
    fn read(path: &Path) -> Self {
        let name = path.file_stem().unwrap_or_default().to_string_lossy().replace('_', " ");
        let board = fs::read_to_string(path).map_err(|e| e.to_string()).and_then(|data| {
            let map = ScenarioMap::parse(&data).map_err(|e| e.to_string())?;
            let mut starts: Vec<_> = map.numbered_starts().collect();
            starts.sort_unstable_by_key(|&(side, _)| side);
            if starts.len() < 2 || starts.iter().enumerate().any(|(i, &(side, hex))| {
                side != i + 1 || !map.is_playable(hex)
            }) {
                return Err("Choose a map with consecutive playable starts beginning at side 1.".into());
            }
            Ok((data, starts.len()))
        });
        Self { name, board }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Field { Map, Side, Team, Faction, Leader, Visibility, Rounds }

impl Field {
    const ALL: [Self; 7] = [Self::Map, Self::Side, Self::Team, Self::Faction, Self::Leader, Self::Visibility, Self::Rounds];

    fn title(self) -> &'static str {
        match self {
            Self::Map => "Map",
            Self::Side => "Side",
            Self::Team => "Team",
            Self::Faction => "Faction",
            Self::Leader => "Leader",
            Self::Visibility => "Visibility",
            Self::Rounds => "Rounds",
        }
    }
}

#[derive(Clone, Copy)]
struct Side {
    team: usize,
    faction: usize,
    leader: usize,
}

pub(super) struct Setup {
    root: PathBuf,
    core: Vec<u8>,
    seed: u32,
    maps: Vec<MapEntry>,
    factions: Vec<Faction>,
    map: usize,
    side: usize,
    sides: Vec<Side>,
    visibility: usize,
    rounds_index: usize,
}

impl Setup {
    pub(super) fn load(root: PathBuf, core: Vec<u8>, seed: u32) -> Result<Self, Box<dyn Error>> {
        let directory = root.join("data/multiplayer");
        let paths = |name: &str, suffix: &str| -> Result<Vec<PathBuf>, Box<dyn Error>> {
            let dir = directory.join(name);
            let mut paths = Vec::new();
            for entry in fs::read_dir(&dir).map_err(|e| format!("{} ({e})", dir.display()))? {
                let path = entry?.path();
                if path.file_name().is_some_and(|s| s.to_string_lossy().ends_with(suffix)) {
                    paths.push(path);
                }
            }
            paths.sort();
            if paths.is_empty() { return Err(format!("No *{suffix} files in {}", dir.display()).into()); }
            Ok(paths)
        };
        let maps: Vec<_> = paths("maps", ".map")?.iter().map(|path| MapEntry::read(path)).collect();
        let mut factions = Vec::new();
        for path in paths("factions", "-default.cfg")? {
            let faction = Faction::parse(&read_file(&path)?).map_err(|e| format!("{} ({e})", path.display()))?;
            factions.push(faction);
        }
        let first = maps.iter().position(|map| map.name == "2p Hamlets" && map.board.is_ok())
            .or_else(|| maps.iter().position(|map| map.board.is_ok())).unwrap_or(0);
        let mut setup = Self { root, core, seed, maps, factions, map: first, side: 0, sides: Vec::new(), visibility: 0, rounds_index: 29 };
        setup.select(Field::Map, first);
        Ok(setup)
    }

    fn count(&self, field: Field) -> usize {
        match field {
            Field::Map => self.maps.len(),
            Field::Side | Field::Team => self.sides.len(),
            Field::Visibility => VISIBILITY_OPTIONS.len(),
            Field::Rounds => UNLIMITED_INDEX + 1,
            Field::Faction if !self.sides.is_empty() => self.factions.len(),
            Field::Leader if !self.sides.is_empty() => self.factions[self.sides[self.side].faction].leaders.len(),
            _ => 0,
        }
    }

    fn selected(&self, field: Field) -> usize {
        match field {
            Field::Map => self.map,
            Field::Side => self.side,
            Field::Team => self.sides[self.side].team,
            Field::Faction => self.sides[self.side].faction,
            Field::Leader => self.sides[self.side].leader,
            Field::Visibility => self.visibility,
            Field::Rounds => self.rounds_index,
        }
    }

    fn label(&self, field: Field, index: usize) -> String {
        match field {
            Field::Map => self.maps[index].name.clone(),
            Field::Side => format!("Side {} of {}", index + 1, self.sides.len()),
            Field::Team => format!("Team {}", index + 1),
            Field::Faction => self.factions[index].id.clone(),
            Field::Leader => self.factions[self.sides[self.side].faction].leaders[index].clone(),
            Field::Visibility => VISIBILITY_OPTIONS[index].1.to_owned(),
            Field::Rounds if index == UNLIMITED_INDEX => "Unlimited".into(),
            Field::Rounds => round_limit(index).to_string(),
        }
    }

    fn select(&mut self, field: Field, index: usize) {
        match field {
            Field::Map => {
                self.map = index;
                let count = self.maps[index].board.as_ref().map_or(0, |(_, n)| *n);
                self.sides.truncate(count);
                // Keep alliances intact if the smaller map lacks a selected team slot.
                for index in 0..self.sides.len() {
                    let old = self.sides[index].team;
                    if old < count { continue; }
                    let free = (0..count).find(|&team| self.sides.iter().all(|s| s.team != team))
                        .expect("at most one team per side");
                    for side in self.sides.iter_mut().filter(|side| side.team == old) {
                        side.team = free;
                    }
                }
                for side in self.sides.len()..count {
                    let default_id = ["Loyalists", "Rebels"].get(side).copied();
                    let faction = self.factions.iter().position(|f| Some(f.id.as_str()) == default_id)
                        .unwrap_or(side % self.factions.len());
                    self.sides.push(Side { team: side, faction, leader: 0 });
                }
                self.side = self.side.min(count.saturating_sub(1));
            }
            Field::Side => self.side = index,
            Field::Team => self.sides[self.side].team = index,
            Field::Faction => {
                if self.sides[self.side].faction != index {
                    self.sides[self.side].faction = index;
                    self.sides[self.side].leader = 0;
                }
            }
            Field::Leader => self.sides[self.side].leader = index,
            Field::Visibility => self.visibility = index,
            Field::Rounds => self.rounds_index = index,
        }
    }

    fn scenario(&self) -> Result<engine_lite::wml::Node, Box<dyn Error>> {
        let map = &self.maps[self.map];
        let (data, _) = map.board.as_ref().map_err(|e| e.as_str())?;
        let sides: Vec<_> = self.sides.iter().map(|side| {
            let faction = &self.factions[side.faction];
            skirmish::SideChoice { faction, leader: Some(&faction.leaders[side.leader]), team: side.team + 1 }
        }).collect();
        skirmish::scenario_with_teams(
            &map.name, data, round_limit(self.rounds_index), &sides, VISIBILITY_OPTIONS[self.visibility].0,
        )
    }

    fn build(&self) -> Result<BoardData, Box<dyn Error>> {
        let scenario = self.scenario()?;
        let mut bytes = Vec::new();
        wml_emit::write_config(&mut bytes, &scenario)?;
        BoardData::from_source(&bytes, &self.core, self.root.clone(), self.root.clone(), self.seed)
    }

    pub(super) async fn run(mut self) -> Result<(), Box<dyn Error>> {
        let mut view = view::SetupView::new(&self.root);
        let mut error = None;
        loop {
            match view.frame(&self, error.as_deref()) {
                view::Input::Choose(field, index) => {
                    self.select(field, index);
                    error = None;
                }
                view::Input::Start(players) => {
                    // Present loading feedback before parsing the original core catalog.
                    next_frame().await;
                    let result = match self.build() {
                        Ok(board) => show_board(board, players).await,
                        Err(cause) => Err(cause),
                    };
                    match result {
                        Ok(()) => return Ok(()),
                        Err(cause) => error = Some(format!("Choose another map or leader. {cause}")),
                    }
                }
                view::Input::Exit => return Ok(()),
                view::Input::Idle => {}
            }
            next_frame().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skirmish_team_picker_controls_computer_targets_and_allied_victory() {
        use engine_lite::{
            board::AttackChoice, fixed_match::{Action, MatchState}, hex::Hex,
            opponent::attack_or_approach, scenario_bundle::Bundle,
        };
        let core = b"[units]\n[unit_type]\nid=Guard\nhitpoints=10\n[attack]\nrange=melee\ndamage=3\nnumber=1\n[/attack]\n[/unit_type]\n[/units]";
        let mut setup = Setup {
            root: PathBuf::new(), core: core.to_vec(), seed: 0,
            maps: vec![MapEntry {
                name: "Three sides".into(),
                board: Ok(("Xv,Xv,Xv,Xv,Xv\nXv,1 Gg,2 Gg,3 Gg,Xv\nXv,Xv,Xv,Xv,Xv".into(), 3)),
            }],
            factions: vec![Faction { id: "Guards".into(), leaders: vec!["Guard".into()], recruits: vec![] }],
            map: 0, side: 0, sides: vec![], visibility: 0, rounds_index: 29,
        };
        let bundle = |setup: &Setup| {
            let mut bytes = Vec::new();
            wml_emit::write_config(&mut bytes, &setup.scenario().unwrap()).unwrap();
            Bundle::parse_with_core(&bytes, &setup.core).unwrap()
        };
        setup.select(Field::Map, 0);
        setup.select(Field::Side, 1);
        assert_eq!(setup.label(Field::Team, setup.selected(Field::Team)), "Team 2");
        setup.select(Field::Team, 0);
        let scenario = bundle(&setup);
        let mut game = scenario.start(7, |_| unreachable!()).unwrap();
        game.step(Action::EndTurn).unwrap();
        let (view, actions) = game.decision().unwrap();
        assert_eq!(view.allied_sides, [1, 2]);
        let attack = |x| Action::Attack(AttackChoice { from: Hex { x: 1, y: 0 }, to: Hex { x, y: 0 }, weapon: 0 });
        assert!(!actions.contains(&attack(0)));
        assert_eq!(attack_or_approach(&view, &actions), Some(&attack(2)));

        setup.select(Field::Side, 2);
        setup.select(Field::Team, 0);
        let scenario = bundle(&setup);
        assert_eq!(scenario.start(7, |_| unreachable!()).unwrap().state(), MatchState::AlliedVictory(vec![1, 2, 3]));
    }

    #[test]
    fn changing_one_sides_faction_resets_only_its_leader_and_map_resizes_the_roster() {
        let faction = |id: &str| Faction {
            id: id.into(), leaders: vec!["First".into(), "Second".into()], recruits: vec!["Recruit".into()],
        };
        let map = |n: usize| MapEntry { name: n.to_string(), board: Ok((String::new(), n)) };
        let mut setup = Setup {
            root: PathBuf::new(), core: Vec::new(), seed: 0, maps: vec![map(2usize), map(4)],
            factions: vec![faction("Rebels"), faction("Loyalists")], map: 0, side: 0, sides: vec![], visibility: 0, rounds_index: 29,
        };
        setup.select(Field::Map, 0);
        assert_eq!(setup.label(Field::Visibility, setup.selected(Field::Visibility)), "Open map");
        assert_eq!(setup.label(Field::Rounds, setup.selected(Field::Rounds)), "30");
        setup.select(Field::Rounds, setup.count(Field::Rounds) - 1);
        assert_eq!(round_limit(setup.selected(Field::Rounds)), -1);
        assert_eq!(setup.label(Field::Rounds, setup.selected(Field::Rounds)), "Unlimited");
        setup.select(Field::Visibility, 3);
        assert_eq!(setup.label(Field::Faction, setup.selected(Field::Faction)), "Loyalists");
        setup.select(Field::Leader, 1);
        setup.select(Field::Map, 1);
        setup.select(Field::Team, 3);
        setup.select(Field::Side, 1);
        setup.select(Field::Team, 3);
        setup.select(Field::Side, 3);
        setup.select(Field::Leader, 1);
        setup.select(Field::Faction, 0);
        assert_eq!(setup.selected(Field::Leader), 0);
        setup.select(Field::Map, 0);
        assert_eq!((setup.count(Field::Side), setup.side), (2, 1));
        assert_eq!(setup.selected(Field::Team), 0);
        setup.select(Field::Side, 0);
        assert_eq!(setup.selected(Field::Team), 0);
        assert_eq!(setup.selected(Field::Leader), 1);
        assert_eq!(setup.label(Field::Visibility, setup.selected(Field::Visibility)), "Fog and shroud");
    }
}
