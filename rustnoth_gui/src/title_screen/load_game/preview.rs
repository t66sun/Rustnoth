//! Read the selected checkpoint once and project its preview through the playing side's view.
use super::*;
use crate::{BoardData, render};
use engine_lite::fixed_match::{MatchState, carryover::CampaignCarryover};
use rustnoth_gui::session::{Players, Session};
use crate::desktop::local_date;
use std::time::SystemTime;
use crate::title_screen::locale::Locale;
use engine_lite::wml::Node;

fn mode_label(locale: &mut Locale, root: &Path, players: Players, campaign: Option<&Node>) -> String {
    if let Some(descriptor) = campaign {
        let name = locale.attribute(root, descriptor, "name", descriptor.attribute("id").unwrap_or(""));
        if !name.is_empty() {
            return locale.ui("Campaign: $campaign_name").replace("$campaign_name", &name);
        }
    }
    locale.ui(if players == Players::Campaign { "Campaign" } else { "Multiplayer" }).to_owned()
}

#[derive(Default)]
pub(super) struct Date {
    pub short: String,
    pub long: String,
}
impl Date {
    pub(super) fn for_slot(slot: &Slot, now: SystemTime) -> Self {
        let modified = std::fs::metadata(slot.directory.join("name.txt")).and_then(|m| m.modified());
        let Some(time) = modified.ok().and_then(local_date) else { return Self::default() };
        let long = format!("{:04}-{:02}-{:02} {:02}:{:02}", time.tm_year + 1900, time.tm_mon + 1, time.tm_mday, time.tm_hour, time.tm_min);
        let today = local_date(now).is_some_and(|n| n.tm_year == time.tm_year && n.tm_yday == time.tm_yday);
        let short = if today { format!("{:02}:{:02}", time.tm_hour, time.tm_min) } else { long[..10].into() };
        Self { short, long }
    }
}

pub(super) struct Preview {
    pub data: BoardData,
    pub players: Players,
    pub map: Option<Texture2D>,
    pub replay: bool,
    pub mode_label: String,
    pub turn: Option<i32>,
    pub leader: Option<Leader>,
}
pub(super) struct Leader {
    pub image: Option<Texture2D>,
    pub name: String,
    pub gold: Option<i64>,
    pub active: usize,
    pub reserve: Option<usize>,
}

impl Preview {
    pub(super) fn read(root: &Path, slot: &Slot, locale: &mut Locale) -> Result<Self, Box<dyn Error>> {
        let (data, players) = saved_game::load(slot, root)?;
        let restore = data.restore.as_ref().ok_or("Missing save record")?;
        let replay = restore.is_replay();
        let mode_label = mode_label(locale, root, players,
            restore.campaign.as_ref().and_then(|route| route.saved_descriptor()));
        let mut roster = restore.replay_roster.as_ref().and_then(|r| CampaignCarryover::from_replay_roster(&r.replay_roster()));
        let game = data.scenario.start_with_controllers(data.seed, |_| {
            restore.map.clone().ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "Saved scenario is missing its external map"))
        }, roster.as_mut(), players == Players::Campaign)?;
        let mut session = Session::with_players(game, players);
        if !replay {
            for event in &restore.events { event.apply(&mut session)?; }
        }
        let turn = match session.state() { MatchState::Playing(t) => Some(t.round), _ => None };
        let map = session.observation().and_then(|view| render::save_preview(&data.assets, view, 260.));
        let leader = session.observation().and_then(|view| {
            let unit = view.units.iter().find(|unit| unit.side == view.side && unit.can_recruit)?;
            let image = data.assets.unit_image(&unit.type_id).and_then(|p| render::load_texture(&p, None));
            let name = session.unit_report(unit.position, unit.position).and_then(|r| r.name)
                .unwrap_or_else(|| data.assets.unit_name(&unit.type_id).unwrap_or(&unit.type_id).to_owned());
            Some(Leader { image, name, gold: view.gold,
                active: view.units.iter().filter(|u| u.side == view.side).count(),
                reserve: (!session.legal_actions().is_empty()).then(|| session.legal_actions().iter()
                    .filter(|action| matches!(action, engine_lite::fixed_match::Action::Disband { .. })).count()) })
        });
        Ok(Self { data, players, map, replay, mode_label, turn, leader })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::campaign_browser::Browser;

    #[test]
    fn checkpoint_type_identifies_the_saved_campaign_and_its_tutorial() {
        let doc = engine_lite::wml::parse(b"[campaign]\nname=The South Guard\n[/campaign]").unwrap();
        let descriptor = &doc.children[0].1;
        let mut locale = Locale::default();
        let mut label = |players, campaign| mode_label(&mut locale, Path::new(""), players, campaign);
        assert_eq!(label(Players::Campaign, Some(descriptor)), "Campaign: The South Guard");
        let training = engine_lite::wml::parse(b"[campaign]\nid=Tutorial\nname=Battle Training\n[/campaign]").unwrap();
        assert_eq!(label(Players::Campaign, Some(&training.children[0].1)), "Campaign: Battle Training");
        assert_eq!(label(Players::Campaign, None), "Campaign");
        assert_eq!(label(Players::HotSeat, None), "Multiplayer");
        assert_eq!(label(Players::VersusComputer, None), "Multiplayer");
    }

    #[test]
    fn reopened_campaign_keeps_its_name_in_the_selected_language() {
        let root = std::env::temp_dir().join(format!("rustnoth-preview-name-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let file = root.join("original.cfg");
        fs::write(&file, b"#textdomain wesnoth-tsg\n[campaign]\nid=SG\nname= _ \"The South Guard\"\n[/campaign]\n[scenario]\nid=opening\n[/scenario]").unwrap();
        let route = Browser::load(&file, root.clone(), Vec::new(), root.clone(), 5489).unwrap();
        let saved = root.join("campaign.cfg");
        route.save_route(&saved).unwrap();
        let archive = engine_lite::wml::parse(fs::read(saved).unwrap()).unwrap();
        let descriptor = archive.children[0].1.children.iter().find(|(tag, _)| tag == "campaign").unwrap();
        let source = "The South Guard";
        let target = "南疆卫队";
        let catalogs = root.join("translations/zh_CN/LC_MESSAGES");
        fs::create_dir_all(&catalogs).unwrap();
        let words = [0x950412de_u32, 0, 1, 28, 36, 0, 0, source.len() as u32, 44,
            target.len() as u32, 45 + source.len() as u32];
        let mut mo: Vec<_> = words.into_iter().flat_map(u32::to_le_bytes).collect();
        for message in [source, target] { mo.extend(message.bytes()); mo.push(0); }
        fs::write(catalogs.join("wesnoth-tsg.mo"), mo).unwrap();
        let mut locale = Locale::load(&root, "zh_CN".into()).unwrap();
        assert_eq!(mode_label(&mut locale, &root, Players::Campaign, Some(&descriptor.1)), "Campaign: 南疆卫队");
        fs::remove_dir_all(root).unwrap();
    }
}
