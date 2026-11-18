use std::{error::Error, fs, path::{Path, PathBuf}};

use engine_lite::{campaign, wml, wml_emit};
use macroquad::prelude::{next_frame, set_default_camera};
use rustnoth_gui::session::Players;

use crate::{BoardData, dialog::{Dialog, ListAction, ListPosition}, read_file, show_board};

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

    fn board(&self, id: &str) -> Result<BoardData, Box<dyn Error>> {
        let selected = campaign::select(&self.campaign, id, |file| fs::read_to_string(self.maps.join(file)))?;
        let mut source = Vec::new();
        wml_emit::write_config(&mut source, &selected)?;
        BoardData::from_source(&source, &self.core, self.maps.clone(), self.root.clone(), self.seed)
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
                    let result = match self.board(id) {
                        Ok(data) => show_board(data, Players::HotSeat).await,
                        Err(cause) => Err(cause),
                    };
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
            [scenario]\nid=second\nname=River Crossing\nmap_file=river.map\n\
            [side]\ntype=Captain\n[/side]\n[side]\ntype=Captain\n[/side]\n[/scenario]").unwrap();
        fs::write(maps.join("river.map"), "Xv,Xv,Xv,Xv\nXv,1 Gg,2 Gg,Xv\nXv,Xv,Xv,Xv").unwrap();
        let browser = Browser::load(&file, maps, b"[units]\n[/units]".to_vec(), root.clone(), 9).unwrap();
        assert_eq!(campaign::scenarios(&browser.campaign).collect::<Vec<_>>(),
            [("first", ""), ("second", "River Crossing")]);
        assert!(browser.board("first").is_err());
        let board = browser.board("second").unwrap();
        assert_eq!(board.title, "River Crossing");
        let game = board.scenario.start(board.seed, |_| unreachable!("selected map was embedded")).unwrap();
        let view = game.observation_for(1).unwrap();
        assert!(view.units.iter().any(|u| u.position == Hex { x: 0, y: 0 } && u.hitpoints == 19));
        fs::remove_dir_all(root).unwrap();
    }
}
