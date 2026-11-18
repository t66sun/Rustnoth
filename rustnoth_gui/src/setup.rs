use std::{error::Error, fs, path::{Path, PathBuf}};

use engine_lite::{map::ScenarioMap, skirmish::{self, Faction, Visibility}, wml_emit};
use macroquad::prelude::*;

use crate::{read_file, show_board, BoardData};

mod view;

const ROUND_LIMIT: i32 = 30;
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
enum Field { Map, Side, Faction, Leader, Visibility }

impl Field {
    const ALL: [Self; 5] = [Self::Map, Self::Side, Self::Faction, Self::Leader, Self::Visibility];

    fn title(self) -> &'static str {
        match self {
            Self::Map => "Map",
            Self::Side => "Side",
            Self::Faction => "Faction",
            Self::Leader => "Leader",
            Self::Visibility => "Visibility",
        }
    }
}

#[derive(Clone, Copy)]
struct Side {
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
        let mut setup = Self { root, core, seed, maps, factions, map: first, side: 0, sides: Vec::new(), visibility: 0 };
        setup.select(Field::Map, first);
        Ok(setup)
    }

    fn count(&self, field: Field) -> usize {
        match field {
            Field::Map => self.maps.len(),
            Field::Side => self.sides.len(),
            Field::Visibility => VISIBILITY_OPTIONS.len(),
            Field::Faction if !self.sides.is_empty() => self.factions.len(),
            Field::Leader if !self.sides.is_empty() => self.factions[self.sides[self.side].faction].leaders.len(),
            _ => 0,
        }
    }

    fn selected(&self, field: Field) -> usize {
        match field {
            Field::Map => self.map,
            Field::Side => self.side,
            Field::Faction => self.sides[self.side].faction,
            Field::Leader => self.sides[self.side].leader,
            Field::Visibility => self.visibility,
        }
    }

    fn label(&self, field: Field, index: usize) -> String {
        match field {
            Field::Map => self.maps[index].name.clone(),
            Field::Side => format!("Side {} of {}", index + 1, self.sides.len()),
            Field::Faction => self.factions[index].id.clone(),
            Field::Leader => self.factions[self.sides[self.side].faction].leaders[index].clone(),
            Field::Visibility => VISIBILITY_OPTIONS[index].1.to_owned(),
        }
    }

    fn select(&mut self, field: Field, index: usize) {
        match field {
            Field::Map => {
                self.map = index;
                let count = self.maps[index].board.as_ref().map_or(0, |(_, n)| *n);
                self.sides.truncate(count);
                for side in self.sides.len()..count {
                    let default_id = ["Loyalists", "Rebels"].get(side).copied();
                    let faction = self.factions.iter().position(|f| Some(f.id.as_str()) == default_id)
                        .unwrap_or(side % self.factions.len());
                    self.sides.push(Side { faction, leader: 0 });
                }
                self.side = self.side.min(count.saturating_sub(1));
            }
            Field::Side => self.side = index,
            Field::Faction => {
                if self.sides[self.side].faction != index {
                    self.sides[self.side] = Side { faction: index, leader: 0 };
                }
            }
            Field::Leader => self.sides[self.side].leader = index,
            Field::Visibility => self.visibility = index,
        }
    }

    fn build(&self) -> Result<BoardData, Box<dyn Error>> {
        let map = &self.maps[self.map];
        let (data, _) = map.board.as_ref().map_err(|e| e.as_str())?;
        let sides: Vec<_> = self.sides.iter().map(|side| {
            let faction = &self.factions[side.faction];
            (faction, Some(faction.leaders[side.leader].as_str()))
        }).collect();
        let scenario = skirmish::scenario_with_visibility(
            &map.name, data, ROUND_LIMIT, &sides, VISIBILITY_OPTIONS[self.visibility].0,
        )?;
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
    fn changing_one_sides_faction_resets_only_its_leader_and_map_resizes_the_roster() {
        let faction = |id: &str| Faction {
            id: id.into(), leaders: vec!["First".into(), "Second".into()], recruits: vec!["Recruit".into()],
        };
        let map = |n: usize| MapEntry { name: n.to_string(), board: Ok((String::new(), n)) };
        let mut setup = Setup {
            root: PathBuf::new(), core: Vec::new(), seed: 0, maps: vec![map(2usize), map(4)],
            factions: vec![faction("Rebels"), faction("Loyalists")], map: 0, side: 0, sides: vec![], visibility: 0,
        };
        setup.select(Field::Map, 0);
        assert_eq!(setup.label(Field::Visibility, setup.selected(Field::Visibility)), "Open map");
        setup.select(Field::Visibility, 3);
        assert_eq!(setup.label(Field::Faction, setup.selected(Field::Faction)), "Loyalists");
        setup.select(Field::Leader, 1);
        setup.select(Field::Map, 1);
        setup.select(Field::Side, 3);
        setup.select(Field::Leader, 1);
        setup.select(Field::Faction, 0);
        assert_eq!(setup.selected(Field::Leader), 0);
        setup.select(Field::Map, 0);
        assert_eq!((setup.count(Field::Side), setup.side), (2, 1));
        setup.select(Field::Side, 0);
        assert_eq!(setup.selected(Field::Leader), 1);
        assert_eq!(setup.label(Field::Visibility, setup.selected(Field::Visibility)), "Fog and shroud");
    }
}
