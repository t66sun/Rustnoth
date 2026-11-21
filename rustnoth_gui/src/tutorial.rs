//! The opening movement and attack exercises of the installed Battle Training campaign.
use std::{error::Error, fs, path::Path};

use engine_lite::{fixed_match::Action, hex::Hex, wml::{self, Node}};
use macroquad::prelude::*;
use rustnoth_gui::{save_replay::Event, session::Session, team_colors::TeamColors};

use crate::{desktop, dialog, render};

mod opening;
mod healing;
mod recruiting;
mod end_turn;
mod weapons;
mod practice;
mod advice;
mod ending;
mod second;
pub(super) use ending::prepare_campaign;

pub(super) const FIRST_SCENARIO: &str = "tutorial";
const SCOPE: &str = "Movement, attacking, village healing and recruiting";

pub(super) fn insert_entry(catalog: &mut Node, source: &str) -> Result<(), Box<dyn Error>> {
    // The stock campaign descriptor precedes the define which includes its scenarios and Lua.
    let metadata = source.split_once("#ifdef TUTORIAL").ok_or("missing tutorial campaign definition")?.0;
    let root = wml::parse(metadata)?;
    let (_, entry) = root.children.into_iter().find(|(tag, cfg)| tag == "campaign"
        && cfg.attribute("id") == Some("Tutorial") && cfg.attribute("first_scenario") == Some(FIRST_SCENARIO))
        .ok_or("missing Battle Training campaign entry")?;
    let mut level = Node::default();
    put(&mut level, "id", FIRST_SCENARIO);
    put(&mut level, "name", SCOPE);
    catalog.children.insert(0, ("campaign".into(), entry));
    catalog.children.insert(1, ("scenario".into(), level));
    Ok(())
}

fn put(cfg: &mut Node, key: &str, value: &str) {
    cfg.attributes.insert(key.into(), wml::Value { text: value.into(), ..Default::default() });
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Hero { Konrad, Lisar }

impl Hero {
    fn name(self) -> &'static str {
        match self { Self::Konrad => "Konrad", Self::Lisar => "LiÃ¢â‚¬â„¢sar" }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Step { Approach, Attack, EndFirst, WaitSecond, ToVillage, EndSecond, WaitThird, ToKeep, Recruit, EndThird, WaitFourth, FighterAttack, Practice, Complete }

pub(super) struct Lesson {
    end_turn_reasons: end_turn::Reasons,
    step: Step,
    mentor: Hex,
    select_prompt: String,
    movement_prompt: String,
    undo_prompt: String,
    show_undo_prompt: bool,
    selection_hint: Option<Node>,
    healing: healing::Exercises,
    recruiting: recruiting::Exercise,
    weapons: weapons::Exercise,
    advice: advice::Advice,
    before_healing: Option<i32>,
}

impl Lesson {
    fn new(mentor: Hex, hero: Hero, undo_prompt: String, healing: healing::Exercises, recruiting: recruiting::Exercise, end_turn_reasons: end_turn::Reasons, weapons: weapons::Exercise) -> Self {
        Self { end_turn_reasons, step: Step::Approach, mentor, select_prompt: format!("Click on {}", hero.name()),
            movement_prompt: format!("Move {} next to Delfador", hero.name()), undo_prompt, show_undo_prompt: false, selection_hint: None, healing, recruiting, weapons, advice: Default::default(), before_healing: None }
    }

    #[cfg(test)]
    pub(super) fn finished(&self) -> bool { self.step == Step::Complete }

    pub(super) fn selected(&mut self, at: Hex, session: &mut Session<'_>) {
        self.weapons.selected(at, session);
    }

    pub(super) fn weapon_hint(&self, session: &Session<'_>, action: &Action) -> Option<usize> {
        if !matches!(self.step, Step::FighterAttack | Step::Practice) { return None; }
        self.weapons.weapon(session, action)
    }

    pub(super) fn explain_weapon(&mut self, hint: usize, from: Hex, session: &mut Session<'_>) {
        self.weapons.explain(hint, from, session);
    }

    pub(super) fn select_student(&mut self, session: &mut Session<'_>) -> bool {
        if self.step == Step::Approach && let Some(line) = self.selection_hint.take() {
            session.present_message(&line);
            return true;
        }
        false
    }

    pub(super) fn draw_labels(&self, art: &render::BoardArt, layout: Option<&rustnoth_gui::layout::BoardLayout>) {
        let Some(layout) = layout else { return };
        for (hex, text, in_fog) in self.healing.markers() {
            art.draw_map_label(layout, hex, text, false, in_fog);
        }
        if matches!(self.step, Step::ToKeep | Step::Recruit | Step::EndThird) {
            art.draw_map_label(layout, self.recruiting.keep, &self.recruiting.label, self.step == Step::ToKeep, true);
        }
    }

    pub(super) fn prompt(&self, selected: bool) -> Option<&str> {
        Some(match self.step {
            Step::Approach if self.show_undo_prompt => &self.undo_prompt,
            Step::Approach if !selected => &self.select_prompt,
            Step::Approach => &self.movement_prompt,
            Step::Attack => "Attack the quintain",
            Step::EndFirst | Step::EndSecond | Step::EndThird => &self.healing.end_prompt,
            Step::ToVillage => &self.healing.village_prompt,
            Step::ToKeep => &self.recruiting.keep_prompt,
            Step::Recruit => &self.recruiting.recruit_prompt,
            Step::FighterAttack | Step::Practice if self.weapons.prompting => &self.weapons.prompt,
            Step::WaitSecond | Step::WaitThird | Step::WaitFourth | Step::FighterAttack | Step::Practice | Step::Complete => return None,
        })
    }

    pub(super) fn restrict(&self, session: &mut Session<'_>) {
        session.retain_actions(|action| match (self.step, action) {
            (Step::Approach, Action::Move { .. }) => true,
            (Step::Attack, Action::Attack(attack)) => attack.to == self.mentor,
            (Step::EndFirst | Step::EndSecond, Action::EndTurn) => true,
            (Step::ToVillage, Action::Move { to, .. }) => self.healing.villages.contains(to),
            (Step::ToKeep, Action::Move { to, .. }) => *to == self.recruiting.keep,
            (Step::ToKeep | Step::Recruit, Action::Attack(_)) => true,
            (Step::Recruit, Action::Recruit { .. }) | (Step::EndThird, _) => true,
            (Step::FighterAttack, action) => !matches!(action, Action::EndTurn),
            (Step::Practice, _) => true,
            _ => false,
        });
    }

    pub(super) fn advance(&mut self, session: &mut Session<'_>) {
        if practice::guardians_have_arrived(session) {
            self.step = Step::Practice;
        }
        self.step = match self.step {
            Step::Approach => {
                if matches!(session.history().last(), Some(Event::Player(Action::Move { to, .. }, _, _)) if to.distance(self.mentor) == 1) {
                    Step::Attack
                } else {
                    self.show_undo_prompt = !session.legal_actions().iter().any(|action|
                        matches!(action, Action::Move { to, .. } if to.distance(self.mentor) == 1));
                    Step::Approach
                }
            }
            Step::Attack => {
                healing::say(&self.healing.after_attack, session, None);
                Step::EndFirst
            }
            Step::EndFirst => Step::WaitSecond,
            Step::ToVillage => {
                healing::say(&self.healing.capture, session, None);
                Step::EndSecond
            }
            Step::EndSecond => {
                self.before_healing = healing::student_hp(session);
                Step::WaitThird
            }
            Step::ToKeep | Step::Recruit if matches!(session.history().last(),
                Some(rustnoth_gui::save_replay::Event::Player(Action::Attack(_), ..))) => Step::EndThird,
            Step::ToKeep => {
                session.present_message(&self.recruiting.instruction);
                Step::Recruit
            }
            Step::Recruit if self.recruiting.recruited(session) => {
                session.present_message(&self.recruiting.crowns);
                Step::EndThird
            }
            Step::EndThird if matches!(session.history().last(),
                Some(rustnoth_gui::save_replay::Event::Player(Action::EndTurn, ..))) => Step::WaitFourth,
            Step::FighterAttack if matches!(session.history().last(),
                Some(rustnoth_gui::save_replay::Event::Player(Action::Attack(_), ..))) => Step::Practice,
            other => other,
        };
        self.after_computer(session);
    }

    pub(super) fn after_computer(&mut self, session: &mut Session<'_>) {
        use engine_lite::fixed_match::MatchState;
        if practice::guardians_have_arrived(session) {
            self.step = Step::Practice;
        }
        self.advice.begin_turn(session);
        match session.state() {
            MatchState::Playing(turn) if turn.side != 1 => {
                if self.step == Step::WaitThird { self.before_healing = healing::student_hp(session); }
            }
            MatchState::Playing(turn) if turn.round == 2 && self.step == Step::WaitSecond => {
                self.healing.marked = true;
                healing::say(&self.healing.second_turn, session, None);
                self.step = Step::ToVillage;
            }
            MatchState::Playing(turn) if turn.round == 3 && self.step == Step::WaitThird => {
                healing::say(&self.healing.healed, session, self.before_healing);
                self.step = Step::ToKeep;
            }
            MatchState::Playing(turn) if turn.round == 4 && self.step == Step::WaitFourth => {
                self.step = if self.weapons.start(session) { Step::FighterAttack } else { Step::Practice };
            }
            MatchState::Playing(turn) if turn.round >= 5 && self.step == Step::Practice => {
                self.weapons.prompting = false;
            }
            MatchState::Playing(_) => {}
            _ => self.step = Step::Complete,
        }
        self.restrict(session);
    }

    pub(super) fn undo_from(&self, session: &Session<'_>) -> Option<Hex> {
        if self.step == Step::Approach && session.dialogue().is_none()
            && let Some(Event::Player(Action::Move { from, .. }, _, _)) = session.history().last() {
            Some(*from)
        } else {
            None
        }
    }

    pub(super) fn undo<'rules>(&mut self, session: &mut Session<'rules>,
        restart: impl FnOnce() -> Result<Session<'rules>, Box<dyn Error>>,
    ) -> Result<Option<Hex>, Box<dyn Error>> {
        let Some(from) = self.undo_from(session) else { return Ok(None) };
        // The opening exercise precedes combat. Replay its accepted prefix so village
        // ownership, movement points and event state all come from the same referee.
        let mut restored = restart()?;
        for event in &session.history()[..session.history().len() - 1] {
            event.apply(&mut restored)?;
        }
        self.restrict(&mut restored);
        *session = restored;
        self.show_undo_prompt = false;
        Ok(Some(from))
    }
}

pub(super) fn draw_prompt(prompt: &str, font: Option<&Font>) {
    let view = render::board_viewport();
    let prompt = dialog::fit(font, prompt.into(), (view.width - 8.0).max(1.0), 18);
    let x = view.x + (view.width - measure_text(&prompt, font, 18, 1.0).width) * 0.5;
    let y = view.y + view.height - 24.0;
    dialog::text(font, &prompt, x + 1.0, y + 1.0, 18, BLACK);
    dialog::text(font, &prompt, x, y, 18, WHITE);
}

pub(super) async fn run(root: &Path, core: &[u8], seed: u32, mode: crate::campaign_browser::CombatMode, modifications: &[Node]) -> Result<(), Box<dyn Error>> {
    let form = dialog::Dialog::new(root);
    let Some(hero) = choose_character(root, &form).await? else { return Ok(()) };
    let installed = root.to_owned();
    let worker = std::thread::spawn(move || desktop::prepare_tutorial(&installed));
    loop {
        let frame = form.begin(110.0);
        dialog::text(form.font.as_ref(), "Loading Battle TrainingÃ¢â‚¬Â¦", frame.panel.x + 24.0, frame.panel.y + 60.0, 20, dialog::PALE);
        set_default_camera();
        next_frame().await;
        if worker.is_finished() { break; }
    }
    let file = worker.join().map_err(|_| "Tutorial preparation stopped")??;
    let maps = root.join(desktop::TUTORIAL).join("maps");
    let browser = crate::campaign_browser::Browser::load(&file, maps, core.to_vec(), root.to_owned(), seed)?;
    browser.play_tutorial(hero, mode, modifications).await
}

async fn choose_character(root: &Path, form: &dialog::Dialog) -> Result<Option<Hero>, Box<dyn Error>> {
    let images = root.join(desktop::TUTORIAL).join("images/units");
    let colors = TeamColors::parse(&fs::read(root.join("data/core/team-colors.cfg"))?)?;
    let sprites = [
        render::load_texture(&images.join("konrad-fighter.png"), None).ok_or("missing Konrad tutorial sprite")?,
        render::load_texture(&images.join("human-princess.png"), Some((&colors, 1))).ok_or("missing LiÃ¢â‚¬â„¢sar tutorial sprite")?,
    ];
    let heroes = [Hero::Konrad, Hero::Lisar];
    let mut focus = 0;
    let mut pressed = None;
    let mut previous_pointer = mouse_position();
    loop {
        let frame = form.begin(246.0);
        let panel = frame.panel;
        let font = form.font.as_ref();
        let center = panel.x + panel.w * 0.5;
        dialog::text(font, "Select Character", panel.x + 22.0, panel.y + 34.0, 24, dialog::GOLD);
        dialog::text(font, "Who do you want to play as?", panel.x + 22.0, panel.y + 70.0, 18, dialog::PALE);
        let buttons = [Rect::new(center - 141.0, panel.y + 173.0, 132.0, 40.0),
            Rect::new(center + 9.0, panel.y + 173.0, 132.0, 40.0)];
        let hovered = buttons.iter().position(|b| b.contains(frame.mouse));
        if previous_pointer != mouse_position() {
            if let Some(index) = hovered { focus = index; }
            previous_pointer = mouse_position();
        }
        if [KeyCode::Left, KeyCode::Right, KeyCode::Tab].into_iter().any(is_key_pressed) { focus = 1 - focus; }
        if frame.clicked { pressed = hovered; }
        let clicked = if is_mouse_button_released(MouseButton::Left) { pressed.take().filter(|i| Some(*i) == hovered) } else { None };
        for (index, bounds) in buttons.into_iter().enumerate() {
            let cx = bounds.x + bounds.w * 0.5;
            draw_texture(&sprites[index], cx - sprites[index].width() * 0.5, bounds.y - sprites[index].height(), WHITE);
            dialog::button(bounds, hovered == Some(index), index == focus);
            let label = heroes[index].name();
            let width = measure_text(label, font, 18, 1.0).width;
            dialog::text(font, label, cx - width * 0.5, bounds.y + 26.0, 18, dialog::PALE);
        }
        set_default_camera();
        if is_key_pressed(KeyCode::Escape) { return Ok(None); }
        if frame.confirm { return Ok(Some(heroes[focus])); }
        if let Some(index) = clicked { return Ok(Some(heroes[index])); }
        next_frame().await;
    }
}

#[test]
fn installed_campaign_picker_places_battle_training_first_with_its_native_artwork() {
    let mut catalog = wml::parse("[campaign]\nid=South_Guard\n[/campaign]").unwrap();
    insert_entry(&mut catalog, "[campaign]\nid=Tutorial\nname=Battle Training\nfirst_scenario=tutorial\n\
        image=data/campaigns/tutorial/images/campaign_image.png\n[/campaign]\n#ifdef TUTORIAL\n{campaigns/tutorial/scenarios}\n#endif").unwrap();
    let (tag, first) = &catalog.children[0];
    assert_eq!(tag, "campaign");
    assert_eq!(first.attribute("name"), Some("Battle Training"));
    assert_eq!(first.attribute("image"), Some("data/campaigns/tutorial/images/campaign_image.png"));
    assert_eq!(engine_lite::campaign::scenarios(&catalog).collect::<Vec<_>>(), [(FIRST_SCENARIO, SCOPE)]);
    assert_eq!(catalog.children[2].1.attribute("id"), Some("South_Guard"));
}
