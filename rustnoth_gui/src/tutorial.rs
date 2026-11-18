//! The opening movement and attack exercises of the installed Battle Training campaign.
use std::{error::Error, fs, path::Path};

use engine_lite::{campaign, fixed_match::Action, hex::Hex, wml::{self, Node}, wml_emit};
use macroquad::prelude::*;
use rustnoth_gui::{session::{Players, Session}, team_colors::TeamColors};

use crate::{BoardData, desktop, dialog, render, show_board};

mod opening;

pub(super) const FIRST_SCENARIO: &str = "tutorial";
const SCOPE: &str = "Opening exercises: movement and attacking";

pub(super) fn insert_entry(catalog: &mut Node, source: &str) -> Result<(), Box<dyn Error>> {
    // The stock campaign descriptor precedes the define which includes its scenarios and Lua.
    let metadata = source.split_once("#ifdef TUTORIAL").ok_or("missing tutorial campaign definition")?.0;
    let root = wml::parse(metadata)?;
    let (_, mut entry) = root.children.into_iter().find(|(tag, cfg)| tag == "campaign"
        && cfg.attribute("id") == Some("Tutorial") && cfg.attribute("first_scenario") == Some(FIRST_SCENARIO))
        .ok_or("missing Battle Training campaign entry")?;
    let desc = entry.attributes.entry("description".into()).or_default();
    desc.text.push_str("\n\n");
    desc.text.push_str(SCOPE);
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
enum Hero { Konrad, Lisar }

impl Hero {
    fn name(self) -> &'static str {
        match self { Self::Konrad => "Konrad", Self::Lisar => "Li’sar" }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Step { Approach, Attack, Complete }

pub(super) struct Lesson {
    step: Step,
    mentor: Hex,
    select_prompt: String,
    movement_prompt: String,
}

impl Lesson {
    fn new(mentor: Hex, hero: Hero) -> Self {
        Self { step: Step::Approach, mentor, select_prompt: format!("Click on {}", hero.name()),
            movement_prompt: format!("Move {} next to Delfador", hero.name()) }
    }

    pub(super) fn finished(&self) -> bool { self.step == Step::Complete }

    pub(super) fn draw_prompt(&self, selected: bool, font: Option<&Font>) {
        let prompt: &str = match self.step {
            Step::Approach if !selected => &self.select_prompt,
            Step::Approach => &self.movement_prompt,
            Step::Attack => "Attack the quintain",
            Step::Complete => "Opening exercises complete",
        };
        let view = render::board_viewport();
        let prompt = dialog::fit(font, prompt.into(), (view.width - 8.0).max(1.0), 18);
        let x = view.x + (view.width - measure_text(&prompt, font, 18, 1.0).width) * 0.5;
        let y = view.y + view.height - 24.0;
        dialog::text(font, &prompt, x + 1.0, y + 1.0, 18, BLACK);
        dialog::text(font, &prompt, x, y, 18, WHITE);
    }

    pub(super) fn restrict(&self, session: &mut Session<'_>) {
        // Wrong destinations would require the native undo action. Offer reachable goal hexes
        // on the first move so the learner can always finish on the current turn.
        session.retain_actions(|action| match (self.step, action) {
            (Step::Approach, Action::Move { to, .. }) => to.distance(self.mentor) == 1,
            (Step::Attack, Action::Attack(attack)) => attack.to == self.mentor,
            _ => false,
        });
    }

    pub(super) fn advance(&mut self, session: &mut Session<'_>) {
        self.step = match self.step { Step::Approach => Step::Attack, Step::Attack | Step::Complete => Step::Complete };
        self.restrict(session);
    }
}

pub(super) async fn run(root: &Path, core: &[u8], seed: u32) -> Result<(), Box<dyn Error>> {
    let form = dialog::Dialog::new(root);
    let Some(hero) = choose_character(root, &form).await? else { return Ok(()) };
    let installed = root.to_owned();
    let worker = std::thread::spawn(move || desktop::prepare_tutorial(&installed));
    while !worker.is_finished() {
        let frame = form.begin(110.0);
        dialog::text(form.font.as_ref(), "Loading Battle Training…", frame.panel.x + 24.0, frame.panel.y + 60.0, 20, dialog::PALE);
        set_default_camera();
        next_frame().await;
    }
    let file = worker.join().map_err(|_| "Tutorial preparation stopped")??;
    let definition = wml::parse(fs::read(file)?)?;
    let maps = root.join(desktop::TUTORIAL).join("maps");
    let mut selected = campaign::select(&definition, FIRST_SCENARIO, |file| fs::read_to_string(maps.join(file)))?;
    let (_, scenario) = selected.children.iter_mut().find(|(tag, _)| tag == "scenario").ok_or("missing tutorial scenario")?;
    let lesson = opening::prepare(scenario, hero)?;
    let mut bytes = Vec::new();
    wml_emit::write_config(&mut bytes, &selected)?;
    let mut board = BoardData::from_source(&bytes, core, maps, root.to_owned(), seed)?;
    board.lesson = Some(lesson);
    next_frame().await;
    show_board(board, Players::HotSeat).await
}

async fn choose_character(root: &Path, form: &dialog::Dialog) -> Result<Option<Hero>, Box<dyn Error>> {
    let images = root.join(desktop::TUTORIAL).join("images/units");
    let colors = TeamColors::parse(&fs::read(root.join("data/core/team-colors.cfg"))?)?;
    let sprites = [
        render::load_texture(&images.join("konrad-fighter.png"), None).ok_or("missing Konrad tutorial sprite")?,
        render::load_texture(&images.join("human-princess.png"), Some((&colors, 1))).ok_or("missing Li’sar tutorial sprite")?,
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
    assert_eq!(campaign::scenarios(&catalog).collect::<Vec<_>>(), [(FIRST_SCENARIO, SCOPE)]);
    assert_eq!(catalog.children[2].1.attribute("id"), Some("South_Guard"));
}
