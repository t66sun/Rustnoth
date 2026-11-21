//! Read the selected checkpoint once and project its preview through the playing side's view.
use super::*;
use crate::{BoardData, render};
use engine_lite::fixed_match::{MatchState, carryover::CampaignCarryover};
use rustnoth_gui::session::{Players, Session};
use std::time::{SystemTime, UNIX_EPOCH};

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

fn local_date(time: SystemTime) -> Option<libc::tm> {
    let stamp = libc::time_t::try_from(time.duration_since(UNIX_EPOCH).ok()?.as_secs()).ok()?;
    let mut out = std::mem::MaybeUninit::uninit();
    // Both functions write a complete tm to the supplied buffer on success.
    unsafe {
        #[cfg(windows)]
        let ok = libc::localtime_s(out.as_mut_ptr(), &stamp) == 0;
        #[cfg(not(windows))]
        let ok = !libc::localtime_r(&stamp, out.as_mut_ptr()).is_null();
        ok.then(|| out.assume_init())
    }
}

pub(super) struct Preview {
    pub data: BoardData,
    pub players: Players,
    pub map: Option<Texture2D>,
    pub replay: bool,
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
    pub(super) fn read(root: &Path, slot: &Slot) -> Result<Self, Box<dyn Error>> {
        let (data, players) = saved_game::load(slot, root)?;
        let restore = data.restore.as_ref().ok_or("Missing save record")?;
        let replay = restore.replay_roster.is_some();
        let mut roster = restore.replay_roster.as_ref().and_then(|r| CampaignCarryover::from_replay_roster(&r.replay_roster()));
        let game = data.scenario.start_campaign(data.seed, |_| {
            restore.map.clone().ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "Saved scenario is missing its external map"))
        }, roster.as_mut())?;
        let mut session = Session::with_players(game, players);
        if !replay {
            for event in &restore.events { event.apply(&mut session)?; }
        }
        let turn = match session.state() { MatchState::Playing(t) => Some(t.round), _ => None };
        let map = session.observation().and_then(|view| render::observed_preview(root, &data.assets, view, 260.));
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
        Ok(Self { data, players, map, replay, turn, leader })
    }
}
