//! Local hot-seat and versus-computer checkpoints with their starting rules and map.
use crate::BoardData;
use engine_lite::{
    hex::Hex,
    wml::{self, Node},
    wml_emit,
};
use rustnoth_gui::{
    save_replay::{Event, invalid, set},
    session::Players,
};
use std::{
    error::Error,
    fs, io,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub(super) struct Restore {
    pub replay_roster: Option<engine_lite::fixed_match::carryover::CampaignCarryover>,
    pub map: Option<String>,
    pub events: Vec<Event>,
    pub camera: Option<[f32; 3]>,
    pub selected: Option<Hex>,
}

pub(super) struct Slot {
    pub directory: PathBuf,
    pub label: String,
}
pub(super) fn directory() -> io::Result<PathBuf> {
    Ok(crate::display_preferences::config_directory()?.join("saves"))
}

pub(super) fn slots(base: &Path) -> io::Result<Vec<Slot>> {
    let files = match fs::read_dir(base) {
        Ok(files) => files,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut slots = Vec::new();
    for entry in files {
        let entry = entry?;
        let path = entry.path();
        if !entry.file_type()?.is_dir() || path.extension().is_none_or(|ext| ext != "rnsave") {
            continue;
        }
        let label = fs::read_to_string(path.join("name.txt"))
            .unwrap_or_else(|_| entry.file_name().to_string_lossy().into_owned());
        slots.push(Slot {
            directory: path,
            label,
        });
    }
    slots.sort_by(|a, b| b.directory.cmp(&a.directory));
    Ok(slots)
}

pub(super) fn save(
    base: &Path,
    name: &str,
    data: &BoardData,
    players: Players,
    map: Option<&str>,
    events: &[Event],
    camera: Option<[f32; 3]>,
    selected: Option<Hex>,
    replay_roster: Option<&Node>,
) -> io::Result<()> {
    let mut record = Node::default();
    set(&mut record, "format", 1);
    set(&mut record, "seed", data.seed);
    set(
        &mut record,
        "players",
        if players == Players::HotSeat {
            "hot_seat"
        } else {
            "computer"
        },
    );
    if let Some([x, y, zoom]) = camera {
        set(&mut record, "camera_x", x);
        set(&mut record, "camera_y", y);
        set(&mut record, "tile_size", zoom);
    }
    if let Some(hex) = selected {
        set(&mut record, "selected_x", hex.x);
        set(&mut record, "selected_y", hex.y);
    }
    record
        .children
        .extend(events.iter().map(|event| ("step".into(), event.encode())));
    fs::create_dir_all(base)?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let key = format!("{stamp:032}-{}", std::process::id());
    let draft = base.join(format!("{key}.pending"));
    fs::create_dir(&draft)?;
    let result = (|| {
        fs::write(draft.join("name.txt"), name)?;
        fs::write(draft.join("scenario.cfg"), &data.source)?;
        fs::write(draft.join("rules.cfg"), &data.core)?;
        if let Some(map) = map {
            fs::write(draft.join("board.map"), map)?;
        }
        wml_emit::write_config(fs::File::create(draft.join("replay.cfg"))?, &record)?;
        if let Some(roster) = replay_roster {
            wml_emit::write_config(fs::File::create(draft.join("starting-roster.cfg"))?, roster)?;
        }
        fs::rename(&draft, base.join(format!("{key}.rnsave")))
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(draft);
    }
    result
}

pub(super) fn load(slot: &Slot, root: &Path) -> Result<(BoardData, Players), Box<dyn Error>> {
    let dir = &slot.directory;
    let record = wml::parse(fs::read(dir.join("replay.cfg"))?)?;
    if record.attribute("format") != Some("1") {
        return Err(invalid("Unsupported Rustnoth save version").into());
    }
    let seed = record
        .attribute("seed")
        .ok_or("Missing saved seed")?
        .parse()?;
    let players = match record.attribute("players") {
        Some("computer") => Players::VersusComputer,
        Some("hot_seat") => Players::HotSeat,
        _ => return Err(invalid("Unknown control mode in saved game").into()),
    };
    let mut events = Vec::new();
    for (tag, step) in &record.children {
        if tag != "step" {
            return Err(invalid("Unexpected saved game entry").into());
        }
        events.push(Event::decode(step)?);
    }
    let map = match fs::read_to_string(dir.join("board.map")) {
        Ok(map) => Some(map),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.into()),
    };
    let node_float = |name| {
        record
            .attribute(name)?
            .parse::<f32>()
            .ok()
            .filter(|v| v.is_finite())
    };
    let camera = node_float("camera_x").and_then(|x| {
        Some([
            x,
            node_float("camera_y")?,
            node_float("tile_size")?.max(1.0),
        ])
    });
    let selected = record.attribute("selected_x").and_then(|x| {
        Some(Hex {
            x: x.parse().ok()?,
            y: record.attribute("selected_y")?.parse().ok()?,
        })
    });
    let mut data = BoardData::from_source(
        &fs::read(dir.join("scenario.cfg"))?,
        &fs::read(dir.join("rules.cfg"))?,
        dir.clone(),
        root.to_owned(),
        seed,
    )?;
    data.restore = Some(Restore {
        replay_roster: match fs::read(dir.join("starting-roster.cfg")) {
            Ok(bytes) => Some(engine_lite::fixed_match::carryover::CampaignCarryover::from_replay_roster(
                &wml::parse(bytes)?).ok_or("Invalid starting roster in campaign replay")?),
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.into()),
        },
        map,
        events,
        camera,
        selected,
    });
    Ok((data, players))
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_lite::{board::AttackChoice, fixed_match::Action};
    use rustnoth_gui::session::Session;

    #[test]
    fn a_campaign_replay_reconstructs_its_incoming_veteran_before_playing_the_actions() {
        use engine_lite::fixed_match::carryover::CampaignCarryover;
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let temp = std::env::temp_dir().join(format!("rustnoth-scenario-replay-{stamp}"));
        fs::create_dir_all(temp.join("data/core/images")).unwrap();
        let source = b"[scenario]\nid=west\nname=West\nturns=9\nmap_file=west.map\n[side]\nid=hero\nsave_id=army\ncontroller=human\ntype=Guard\n[/side]\n[side]\nid=foe\ntype=Guard\n[/side]\n[event]\nname=side 1 turn end\n[endlevel]\nresult=victory\n[/endlevel]\n[/event]\n[/scenario]";
        let core = b"[units]\n[unit_type]\nid=Guard\nhitpoints=20\nmovement=0\n[/unit_type]\n[/units]";
        let map = "Xv,Xv,Xv,Xv,Xv,Xv\nXv,1 Gg,Gg,Gg,2 Gg,Xv\nXv,Xv,Xv,Xv,Xv,Xv";
        let roster = wml::parse("[side]\nsave_id=army\n[unit]\nid=hero\ntype=Guard\nname=Veteran\nexperience=12\nearned_amlas=0\ncanrecruit=yes\n[/unit]\n[/side]").unwrap();
        let mut route = CampaignCarryover::from_replay_roster(&roster).unwrap();
        let starting_roster = route.replay_roster();
        let board = BoardData::from_source(source, core, temp.clone(), temp.clone(), 12).unwrap();
        let mut live = Session::new(board.scenario.start_campaign(12, |_| Ok(map.into()),Some(&mut route)).unwrap());
        let units = |s: &Session| s.observation().unwrap().units.iter().map(|u| (u.side,u.position,u.experience)).collect::<Vec<_>>();
        let opening = units(&live);
        assert!(opening.iter().any(|&(side,_,xp)| side == 1 && xp == 12));
        live.apply(Action::EndTurn).unwrap();
        assert!(live.save_victory_replay());
        save(&temp, "West replay", &board, Players::HotSeat, Some(map), live.history(),None,None,Some(&starting_roster)).unwrap();
        let entries = slots(&temp).unwrap();
        let (mut archive, _) = load(&entries[0], &temp).unwrap();
        let mut replay = archive.restore.take().unwrap();
        let mut watch = Session::new(archive.scenario.start_campaign(12, |_| Ok(replay.map.unwrap()),replay.replay_roster.as_mut()).unwrap());
        assert_eq!(units(&watch), opening);
        for event in replay.events { event.apply(&mut watch).unwrap(); }
        assert_eq!(watch.state(), live.state());
        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn standalone_save_recreates_the_view_next_turn_and_actions_without_source_files() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp =
            std::env::temp_dir().join(format!("rustnoth-save-test-{}-{stamp}", std::process::id()));
        fs::create_dir_all(temp.join("data/core/images")).unwrap();
        let source=b"[scenario]\nid=road\nname=Road\nturns=9\nmap_file=road.map\n[side]\nid=leader\ntype=Guard\n[/side]\n[side]\nid=rival\ntype=Guard\n[/side]\n[event]\nname=start\n[message]\nspeaker=narrator\nmessage=Welcome\n[/message]\n[/event]\n[/scenario]";
        let core =
            b"[units]\n[unit_type]\nid=Guard\nhitpoints=20\nmovement=0\n[/unit_type]\n[/units]";
        let map = "Xv,Xv,Xv,Xv,Xv,Xv,Xv,Xv\nXv,1 Gg,Gg,Gg,Gg,Gg,2 Gg,Xv\nXv,Xv,Xv,Xv,Xv,Xv,Xv,Xv";
        let board = BoardData::from_source(source, core, temp.clone(), temp.clone(), 99).unwrap();
        let mut before = Session::new(board.scenario.start(99, |_| Ok(map.to_owned())).unwrap());
        assert_eq!(before.dialogue().unwrap().text, "Welcome");
        before.dismiss_dialogue();
        before.apply(Action::EndTurn).unwrap();
        assert_eq!(before.observation().unwrap().side, 2);
        let base = temp.join("saves");
        let focus = Some([2.0, 1.0, 72.0]);
        let selected = Some(Hex { x: 5, y: 0 });
        save(
            &base,
            "Road checkpoint",
            &board,
            Players::HotSeat,
            Some(map),
            before.history(),
            focus,
            selected,
            None,
        )
        .unwrap();
        let entries = slots(&base).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].label, "Road checkpoint");
        let (mut restored, players) = load(&entries[0], &temp).unwrap();
        let checkpoint = restored.restore.take().unwrap();
        assert_eq!((checkpoint.camera, checkpoint.selected), (focus, selected));
        assert_eq!(checkpoint.events, before.history());
        let mut after = Session::with_players(
            restored
                .scenario
                .start(restored.seed, |_| Ok(checkpoint.map.unwrap()))
                .unwrap(),
            players,
        );
        for step in checkpoint.events {
            step.apply(&mut after).unwrap();
        }
        assert_eq!(after.observation(), before.observation());
        assert_eq!(after.legal_actions(), before.legal_actions());
        assert_eq!(after.state(), before.state());
        after.apply(Action::EndTurn).unwrap();
        before.apply(Action::EndTurn).unwrap();
        assert_eq!(after.observation(), before.observation());
        let preferred = Event::Player(
            Action::Attack(AttackChoice {
                from: Hex { x: 0, y: 0 },
                to: Hex { x: 1, y: 0 },
                weapon: 2,
            }),
            Some("Knight".into()),
            None,
        );
        assert_eq!(Event::decode(&preferred.encode()).unwrap(), preferred);
        fs::remove_dir_all(temp).unwrap();
    }
}
