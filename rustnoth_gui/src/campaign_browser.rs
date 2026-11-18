use std::{error::Error, fs, path::{Path, PathBuf}};

use engine_lite::{campaign, fixed_match::carryover::CampaignGold, wml, wml_emit};
use macroquad::prelude::{next_frame, set_default_camera};
use rustnoth_gui::session::Players;

use crate::{BoardData, dialog::{Dialog, ListAction, ListPosition}, read_file, play_board};

pub(super) struct Browser {
    campaign: wml::Node,
    maps: PathBuf,
    core: Vec<u8>,
    root: PathBuf,
    seed: u32,
}

impl Browser {
    pub(super) fn load(file: &Path, maps: PathBuf, core: Vec<u8>, root: PathBuf, seed: u32)
        -> Result<Self, Box<dyn Error>>
    {
        let campaign = wml::parse(read_file(file)?)?;
        if campaign::scenarios(&campaign).next().is_none() {
            return Err("campaign contains no scenarios with an ID".into());
        }
        Ok(Self { campaign, maps, core, root, seed })
    }

    fn board(&self, id: &str, gold: &mut CampaignGold) -> Result<BoardData, Box<dyn Error>> {
        let mut selected = campaign::select(&self.campaign, id, |file| fs::read_to_string(self.maps.join(file)))?;
        for (_, scenario) in selected.children.iter_mut().filter(|(tag, _)| tag == "scenario") {
            gold.transfer_to(scenario);
        }
        let mut source = Vec::new();
        wml_emit::write_config(&mut source, &selected)?;
        BoardData::from_source(&source, &self.core, self.maps.clone(), self.root.clone(), self.seed)
    }

    async fn play_from(&self, mut id: String) -> Result<(), Box<dyn Error>> {
        let mut gold = CampaignGold::default();
        loop {
            let board = self.board(&id, &mut gold)?;
            let Some(next) = play_board(board, Players::HotSeat, Some(&mut gold)).await? else { return Ok(()) };
            id = next;
            next_frame().await;
        }
    }

    pub(super) async fn run(self) -> Result<(), Box<dyn Error>> {
        let scenarios: Vec<_> = campaign::scenarios(&self.campaign).collect();
        let dialog = Dialog::new(&self.root);
        let mut position = ListPosition::default();
        let mut error: Option<String> = None;
        loop {
            let frame = dialog.begin(636.0);
            let action = dialog.choose(&frame, &mut position, scenarios.len(), None, "Campaign scenarios",
                error.as_deref(), |index| {
                    let (id, name) = scenarios[index];
                    if name.is_empty() { (id.to_owned(), None) }
                    else { (name.to_owned(), Some(id.to_owned())) }
                });
            set_default_camera();
            match action {
                ListAction::Choose(index) => {
                    position.cursor = index;
                    next_frame().await;
                    let (id, _) = scenarios[index];
                    let result = self.play_from(id.to_owned()).await;
                    error = result.err().map(|cause| format!("{id} — {cause}"));
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
            [side]\nid=hero\nsave_id=scouts\ntype=Captain\ncontroller=human\ngold=104\nincome=2\nvillage_gold=3\n[/side]\n[side]\nid=guards\ntype=Captain\ngold=1000\n[/side]\n\
            [event]\nname=side 1 turn end\n[endlevel]\nnext_scenario=third\ncarryover_percentage=40\ncarryover_add=yes\nbonus=no\n[result]\nside=1\nbonus=yes\ncarryover_percentage=50\n[/result]\n[/endlevel]\n[/event]\n[/scenario]\n\
            [scenario]\nid=third\nname=The Harbor\nmap_file=river.map\n[side]\nid=guards\ntype=Captain\ngold=5\n[/side]\n[side]\nid=scouts\ntype=Captain\ngold=20\n[/side]\n[event]\nname=prestart\n[gold]\nside=2\namount=2\n[/gold]\n[/event]\n[/scenario]").unwrap();
        fs::write(maps.join("river.map"), "Xv,Xv,Xv,Xv,Xv\nXv,1 Gg,Vh,2 Gg,Xv\nXv,Xv,Xv,Xv,Xv").unwrap();
        let browser = Browser::load(&file, maps, b"[terrain_type]\nstring=Vh\ngives_income=yes\n[/terrain_type]\n[units]\n[/units]".to_vec(), root.clone(), 9).unwrap();
        assert_eq!(campaign::scenarios(&browser.campaign).collect::<Vec<_>>(),
            [("first", ""), ("second", "River Crossing"), ("third", "The Harbor")]);
        let mut gold = CampaignGold::default();
        assert!(browser.board("first", &mut gold).is_err());
        let board = browser.board("second", &mut gold).unwrap();
        assert_eq!(board.title, "River Crossing");
        assert_eq!(board.story.len(), 2);
        assert_eq!(board.story[0].text, "Winter & snow.");
        assert_eq!(board.story[1].title, "River Crossing");
        let game = board.scenario.start(board.seed, |_| unreachable!("selected map was embedded")).unwrap();
        let view = game.observation_for(1).unwrap();
        assert!(view.units.iter().any(|u| u.position == Hex { x: 0, y: 0 } && u.hitpoints == 19));
        let mut session = rustnoth_gui::session::Session::new(game);
        assert_eq!(session.next_scenario(), None);
        session.apply(engine_lite::fixed_match::Action::EndTurn).unwrap();
        session.store_campaign_gold(&mut gold);
        let successor = browser.board(session.next_scenario().unwrap(), &mut gold).unwrap();
        assert_eq!(successor.title, "The Harbor");
        let next = successor.scenario.start(successor.seed, |_| unreachable!()).unwrap();
        // 104 gold + three turns at 7 (one unowned village + base and side income).
        // Half of 125 rounds to 63, added to 20 before the 2-gold prestart grant.
        assert_eq!(next.observation_for(2).unwrap().gold, Some(85));
        assert_eq!(next.observation_for(1).unwrap().gold, Some(5));
        fs::remove_dir_all(root).unwrap();
    }
}
