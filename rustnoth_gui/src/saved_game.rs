//! Desktop checkpoints with their starting rules, map and campaign continuation.
use crate::{BoardData, campaign_browser::Browser};
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
    pub watch_replay: bool,
    pub campaign: Option<Browser>,
    pub replay_roster: Option<engine_lite::fixed_match::carryover::CampaignCarryover>,
    pub map: Option<String>,
    pub events: Vec<Event>,
    pub tutorial_hints: Option<crate::tutorial::SavedHints>,
    pub camera: Option<[f32; 3]>,
    pub selected: Option<Hex>,
}

impl Restore {
    pub fn is_replay(&self) -> bool {
        self.campaign.is_none() && self.replay_roster.is_some()
    }
}

pub(super) enum CampaignSave<'a> {
    Replay(&'a Node),
    Checkpoint(&'a Browser, &'a Node, &'a str),
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
    campaign: Option<CampaignSave<'_>>,
) -> io::Result<()> {
    let mut record = Node::default();
    set(&mut record, "format", 6);
    set(&mut record, "seed", data.seed);
    if let Some(CampaignSave::Checkpoint(_, _, hint)) = &campaign {
        set(&mut record, "kind", "campaign");
        if let Some(lesson) = &data.lesson {
            record.children.push(("battle_training".into(), lesson.checkpoint(hint)));
        }
    }
    set(
        &mut record,
        "players",
        match players {
            Players::HotSeat => "hot_seat",
            Players::VersusComputer => "computer",
            Players::Campaign => "campaign",
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
        let mut output = io::BufWriter::new(fs::File::create(draft.join("replay.cfg"))?);
        wml_emit::write_translatable_config(&mut output, &record)?;
        io::Write::flush(&mut output)?;
        drop(output);
        if let Some(kind) = campaign {
            let roster = match kind {
                CampaignSave::Replay(roster) => roster,
                CampaignSave::Checkpoint(browser, roster, _) => {
                    browser.save_route(&draft.join("campaign.cfg"))?;
                    roster
                }
            };
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
    let format = record.attribute("format");
    if !matches!(format, Some("1" | "2" | "3" | "4" | "5" | "6")) {
        return Err(invalid("Unsupported Rustnoth save version").into());
    }
    let seed = record
        .attribute("seed")
        .ok_or("Missing saved seed")?
        .parse()?;
    let players = match record.attribute("players") {
        Some("computer") => Players::VersusComputer,
        Some("hot_seat") => Players::HotSeat,
        Some("campaign") => Players::Campaign,
        _ => return Err(invalid("Unknown control mode in saved game").into()),
    };
    let mut events = Vec::new();
    let mut tutorial = None;
    for (tag, step) in &record.children {
        match tag.as_str() {
            "step" => events.push(Event::decode(step)?),
            "battle_training" if tutorial.is_none() && record.attribute("kind") == Some("campaign") =>
                tutorial = Some(crate::tutorial::Lesson::from_checkpoint(step)?),
            _ => return Err(invalid("Unexpected saved game entry").into()),
        }
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
    let mut source = fs::read(dir.join("scenario.cfg"))?;
    if let Some(version @ ("1" | "2" | "3" | "4")) = format { source = legacy_scenario(&source, version)?; }
    let mut data = BoardData::from_source(
        &source,
        &fs::read(dir.join("rules.cfg"))?,
        dir.clone(),
        root.to_owned(),
        seed,
    )?;
    let campaign = match record.attribute("kind") {
        Some("campaign") => Some(Browser::load_route(&dir.join("campaign.cfg"), &data)?),
        None => None,
        _ => return Err(invalid("Unknown saved game kind").into()),
    };
    let (lesson, tutorial_hints) = tutorial.unzip();
    data.lesson = lesson;
    data.restore = Some(Restore {
        tutorial_hints,
        watch_replay: false,
        replay_roster: match fs::read(dir.join("starting-roster.cfg")) {
            Ok(bytes) => Some(engine_lite::fixed_match::carryover::CampaignCarryover::from_replay_roster(
                &wml::parse(bytes)?).ok_or("Invalid starting roster in campaign replay")?),
            Err(e) if e.kind() == io::ErrorKind::NotFound && campaign.is_none() => None,
            Err(e) => return Err(e.into()),
        },
        campaign,
        map,
        events,
        camera,
        selected,
    });
    Ok((data, players))
}

/// Encode the old scenario semantics in the embedded source so resaves replay the same history.
fn legacy_scenario(bytes: &[u8], version: &str) -> Result<Vec<u8>, Box<dyn Error>> {
    fn fixed(unit: &mut Node) {
        set(unit, "random_gender", "no");
        set(unit, "random_traits", "no");
    }
    fn fixed_events(actions: &mut Node, fixed_reinforcements: bool) {
        for (tag, child) in &mut actions.children {
            match tag.as_str() {
                "unit" if fixed_reinforcements => fixed(child),
                // Through format 3 an object action made its enclosing event unsupported.
                "object" => *tag = "rustnoth_legacy_unsupported_object".into(),
                "event" | "if" | "then" | "else" => fixed_events(child, fixed_reinforcements),
                _ => {}
            }
        }
    }
    let mut document = wml::parse(bytes)?;
    for (_, scenario) in document.children.iter_mut().filter(|(tag, _)| tag == "scenario") {
        set(scenario, "rustnoth_legacy_recruits", "yes");
        for (tag, child) in &mut scenario.children {
            if tag == "event" && version != "4" { fixed_events(child, version != "3"); }
            if tag == "side" && version == "1" {
                fixed(child);
                for (_, unit) in child.children.iter_mut().filter(|(tag, _)| tag == "unit" || tag == "leader") {
                    fixed(unit);
                }
            }
        }
    }
    let mut out = Vec::new();
    wml_emit::write_translatable_config(&mut out, &document)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_lite::{board::AttackChoice, fixed_match::Action};
    use rustnoth_gui::session::Session;

    #[test]
    fn version_four_keeps_its_recruit_stream_and_authored_objects_on_resave() {
        let temp = std::env::temp_dir().join(format!("rustnoth-legacy-recruit-{}", std::process::id()));
        fs::create_dir_all(temp.join("data/core/images")).unwrap();
        fs::write(temp.join("rules.cfg"), "[units]\n[/units]").unwrap();
        fs::write(temp.join("scenario.cfg"), "[scenario]\nid=old\n[event]\nname=prestart\n[object]\n[/object]\n[/event]\n[/scenario]").unwrap();
        fs::write(temp.join("replay.cfg"), "format=4\nplayers=hot_seat\nseed=5489").unwrap();
        let old = Slot { directory: temp.clone(), label: "legacy".into() };
        let (data, players) = load(&old, &temp).unwrap();
        let saves = temp.join("saves");
        save(&saves, "again", &data, players, None, &[], None, None, None).unwrap();
        let (reopened, _) = load(&slots(&saves).unwrap()[0], &temp).unwrap();
        let source = wml::parse(&reopened.source).unwrap();
        let scenario = &source.children.iter().find(|(tag, _)| tag == "scenario").unwrap().1;
        assert_eq!(scenario.attribute("rustnoth_legacy_recruits"), Some("yes"));
        assert_eq!(scenario.children[0].1.children[0].0, "object");
        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn a_version_three_water_crossing_survives_load_and_resave() {
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let temp = std::env::temp_dir().join(format!("rustnoth-westin-water-{stamp}"));
        fs::create_dir_all(temp.join("data/core/images")).unwrap();
        let slot = Slot { directory: temp.clone(), label: "River".into() };
        let core = "[terrain_type]\nid=flat\nstring=Gg\n[/terrain_type]\n[terrain_type]\nid=shallow_water\nstring=Ww\n[/terrain_type]\n[units]\n[movetype]\nname=foot\n[movement_costs]\nflat=1\nshallow_water=1\n[/movement_costs]\n[/movetype]\n[unit_type]\nid=Guard\nmovement_type=foot\nmovement=5\nhitpoints=30\n[/unit_type]\n[/units]";
        let source = "[scenario]\nmap_data=\"Xv,Xv,Xv,Xv,Xv\nXv,1 Gg,Ww,2 Gg,Xv\nXv,Xv,Xv,Xv,Xv\"\n[side]\ntype=Guard\n[/side]\n[side]\ntype=Guard\n[/side]\n[event]\nname=prestart\n[object]\nsilent=yes\n[filter]\nside=2\ncanrecruit=yes\n[/filter]\n[effect]\napply_to=movement_costs\nreplace=yes\n[movement_costs]\nshallow_water=99\n[/movement_costs]\n[/effect]\n[/object]\n[/event]\n[/scenario]";
        fs::write(temp.join("rules.cfg"), core).unwrap();
        fs::write(temp.join("scenario.cfg"), source).unwrap();
        let mut history = wml::parse("format=3\nplayers=hot_seat\nseed=5489").unwrap();
        let crossing = Action::Move { from: Hex { x: 2, y: 0 }, to: Hex { x: 1, y: 0 } };
        for action in [Action::EndTurn, crossing] {
            history.children.push(("step".into(), Event::Player(action, None, None).encode()));
        }
        wml_emit::write_config(fs::File::create(temp.join("replay.cfg")).unwrap(), &history).unwrap();
        let resume = |slot: &Slot| {
            let (data, players) = load(slot, &temp).unwrap();
            let mut session = Session::with_players(data.scenario.start(5489, |_| unreachable!()).unwrap(), players);
            let events = &data.restore.as_ref().unwrap().events;
            for event in events { event.apply(&mut session).unwrap(); }
            save(&temp.join("resaved"), "Crossing", &data, players, None, session.history(), None, None, None).unwrap();
        };
        resume(&slot);
        resume(&slots(&temp.join("resaved")).unwrap()[0]);
        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn legacy_move_history_keeps_its_fixed_reinforcement_on_reload_and_resave() {
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let temp = std::env::temp_dir().join(format!("rustnoth-fixed-event-{stamp}"));
        fs::create_dir_all(temp.join("data/core/images")).unwrap();
        let slot = Slot { directory: temp.clone(), label: "Citadel".into() };
        let source = "[scenario]\nmap_data=\"Xv,Xv,Xv,Xv,Xv,Xv\nXv,1 Gg,Gg,Gg,2 Gg,Xv\nXv,Xv,Xv,Xv,Xv,Xv\"\n[side]\ntype=Guard\n[/side]\n[side]\ntype=Guard\n[/side]\n[event]\nname=moveto\n[unit]\ntype=Guard\nid=Ufes\nx,y=3,1\nrandom_traits=yes\n[/unit]\n[/event]\n[/scenario]";
        let core = "[terrain_type]\nstring=Gg\nid=flat\n[/terrain_type]\n[units]\n[movetype]\nname=foot\n[movement_costs]\nflat=1\n[/movement_costs]\n[/movetype]\n[unit_type]\nid=Guard\nhitpoints=30\nmovement_type=foot\nmovement=4\nnum_traits=1\n[/unit_type]\n[trait]\nid=strong\n[/trait]\n[/units]";
        fs::write(temp.join("scenario.cfg"), source).unwrap();
        fs::write(temp.join("rules.cfg"), core).unwrap();
        let action = Action::Move { from: Hex { x: 0, y: 0 }, to: Hex { x: 1, y: 0 } };
        for version in [1, 2] {
            let mut history = Node::default();
            set(&mut history, "format", version);
            set(&mut history, "players", "hot_seat");
            set(&mut history, "seed", 5489);
            history.children.push(("step".into(), Event::Player(action.clone(), None, None).encode()));
            wml_emit::write_config(fs::File::create(temp.join("replay.cfg")).unwrap(), &history).unwrap();
            let (board, players) = load(&slot, &temp).unwrap();
            let events = &board.restore.as_ref().unwrap().events;
            let mut game = Session::with_players(board.scenario.start(5489, |_| unreachable!()).unwrap(), players);
            events[0].apply(&mut game).unwrap();
            let read_traits = |data: &BoardData| {
                let mut battle = data.scenario.start(5489, |_| unreachable!()).unwrap();
                battle.step(action.clone()).unwrap();
                battle.audit_snapshot().units.into_iter().find(|u| u.position == Hex { x: 2, y: 0 }).unwrap().traits
            };
            assert!(read_traits(&board).is_empty());
            let directory = temp.join(version.to_string());
            save(&directory, "continued", &board, players, None, game.history(), None, None, None).unwrap();
            let (resaved, _) = load(&slots(&directory).unwrap()[0], &temp).unwrap();
            assert!(read_traits(&resaved).is_empty());
        }
        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn an_old_save_restores_its_fixed_starting_roster_and_first_action() {
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let temp = std::env::temp_dir().join(format!("rustnoth-fixed-opening-{stamp}"));
        fs::create_dir_all(temp.join("data/core/images")).unwrap();
        let slot = Slot { directory: temp.clone(), label: "legacy".into() };
        fs::write(temp.join("replay.cfg"), "format=1\nseed=5489\nplayers=hot_seat\n").unwrap();
        fs::write(temp.join("scenario.cfg"), b"[scenario]\nmap_data=\"Xv,Xv,Xv,Xv,Xv\nXv,1 Gg,Gg,2 Gg,Xv\nXv,Xv,Xv,Xv,Xv\"\n[side]\ntype=Guard\n[unit]\ntype=Guard\nx,y=2,1\nrandom_traits=yes\n[/unit]\n[/side]\n[side]\ntype=Guard\n[/side]\n[/scenario]").unwrap();
        fs::write(temp.join("rules.cfg"), b"[units]\n[unit_type]\nid=Guard\nnum_traits=1\n[/unit_type]\n[trait]\nid=strong\n[/trait]\n[/units]").unwrap();
        let (data, _) = load(&slot, &temp).unwrap();
        let mut game = data.scenario.start(data.seed, |_| unreachable!()).unwrap();
        assert!(game.audit_snapshot().units.iter().all(|u| u.traits.is_empty()));
        game.step(Action::EndTurn).unwrap();
        assert_eq!(game.playing_side(), 2);
        fs::write(temp.join("replay.cfg"), "format=2\nseed=5489\nplayers=hot_seat\n").unwrap();
        let (data, _) = load(&slot, &temp).unwrap();
        let game = data.scenario.start(data.seed, |_| unreachable!()).unwrap();
        assert_eq!(game.audit_snapshot().units[1].traits, ["strong"]);
        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn a_midcampaign_checkpoint_replays_the_turn_and_continues_with_its_veterans_and_treasury() {
        use engine_lite::fixed_match::carryover::CampaignCarryover;
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let temp = std::env::temp_dir().join(format!("rustnoth-campaign-resume-{stamp}"));
        fs::create_dir_all(temp.join("data/core/images")).unwrap();
        let maps = temp.join("maps");
        fs::create_dir(&maps).unwrap();
        let map = "Xv,Xv,Xv,Xv,Xv\nXv,1 Gg,Gg,2 Gg,Xv\nXv,Xv,Xv,Xv,Xv";
        fs::write(maps.join("battle.map"), map).unwrap();
        let file = temp.join("original.cfg");
        fs::write(&file, r#"
[scenario]
id=west
map_file=battle.map
[side]
id=hero
controller=human
type=Guard
[/side]
[side]
id=foe
type=Guard
[/side]
[event]
name=start
[message]
speaker=narrator
message=Advance
[/message]
[/event]
[event]
name=side 1 turn end
[endlevel]
result=victory
next_scenario=east
[/endlevel]
[/event]
[/scenario]
[scenario]
id=east
map_file=battle.map
[side]
id=hero
type=Guard
[/side]
[side]
id=ally
type=Guard
gold=20
[/side]
[/scenario]"#).unwrap();
        let core = b"[units]\n[unit_type]\nid=Guard\nhitpoints=20\nmovement=4\n[/unit_type]\n[/units]";
        let browser = Browser::load(&file, maps.clone(), core.to_vec(), temp.clone(), 12).unwrap();
        let incoming = wml::parse("[side]\nsave_id=hero\n[unit]\nid=hero\ntype=Guard\nname=Veteran\nexperience=12\nearned_amlas=0\ncanrecruit=yes\n[/unit]\n[/side]\n[treasury]\nsave_id=ally\namount=46\nadd=true\n[/treasury]").unwrap();
        let mut route = CampaignCarryover::from_replay_roster(&incoming).unwrap();
        let board = browser.board("west", &mut route).unwrap();
        let opening = route.replay_roster();
        let mut live = Session::new(board.scenario.start_campaign(12, |_| unreachable!(), Some(&mut route)).unwrap());
        assert_eq!(live.dialogue().unwrap().text, "Advance");
        live.dismiss_dialogue();
        let base = temp.join("saves");
        save(&base, "West", &board, Players::HotSeat, None, live.history(), None, None,
            Some(CampaignSave::Checkpoint(&browser, &opening, ""))).unwrap();
        fs::remove_file(file).unwrap();
        fs::remove_dir_all(maps).unwrap();
        let (mut restored, players) = load(&slots(&base).unwrap()[0], &temp).unwrap();
        let checkpoint = restored.restore.take().unwrap();
        assert!(!checkpoint.is_replay());
        let campaign = checkpoint.campaign.unwrap();
        let mut route = checkpoint.replay_roster.unwrap();
        let mut resumed = Session::with_players(restored.scenario.start_campaign(restored.seed, |_| unreachable!(), Some(&mut route)).unwrap(), players);
        for command in checkpoint.events { command.apply(&mut resumed).unwrap(); }
        assert_eq!(resumed.legal_actions(), live.legal_actions());
        assert_eq!(resumed.observation(), live.observation());
        assert!(resumed.dialogue().is_none());
        resumed.apply(Action::EndTurn).unwrap();
        assert_eq!(resumed.next_scenario(), Some("east"));
        resumed.store_campaign_survivors(&mut route);
        let successor = campaign.board(resumed.next_scenario().unwrap(), &mut route).unwrap();
        let game = successor.scenario.start_campaign(successor.seed, |_| unreachable!(), Some(&mut route)).unwrap();
        assert_eq!(game.observation_for(2).unwrap().gold, Some(66));
        assert!(game.observation_for(1).unwrap().units.iter().any(|u| u.side == 1 && u.experience == 12));
        fs::remove_dir_all(temp).unwrap();
    }

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
        save(&temp, "West replay", &board, Players::Campaign, Some(map), live.history(),None,None,Some(CampaignSave::Replay(&starting_roster))).unwrap();
        let entries = slots(&temp).unwrap();
        let (mut archive, players) = load(&entries[0], &temp).unwrap();
        assert_eq!(players, Players::Campaign);
        let mut replay = archive.restore.take().unwrap();
        let mut watch = Session::with_players(archive.scenario.start_campaign(12, |_| Ok(replay.map.unwrap()),replay.replay_roster.as_mut()).unwrap(), players);
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
