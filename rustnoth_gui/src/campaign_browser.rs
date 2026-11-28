#[path = "campaign_selector.rs"]
mod selector;
pub(crate) use selector::combat::Mode as CombatMode;
#[path = "installed_campaigns.rs"]
mod installed;
#[path = "campaign_modifications.rs"]
pub(crate) mod modifications;
use std::{error::Error, fs, path::{Path, PathBuf}};

use engine_lite::{campaign, fixed_match::carryover::CampaignCarryover, wml, wml_emit};
use macroquad::prelude::{next_frame, set_default_camera};
use rustnoth_gui::session::Players;

use crate::{BoardData, dialog::ListAction, read_file, play_board};

pub(super) struct Browser {
    campaign: wml::Node,
    maps: Option<PathBuf>,
    core: Vec<u8>,
    root: PathBuf,
    seed: u32,
    installed: std::collections::HashMap<String, installed::Campaign>,
    mode: Option<CombatMode>,
    modifications: Vec<wml::Node>,
}

impl Browser {
    pub(super) fn saved_descriptor(&self) -> Option<&wml::Node> {
        let mut campaigns = self.campaign.children.iter().filter(|(tag, _)| tag == "campaign");
        let (_, descriptor) = campaigns.next()?;
        campaigns.next().is_none().then_some(descriptor)
    }

    pub(super) fn load(file: &Path, maps: PathBuf, core: Vec<u8>, root: PathBuf, seed: u32)
        -> Result<Self, Box<dyn Error>>
    {
        let campaign = wml::parse(read_file(file)?)?;
        if campaign::scenarios(&campaign).next().is_none() {
            return Err("campaign contains no scenarios with an ID".into());
        }
        Ok(Self { campaign, maps: Some(maps), core, root, seed, installed: Default::default(), mode: None, modifications: Vec::new() })
    }

    pub(super) fn installed(file: &Path, folders: &Path, core: Vec<u8>, root: PathBuf, seed: u32, extras: wml::Node) -> Result<Self, Box<dyn Error>> {
        let (mut campaign, installed) = installed::catalog(wml::parse(read_file(file)?)?, folders)?;
        campaign.children.extend(extras.children);
        Ok(Self { campaign, maps: Some(PathBuf::new()), core, root, seed, installed, mode: None, modifications: Vec::new() })
    }

    pub(super) fn board(&self, id: &str, route: &mut CampaignCarryover) -> Result<BoardData, Box<dyn Error>> {
        let mut selected = campaign::select(&self.campaign, id, |file| self.read_map(file))?;
        if let Some(mode) = self.mode { mode.classify(&mut selected); }
        for (_, scenario) in selected.children.iter_mut().filter(|(tag, _)| tag == "scenario") {
            modifications::apply(scenario, &self.modifications)?;
            route.gold.transfer_to(scenario);
        }
        let mut source = Vec::new();
        wml_emit::write_config(&mut source, &selected)?;
        BoardData::from_source(&source, &self.core, self.maps.clone().unwrap_or_default(), self.root.clone(), route.scenario_seed(self.seed))
    }

    fn read_map(&self, file: &str) -> std::io::Result<String> {
        let directory = self.maps.as_ref().ok_or_else(|| std::io::Error::new(
            std::io::ErrorKind::NotFound, format!("Campaign map {file} is missing from this save")))?;
        fs::read_to_string(directory.join(file))
    }

    pub(super) fn save_route(&self, file: &Path) -> std::io::Result<()> {
        let mut source = self.campaign.clone();
        if let Some(mode) = self.mode { mode.classify(&mut source); }
        for (_, scenario) in source.children.iter_mut().filter(|(tag, _)| tag == "scenario") {
            if scenario.attribute("map_data").is_some_and(|data| !data.is_empty()) { continue; }
            if let Some(name) = scenario.attribute("map_file").filter(|s| !s.is_empty()) {
                match self.read_map(name) {
                    Ok(map) => {
                        rustnoth_gui::save_replay::set(scenario, "map_data", map);
                        scenario.attributes.remove("map_file");
                    }
                    // A missing map only blocks the branch of the campaign which uses it.
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e),
                }
            }
        }
        let mut archive = wml::Node::default();
        archive.children.push(("campaign_document".into(), source));
        archive.children.extend(self.modifications.iter().cloned().map(|node| ("modification".into(), node)));
        let mut output = std::io::BufWriter::new(fs::File::create(file)?);
        wml_emit::write_translatable_config(&mut output, &archive)?;
        std::io::Write::flush(&mut output)
    }

    pub(super) fn load_route(file: &Path, board: &BoardData) -> Result<Self, Box<dyn Error>> {
        let archive = wml::parse(read_file(file)?)?;
        let mut children = archive.children.into_iter();
        let campaign = children.next().filter(|(tag, _)| tag == "campaign_document")
            .ok_or("Missing saved campaign document")?.1;
        let modifications = children.map(|(tag, node)| (tag == "modification").then_some(node)
            .ok_or("Invalid saved campaign modification")).collect::<Result<_, _>>()?;
        Ok(Self { campaign, maps: None, core: board.core.clone(), root: board.wesnoth_root.clone(),
            seed: board.seed, installed: Default::default(), mode: None, modifications })
    }

    async fn play_installed(&self, entry: &installed::Campaign, id: &str, selector: &mut selector::Selector<'_>) -> Result<(), Box<dyn Error>> {
        let descriptor = self.campaign.children.iter().find(|(tag, cfg)| tag == "campaign" && cfg.attribute("first_scenario") == Some(id))
            .map(|(_, cfg)| cfg).ok_or("Missing campaign descriptor")?;
        let Some(difficulty) = selector.difficulty(descriptor).await else { return Ok(()) };
        let mode = selector.combat_mode();
        let modifications = selector.modifications();
        let (root, selected) = (self.root.clone(), entry.clone());
        let worker = std::thread::spawn(move || crate::desktop::prepare_campaign(&root, &selected.folder, &selected.define, &difficulty));
        while !worker.is_finished() {
            selector.draw(Some("Loading campaign…"));
            set_default_camera();
            next_frame().await;
        }
        let file = worker.join().map_err(|_| "Campaign preparation stopped")??;
        let maps = self.root.join("data/campaigns").join(&entry.folder).join("maps");
        let mut game = Self::load(&file, maps, self.core.clone(), self.root.clone(), self.seed)?;
        game.mode = Some(mode);
        game.modifications = modifications;
        game.play_from(id.into()).await
    }

    async fn play_from(&self, id: String) -> Result<(), Box<dyn Error>> {
        let mut route = CampaignCarryover::default();
        let board = self.board(&id, &mut route)?;
        self.play_route(board, route, Players::Campaign).await
    }

    pub(super) async fn play_tutorial(mut self, mode: CombatMode, modifications: &[wml::Node]) -> Result<(), Box<dyn Error>> {
        self.mode = Some(mode);
        self.modifications = modifications.to_vec();
        let mut opening = campaign::select(&self.campaign, crate::tutorial::FIRST_SCENARIO, |file| self.read_map(file))?;
        mode.classify(&mut opening);
        let maps = self.maps.as_deref().ok_or("Missing Battle Training maps")?;
        let Some(hero) = crate::tutorial::choose_on_map(&opening, &self.core, maps, &self.root, self.seed).await? else { return Ok(()) };
        let dir = crate::display_preferences::config_directory()?;
        let mut locale = crate::title_screen::locale::Locale::load(&self.root, crate::title_screen::locale::saved(&dir)?)?;
        crate::tutorial::localize_round_hints(&mut self.campaign, &self.root, &mut locale);
        let lesson = crate::tutorial::prepare_campaign(&mut self.campaign, hero)?;
        let mut route = CampaignCarryover::default();
        let mut board = self.board(crate::tutorial::FIRST_SCENARIO, &mut route)?;
        board.lesson = Some(lesson);
        self.play_route(board, route, Players::Campaign).await
    }

    pub(super) async fn play_route(&self, mut board: BoardData, mut route: CampaignCarryover, players: Players) -> Result<(), Box<dyn Error>> {
        loop {
            let Some(next) = play_board(board, players, Some((self, &mut route))).await? else { return Ok(()) };
            board = self.board(&next, &mut route)?;
            next_frame().await;
        }
    }

    pub(super) async fn run(self) -> Result<(), Box<dyn Error>> {
        let scenarios: Vec<_> = campaign::scenarios(&self.campaign).collect();
        let mut selector = selector::Selector::new(&self.root, &self.campaign, &scenarios)?;
        selector.campaigns_only = !self.installed.is_empty();
        if selector.campaigns_only { selector.restore_modifications()?; }

        let mut error: Option<String> = None;
        loop {
            let action = selector.draw(error.as_deref());
            set_default_camera();
            if !matches!(action, ListAction::Idle) { selector.remember_modifications()?; }
            match action {
                ListAction::Choose(index) => {
                    let (id, _) = scenarios[index];
                    let result = if let Some(entry) = self.installed.get(id) {
                        if id == crate::tutorial::FIRST_SCENARIO {
                            next_frame().await;
                            crate::tutorial::run(&self.root, &self.core, self.seed, selector.combat_mode(), &selector.modifications()).await
                        } else {
                            self.play_installed(entry, id, &mut selector).await
                        }
                    } else {
                        next_frame().await;
                        self.play_from(id.to_owned()).await
                    };
                    error = result.err().map(|cause| format!("{id} — {cause}"));
                    while macroquad::prelude::get_char_pressed().is_some() {}
                }
                ListAction::Back => return Ok(()),
                ListAction::Idle => {}
            }
            next_frame().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_lite::hex::Hex;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn launching_the_chosen_scenario_uses_its_map_and_campaign_units() {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("rustnoth-browser-{}-{nonce}", std::process::id()));
        let maps = root.join("maps");
        fs::create_dir_all(&maps).unwrap();
        fs::create_dir_all(root.join("data/core/images")).unwrap();
        let file = root.join("campaign.cfg");
        fs::write(&file, b"[units]\n[unit_type]\nid=Captain\nhitpoints=19\n[/unit_type]\n[/units]\n\
            [scenario]\nid=first\nmap_file=missing.map\n[/scenario]\n\
            [scenario]\nid=second\nname=River Crossing\nmap_file=river.map\nturns=4\n\
            [story]\n[part]\nstory= _ \"<span color='#BAAC7D'>Winter &amp; snow.</span>\"\nbackground=story/winter.webp\n[/part]\n[/story]\n\
            [story]\n[part]\nshow_title=yes\n[background_layer]\nimage=maps/river.webp\nbase_layer=yes\n[/background_layer]\n[/part]\n[/story]\n\
            [side]\nid=hero\nsave_id=scouts\ntype=Captain\ncontroller=human\ngold=104\nincome=2\nvillage_gold=3\nexperience=13\n\
            [unit]\nid=scout\ntype=Captain\nx,y=2,1\nhitpoints=3\nexperience=9\nrecall_cost=11\n[status]\npoisoned=yes\n[/status]\n[/unit]\n\
            [unit]\ntype=Captain\nplacement=recall\nexperience=2\n[/unit]\n[/side]\n[side]\nid=guards\ntype=Captain\ngold=1000\n[/side]\n\
            [event]\nname=side 1 turn end\n[endlevel]\nnext_scenario=third\ncarryover_percentage=40\ncarryover_add=yes\nbonus=no\n[result]\nside=1\nbonus=yes\ncarryover_percentage=50\n[/result]\n[/endlevel]\n[/event]\n[/scenario]\n\
            [scenario]\nid=third\nname=The Harbor\nmap_file=river.map\n[side]\nid=guards\ntype=Captain\ngold=5\n[/side]\n[side]\nid=hero\nsave_id=scouts\ntype=Captain\ngold=20\n[/side]\n[event]\nname=prestart\n[gold]\nside=2\namount=2\n[/gold]\n[/event]\n[/scenario]").unwrap();
        fs::write(maps.join("river.map"), "Xv,Xv,Xv,Xv,Xv\nXv,1 Gg,Vh,2 Gg,Xv\nXv,Xv,Xv,Xv,Xv").unwrap();
        let mut browser = Browser::load(&file, maps, b"[terrain_type]\nstring=Vh\ngives_income=yes\n[/terrain_type]\n[units]\n[/units]".to_vec(), root.clone(), 9).unwrap();
        browser.mode = Some(CombatMode::Predictable);
        browser.modifications.push(wml::parse("[modification]\nid=grant\n[event]\nname=prestart\n[gold]\nside=2\namount=4\n[/gold]\n[/event]\n[/modification]").unwrap().children.remove(0).1);
        assert_eq!(campaign::scenarios(&browser.campaign).collect::<Vec<_>>(),
            [("first", ""), ("second", "River Crossing"), ("third", "The Harbor")]);
        let mut route = CampaignCarryover::default();
        assert!(browser.board("first", &mut route).is_err());
        let board = browser.board("second", &mut route).unwrap();
        assert_eq!(wml::parse(&board.source).unwrap().attribute("random_mode"), Some("deterministic"));
        assert_eq!(board.title, "River Crossing");
        assert_eq!(board.story.len(), 2);
        assert_eq!(board.story[0].text, "Winter & snow.");
        assert_eq!(board.story[1].title, "River Crossing");
        let game = board.scenario.start(board.seed, |_| unreachable!("selected map was embedded")).unwrap();
        assert_eq!(game.audit_snapshot().gold[1], 1004);
        let view = game.observation_for(1).unwrap();
        assert!(view.units.iter().any(|u| u.position == Hex { x: 0, y: 0 } && u.hitpoints == 19));
        let mut session = rustnoth_gui::session::Session::new(game);
        assert_eq!(session.next_scenario(), None);
        session.apply(engine_lite::fixed_match::Action::EndTurn).unwrap();
        session.store_campaign_survivors(&mut route);
        let successor = browser.board(session.next_scenario().unwrap(), &mut route).unwrap();
        assert_eq!(successor.seed, engine_lite::rng::ReplayRng::new(9).next_u32(),
            "1.18.8 seeds the next scenario with the next word after settlement");
        assert_eq!(wml::parse(&successor.source).unwrap().attribute("rustnoth_interactive_rng"), Some("yes"));
        assert_eq!(successor.title, "The Harbor");
        let mut next = successor.scenario.start_campaign(successor.seed, |_| unreachable!(), Some(&mut route)).unwrap();
        let snapshot = next.audit_snapshot();
        let hero = snapshot.units.iter().find(|u| u.side == 2).unwrap();
        assert_eq!((hero.experience, hero.hitpoints, hero.can_recruit), (13, 19, true));
        assert!(snapshot.reserves[0].is_empty());
        let veterans: Vec<_> = snapshot.reserves[1].iter().map(|u| (u.experience, u.hitpoints)).collect();
        assert_eq!(veterans, [(9, 19), (2, 19)]);
        // 104 gold + three turns at 7 (one unowned village + base and side income).
        // Half of 125 rounds to 63, added to 20 before the 2-gold prestart grant.
        // The selected modification grants side two another four gold on each map.
        assert_eq!(next.observation_for(2).unwrap().gold, Some(89));
        assert_eq!(next.observation_for(1).unwrap().gold, Some(5));
        next.step(engine_lite::fixed_match::Action::EndTurn).unwrap();
        assert_eq!(next.recall_info("scout").unwrap().cost, 11);
        assert_eq!(next.recall_info("Captain-1").unwrap().experience, 2);
        fs::remove_dir_all(root).unwrap();
    }
}
