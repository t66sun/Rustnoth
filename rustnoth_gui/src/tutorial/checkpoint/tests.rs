use super::*;
use crate::{campaign_browser::Browser, saved_game::{self, CampaignSave}, tutorial::opening::tests::{opening, training_core}};
use engine_lite::{fixed_match::{Action, carryover::CampaignCarryover}, wml, wml_emit};
use rustnoth_gui::session::Players;
use std::{fs, time::{SystemTime, UNIX_EPOCH}};

#[test]
fn a_manual_training_save_resumes_the_exercise_and_carries_its_veteran_to_part_two() {
    for hero in [Hero::Konrad, Hero::Lisar] {
        let mut first = opening();
        put(&mut first, "next_scenario", "training_next");
        let side = first.children.iter_mut().find(|(tag, cfg)| tag == "side" && cfg.attribute("id") == Some("student")).unwrap();
        side.1.children.push(("unit".into(), wml::parse("type=Elvish Fighter\nid=veteran\nx,y=6,2\nexperience=11").unwrap()));
        let mut second = wml::parse("id=training_next\nvictory_when_enemies_defeated=no\n\
            [side]\nid=student\ntype=Fighter\ncontroller=human\n[/side]\n\
            [side]\nid=orc\nno_leader=yes\ndefeat_condition=never\n[/side]").unwrap();
        put(&mut second, "map_data", first.attribute("map_data").unwrap());
        let mut campaign = Node::default();
        campaign.children = vec![("scenario".into(), first), ("scenario".into(), second)];
        let mut lesson = crate::tutorial::prepare_campaign(&mut campaign, hero).unwrap();
        // Finish the fixture at the first strike so the same resumed route enters Part II.
        campaign.children[0].1.children.extend(wml::parse("[event]\nname=attack_end\n[endlevel]\nresult=victory\nnext_scenario=training_next\n[/endlevel]\n[/event]").unwrap().children);

        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("rustnoth-training-resume-{nonce}"));
        fs::create_dir_all(root.join("data/core/images")).unwrap();
        let original = root.join("training.cfg");
        wml_emit::write_config(fs::File::create(&original).unwrap(), &campaign).unwrap();
        let browser = Browser::load(&original, root.clone(), training_core().into_bytes(), root.clone(), 9).unwrap();
        let mut route = CampaignCarryover::default();
        let mut board = browser.board("tutorial", &mut route.gold).unwrap();
        let roster = route.replay_roster();
        let mut live = Session::with_players(board.scenario.start_with_controllers(9, |_| unreachable!(), Some(&mut route), true).unwrap(), Players::Campaign);
        live.dismiss_dialogue();
        let origin = live.observation().unwrap().units.iter().find(|unit| unit.side == 1 && unit.can_recruit).unwrap().position;
        lesson.restrict(&mut live);
        let movement = live.legal_actions().iter().find(|action| matches!(action, Action::Move { from, to }
            if *from == origin && to.distance(lesson.mentor) == 1)).unwrap().clone();
        live.apply(movement).unwrap();
        lesson.advance(&mut live);
        while live.dialogue().is_some() { live.dismiss_dialogue(); }
        assert_eq!(live.tutorial_hint(), if hero == Hero::Konrad { "Attack with Konrad" } else { "Attack with Lisar" });
        board.lesson = Some(lesson);
        let saves = root.join("saves");
        saved_game::save(&saves, "First strike", &board, Players::Campaign, None, live.history(), None, None,
            Some(CampaignSave::Checkpoint(&browser, &roster, live.tutorial_hint()))).unwrap();
        fs::remove_file(original).unwrap();

        let (mut restored, players) = saved_game::load(&saved_game::slots(&saves).unwrap()[0], &root).unwrap();
        let saved = restored.restore.take().unwrap();
        let mut route = saved.replay_roster.unwrap();
        let mut resumed = Session::with_players(restored.scenario.start_with_controllers(restored.seed, |_| unreachable!(), Some(&mut route), true).unwrap(), players);
        let mut lesson = restored.lesson.unwrap();
        lesson.resume(&mut resumed, &saved.events, saved.tutorial_hints.unwrap()).unwrap();
        assert_eq!(resumed.observation(), live.observation());
        assert_eq!(resumed.history(), live.history());
        assert_eq!(resumed.tutorial_hint(), live.tutorial_hint());
        assert_eq!(resumed.legal_actions(), live.legal_actions());
        assert_eq!(lesson.prompt(true), Some("Attack the quintain"));
        assert!(resumed.dialogue().is_none());
        let attack = resumed.legal_actions().iter().find(|a| matches!(a, Action::Attack(_))).unwrap().clone();
        resumed.apply(attack).unwrap();
        lesson.advance(&mut resumed);
        while resumed.dialogue().is_some() { resumed.dismiss_dialogue(); }
        assert_eq!(resumed.next_scenario(), Some("training_next"));
        resumed.store_campaign_survivors(&mut route);
        let successor = saved.campaign.unwrap().board("training_next", &mut route.gold).unwrap();
        let next = Session::with_players(successor.scenario.start_with_controllers(9, |_| unreachable!(), Some(&mut route), true).unwrap(), players);
        let student = next.observation().unwrap().units.iter().find(|unit| unit.side == 1 && unit.can_recruit).unwrap();
        assert_eq!(student.type_id, if hero == Hero::Konrad { "Fighter" } else { "Fighteress" });
        assert_eq!(next.recall_info("veteran").unwrap().experience, 11);
        fs::remove_dir_all(root).unwrap();
    }
}
