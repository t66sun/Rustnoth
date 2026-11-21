use std::{
    env,
    error::Error,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
};

use engine_lite::{
    fixed_match::Action, hex::Hex, observation::Observation, scenario_bundle::Bundle, wml,
};
use macroquad::{Window, miniquad::conf::Conf, prelude::*};
use rustnoth_gui::{assets::AssetCatalog, choices::options_at, selection::Selection, session::{Players, Session}};

use camera::BoardCamera;

mod camera;
mod campaign_browser;
mod list_scroll;
mod desktop;
mod editor;
mod dialog;
mod display_preferences;
mod end_turn;
mod menu;
mod recruit_dialog;
mod map_menus;
mod render;
mod setup;
mod story;
mod saved_game;
mod replay_viewer;
mod save_dialog;
mod unit_list;
mod scenario_report;
mod screenshot;
mod title_screen;
mod tutorial;
mod artwork;

const USAGE: &str = "rustnoth_gui [--wesnoth-root <directory>]\n\
rustnoth_gui <bundle.cfg> <core.cfg> <seed> [--wesnoth-root <directory>]\n\
rustnoth_gui --campaign <campaign.cfg> <maps-dir> <core.cfg> <seed> [--wesnoth-root <directory>]\n\
rustnoth_gui --skirmish <core.cfg> <seed> [--wesnoth-root <directory>]\n\
rustnoth_gui --title [<campaign.cfg> <maps-dir>] <core.cfg> <seed> [--wesnoth-root <directory>]\n\
rustnoth_gui --screenshot <map> <output.png> [--wesnoth-root <directory>]\n\
Scroll over the board to zoom. Pan with a middle-button drag or the arrow keys.\n\
Press Home to fit the board.";

fn main() -> ExitCode {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if let [flag] = args.as_slice()
        && (flag == "--help" || flag == "-h")
    {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    if args.first().is_some_and(|flag| flag == "--screenshot") {
        let (map, output, explicit) = match &args[1..] {
            [map, output] => (map, output, None),
            [map, output, flag, root] if flag == "--wesnoth-root" => (map, output, Some(Path::new(root))),
            _ => { eprintln!("{USAGE}"); return ExitCode::FAILURE; }
        };
        let result = desktop::locate(explicit).map_err(Box::<dyn Error>::from)
            .and_then(|root| render::map_png(&root, Path::new(map), Path::new(output)));
        return match result {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => { eprintln!("{err}"); ExitCode::FAILURE }
        };
    }
    let launch = match Launch::from_args(&args) {
        Ok(data) => data,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let fullscreen = display_preferences::load().unwrap_or_else(|err| {
        eprintln!("Could not read the Rustnoth display preference: {err}");
        false
    });
    let configuration = Conf {
        fullscreen,
        window_title: format!("Rustnoth - {}", launch.title()),
        window_width: 1200,
        window_height: 820,
        window_resizable: true,
        ..Conf::default()
    };
    Window::from_config(configuration, async move {
        macroquad::experimental::coroutines::start_coroutine(screenshot::listen());
        let result = match launch {
            Launch::Title(title) => title.run(fullscreen).await,
            Launch::Campaign(browser) => browser.run().await,
            Launch::Board(data) => show_board(data, Players::HotSeat).await,
            Launch::Skirmish(setup) => setup.run().await,
        };
        if let Err(error) = result {
            eprintln!("{error}");
            std::process::exit(1);
        }
    });
    ExitCode::SUCCESS
}

#[derive(Debug, Eq, PartialEq)]
struct Options {
    source: Source,
    core: PathBuf,
    seed: u32,
    wesnoth_root: PathBuf,
}

#[derive(Debug, Eq, PartialEq)]
enum Source {
    Title(Option<(PathBuf, PathBuf)>),
    Campaign { file: PathBuf, maps: PathBuf },
    Bundle(PathBuf),
    Skirmish,
}

impl Options {
    fn parse(args: &[OsString]) -> Result<Self, Box<dyn Error>> {
        let (args, root) = match args {
            [rest @ .., flag, root] if flag == "--wesnoth-root" => (rest, PathBuf::from(root)),
            _ => (args, PathBuf::from("../wesnoth_src")),
        };
        let (source, core, seed) = match args {
            [flag, file, maps, core, seed] if flag == "--title" => {
                (Source::Title(Some((file.into(), maps.into()))), core, seed)
            }
            [flag, core, seed] if flag == "--title" => (Source::Title(None), core, seed),
            [flag, file, maps, core, seed] if flag == "--campaign" => {
                (Source::Campaign { file: file.into(), maps: maps.into() }, core, seed)
            }
            [flag, core, seed] if flag == "--skirmish" => (Source::Skirmish, core, seed),
            [file, core, seed] if file != "--campaign" => (Source::Bundle(file.into()), core, seed),
            _ => return Err(USAGE.into()),
        };
        let seed = seed
            .to_str()
            .and_then(|seed| seed.parse().ok())
            .ok_or("seed must be an unsigned 32-bit integer")?;
        Ok(Self { source, core: core.into(), seed, wesnoth_root: root })
    }
}

enum Launch {
    Title(title_screen::TitleScreen),
    Campaign(campaign_browser::Browser),
    Board(BoardData),
    Skirmish(setup::Setup),
}

impl Launch {
    fn from_args(args: &[OsString]) -> Result<Self, Box<dyn Error>> {
        let explicit = match args {
            [] => None,
            [flag, path] if flag == "--wesnoth-root" => Some(Path::new(path)),
            _ => return Options::parse(args).and_then(Self::load),
        };
        let root = desktop::locate(explicit)?;
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_millis() as u32;
        Ok(Self::Title(title_screen::TitleScreen { root, seed, games: title_screen::Games::Installed }))
    }

    fn load(options: Options) -> Result<Self, Box<dyn Error>> {
        let core = read_file(&options.core)?;
        match options.source {
            Source::Title(campaign) => Ok(Self::Title(title_screen::TitleScreen {
                games: title_screen::Games::Prepared(desktop::GameData { campaign, core }),
                root: options.wesnoth_root, seed: options.seed,
            })),
            Source::Campaign { file, maps } => Ok(Self::Campaign(campaign_browser::Browser::load(
                &file, maps, core, options.wesnoth_root, options.seed,
            )?)),
            Source::Bundle(path) => {
                let source = read_file(&path)?;
                let directory = path.parent().unwrap_or_else(|| Path::new(".")).to_owned();
                Ok(Self::Board(BoardData::from_source(&source, &core, directory, options.wesnoth_root, options.seed)?))
            }
            Source::Skirmish => Ok(Self::Skirmish(setup::Setup::load(options.wesnoth_root, core, options.seed)?)),
        }
    }

    fn title(&self) -> &str {
        match self {
            Self::Title(_) => "The Battle for Wesnoth",
            Self::Campaign(_) => "Campaign scenarios",
            Self::Board(data) => &data.title,
            Self::Skirmish(_) => "Skirmish",
        }
    }
}

struct BoardData {
    fresh_attack_seeds: bool,
    source: Vec<u8>,
    core: Vec<u8>,
    restore: Option<saved_game::Restore>,    lesson: Option<tutorial::Lesson>,
    scenario: Bundle,
    story: Vec<story::Part>,
    skip_turn_confirmation: Vec<bool>,
    assets: AssetCatalog,
    title: String,
    directory: PathBuf,
    wesnoth_root: PathBuf,
    seed: u32,
}

impl BoardData {
    fn from_source(source: &[u8], core: &[u8], directory: PathBuf, wesnoth_root: PathBuf, seed: u32) -> Result<Self, Box<dyn Error>> {
        let images = wesnoth_root.join("data/core/images");
        if !images.is_dir() {
            return Err(format!("Wesnoth image directory is missing at {}", images.display()).into());
        }
        let scenario = Bundle::parse_with_core(source, core)?;
        let assets = AssetCatalog::parse(&wesnoth_root, core, source)?;
        let document = wml::parse(source)?;
        let (_, config) = document.children.iter().find(|(tag, _)| tag == "scenario")
            .ok_or("bundle has no scenario")?;
        let title = config.attribute("name").filter(|name| !name.is_empty()).unwrap_or("Scenario").to_owned();
        let story = story::read(config, &title);
        let skip_turn_confirmation = end_turn::side_overrides(config);
        Ok(Self {
            source: source.to_vec(), core: core.to_vec(), restore: None,
            fresh_attack_seeds: document.attribute("rustnoth_interactive_rng") == Some("yes")
                && document.attribute("random_mode").unwrap_or_default().is_empty(),
            scenario,
            lesson: None,
            story,
            skip_turn_confirmation,
            assets,
            title,
            directory,
            wesnoth_root,
            seed,
        })
    }
}

fn read_file(path: &Path) -> Result<Vec<u8>, Box<dyn Error>> {
    fs::read(path).map_err(|error| format!("could not read {} ({error})", path.display()).into())
}

async fn show_board(data: BoardData, players: Players) -> Result<(), Box<dyn Error>> {
    play_board(data, players, None).await.map(|_| ())
}

async fn play_board(mut data: BoardData, players: Players, mut carryover: Option<&mut engine_lite::fixed_match::carryover::CampaignCarryover>) -> Result<Option<String>, Box<dyn Error>> {
    let campaign = carryover.is_some();
    let mut restore = data.restore.take();
    let mut replay_route = restore.as_mut().and_then(|slot| slot.replay_roster.take());
    let mut opening_roster = if campaign && display_preferences::save_replays()? {
        carryover.as_deref().map(|route| route.replay_roster())
    } else { None };
    let mut saved_map = None;
    let game = data.scenario.start_campaign(data.seed, |name| {
        let map = match &restore {
            Some(checkpoint) => checkpoint.map.clone().ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData,"Saved scenario is missing its external map"))?,
            None => fs::read_to_string(data.directory.join(name))?,
        };
        saved_map = Some(map.clone());
        Ok(map)
    }, replay_route.as_mut().or(carryover.as_deref_mut()))?;
    let mut session = Session::with_players(game, players);
    if data.fresh_attack_seeds { session.enable_fresh_attack_seeds(); }
    if let Some(lesson) = &data.lesson { lesson.restrict(&mut session); }
    let mut selection = Selection::default();
    let mut camera = BoardCamera::default();
    let speed = display_preferences::speed::Speed::load()?;
    let skip_ai = display_preferences::skip_ai_moves()?;
    let mut art = render::BoardArt::new(&data.wesnoth_root, speed)?;
    if let Some(checkpoint) = restore {
        if replay_route.is_some() {
            replay_viewer::watch(&mut session, &checkpoint.events, &data, &mut art, &mut camera).await?;
            return Ok(None);
        }
        for event in checkpoint.events { event.apply(&mut session)?; }
        if let Some(view) = session.observation() {
            if let Some(hex) = checkpoint.selected { selection.select_own(hex,view); }
            camera.update(render::board_viewport(),view,false);
            if let Some(focus) = checkpoint.camera { camera.restore(focus); }
        }
    } else {
        story::show(&data.story, &data.assets, &data.wesnoth_root, art.font.as_ref()).await;
    }
    if let Some(observation) = session.observation() {
        art.load(observation, &data.assets, session.map_item_images());
    }
    let mut message = None;
    let mut menu: Option<menu::Menu> = None;
    let mut dialogue_page = 0;
    let mut confirmed_recruit = None;
    let mut confirmed_end_turn = false;
    let preference_dir = display_preferences::config_directory()?;
    let turn_confirmation = end_turn::Mode::load(&preference_dir)?;
    let locale = title_screen::locale::Locale::load(&data.wesnoth_root, title_screen::locale::saved(&preference_dir)?)?;
    let mut end_report_shown = false;
    let top_art = map_menus::Art::new(&data.wesnoth_root);
    let mut top_menu = map_menus::State::default();
    session.enable_combat_playback();
    loop {
        let now = (get_time() * 1000.0) as u64;
        let walking = art.walk.advance(now, is_key_pressed(KeyCode::Escape) || is_key_pressed(KeyCode::Space));
        let dialogue_open = !walking && session.dialogue().is_some();
        if !walking && !dialogue_open && data.lesson.as_ref().is_some_and(tutorial::Lesson::finished) {
            return Ok(None);
        }
        let mut commands = map_menus::available(
            session.objectives().is_some(),
            session.observation().and_then(|v| map_menus::next_unit(v, selection.unit())).is_some(),
            session.legal_actions(),
        );
        if session.observation().is_some_and(|view| view.units.iter().any(|unit| unit.side == view.side)) {
            commands.insert(0, map_menus::Command::UnitList);
        }
        if !campaign && data.lesson.is_none() { commands.insert(0, map_menus::Command::Save); }
        let top = if walking || dialogue_open { map_menus::Response {handled:false,command:None} } else { top_menu.input(&commands) };
        let input_blocked = walking || dialogue_open || top.handled;
        if !input_blocked && is_key_pressed(KeyCode::G)
            && (is_key_down(KeyCode::LeftControl) || is_key_down(KeyCode::RightControl)
                || cfg!(target_os = "macos") && (is_key_down(KeyCode::LeftSuper) || is_key_down(KeyCode::RightSuper))) {
            match display_preferences::save_grid(!art.show_grid) {
                Ok(()) => art.show_grid = !art.show_grid,
                Err(error) => message = Some(format!("Could not save grid preference: {error}")),
            }
        }
        let unit_list = top.command == Some(map_menus::Command::UnitList)
            || !input_blocked && is_key_pressed(KeyCode::U) && (is_key_down(KeyCode::LeftAlt) || is_key_down(KeyCode::RightAlt));
        if unit_list && let Some(view) = session.observation() {
            if let Some(at) = unit_list::show(&data, &session, view, selection.unit()).await {
                selection.select_own(at, view);
                camera.center_world([at.x as f32 * 0.75 + 0.5, at.y as f32 + at.x.rem_euclid(2) as f32 * 0.5 + 0.5]);
                menu = None;
            }
            next_frame().await;
            continue;
        }
        match top.command {
            Some(map_menus::Command::Quit) => return Ok(None),
            Some(map_menus::Command::NextUnit) => {
                if let Some(view) = session.observation() && let Some(hex) = map_menus::next_unit(view, selection.unit()) {
                    selection.select_own(hex, view);
                    camera.center_world([hex.x as f32 * 0.75 + 0.5, hex.y as f32 + hex.x.rem_euclid(2) as f32 * 0.5 + 0.5]);
                    menu = None;
                }
            }
            Some(cmd @ (map_menus::Command::Recruit | map_menus::Command::Recall)) => {
                if let Some(view) = session.observation()
                    && let Some(hex) = map_menus::destination(cmd,session.legal_actions(),selection.unit()) {
                    selection.clear();
                    let mut options = options_at(&selection,hex,view,session.legal_actions(),&data.assets,|id| session.recall_info(id));
                    options.retain(|option| matches!((cmd,&option.action),
                        (map_menus::Command::Recruit,Action::Recruit{..}) | (map_menus::Command::Recall,Action::Recall{..})));
                    menu = menu::Menu::new(hex, options, |_|Vec::new());
                }
            }
            _ => {}
        }
        if !campaign && data.lesson.is_none() && menu.is_none() && ((!input_blocked && save_dialog::save_pressed()) || top.command == Some(map_menus::Command::Save)) {
            next_frame().await;
            if let Some(name) = save_dialog::name(&data.wesnoth_root,&data.title).await {
                message = Some(match saved_game::directory().and_then(|base| saved_game::save(
                    &base,&name,&data,players,saved_map.as_deref(),session.history(),camera.checkpoint(),selection.unit(),None)) {
                    Ok(()) => format!("Saved {name}"),
                    Err(e) => format!("Save failed: {e}"),
                });
            }
            next_frame().await;
            continue;
        }
        if !input_blocked && is_key_pressed(KeyCode::Escape) {
            if menu.take().is_none() {
                return Ok(None);
            }
        }
        if !input_blocked && is_mouse_button_pressed(MouseButton::Right) {
            selection.clear();
            menu = None;
            message = None;
        }
        let (mouse_x, mouse_y) = mouse_position();
        let pointer = vec2(mouse_x, mouse_y);
        if !dialogue_open && let Some(objectives) = session.objectives() {
            let opened = top.command == Some(map_menus::Command::Objectives)
                || (!input_blocked && render::objectives::hotkey());
            if opened {
                render::objectives::show(objectives, &mut art, &data.wesnoth_root).await;
                next_frame().await;
                continue;
            }
        }
        if let Some(observation) = session.observation() {
            let reset = is_key_pressed(KeyCode::Home);
            camera.update(render::board_viewport(), observation, reset);
            if let Some(next) = art.walk.next_hex(now) { camera.show_movement_hex(next); }
            if !reset && !input_blocked {
                if is_mouse_button_pressed(MouseButton::Left)
                    && let Some(point) = render::minimap_point(observation, pointer)
                {
                    camera.center_world(point);
                }
                camera.scroll([mouse_x, mouse_y], mouse_wheel().1);
                camera.drag(
                    [mouse_x, mouse_y],
                    is_mouse_button_pressed(MouseButton::Middle),
                    is_mouse_button_down(MouseButton::Middle),
                );
                let horizontal = is_key_down(KeyCode::Left) as i32 - is_key_down(KeyCode::Right) as i32;
                let vertical = is_key_down(KeyCode::Up) as i32 - is_key_down(KeyCode::Down) as i32;
                let distance = speed.pan_distance(get_frame_time());
                camera.pan([horizontal as f32 * distance, vertical as f32 * distance]);
            }
        }
        let mut ai_walk = None;
        let computer_acted = !walking && !top.handled
            && session.play_computer_recording_walk((!skip_ai).then_some(&mut ai_walk))?;
        if computer_acted && let Some(view) = session.observation() {
            art.load(view, &data.assets, session.map_item_images());
            if let Some(walk) = ai_walk { art.start_observed_walk(walk, &data.assets); }
        }
        let clicked = !input_blocked && !computer_acted && is_mouse_button_pressed(MouseButton::Left);
        if data.lesson.is_none() && !input_blocked && !computer_acted && is_key_pressed(KeyCode::D) && menu.is_none() {
            menu = menu::Menu::disband(&session, |id| data.assets.unit_name(id).unwrap_or(id).to_owned());
            if menu.is_some() {
                selection.clear();
                message = None;
            }
        }
        let can_end = session.legal_actions().contains(&Action::EndTurn);
        let next = if campaign && !input_blocked { session.next_scenario() } else { None };
        let button = render::end_turn_button(screen_width(), screen_height(), can_end || next.is_some());
        if let Some(id) = next && opening_roster.is_none() && (is_key_pressed(KeyCode::Enter)
            || clicked && button.is_some_and(|bounds| bounds.contains(pointer)))
        {
            if let Some(route) = carryover.as_deref_mut() { session.store_campaign_survivors(route); }
            return Ok(Some(id.to_owned()));
        }
        let menu_action = menu.as_mut().filter(|_| !input_blocked).and_then(|menu| {
            let area = render::menu_area();
            if is_key_pressed(KeyCode::P)
                || (clicked && menu.advancement_button(area).is_some_and(|bounds| bounds.contains(pointer)))
            {
                menu.cycle_advancement();
            }
            if is_key_pressed(KeyCode::Tab) {
                menu.cycle();
            }
            let action = if is_key_pressed(KeyCode::Enter) {
                Some(menu.focused_action().clone())
            } else if clicked {
                menu.action_at(pointer, area).cloned()
            } else {
                None
            }?;
            menu.activate(action)
        });
        let mut end_prompt = None;
        let requested = if std::mem::take(&mut confirmed_end_turn) {
            Some(Action::EndTurn)
        } else if let Some(action) = confirmed_recruit.take() {
            Some(action)
        } else if !input_blocked && !computer_acted && can_end
            && (end_turn::modifier_down() && is_key_pressed(KeyCode::Space)
                || (clicked && button.is_some_and(|bounds| bounds.contains(pointer))))
        {
            end_prompt = turn_confirmation.prompt(&session, &data.skip_turn_confirmation);
            end_prompt.is_none().then_some(Action::EndTurn)
        } else if menu_action.is_some() {
            menu_action
        } else if clicked
            && let Some(observation) = session.observation()
            && let Some(position) = camera.hex_at([mouse_x, mouse_y])
        {
            let movement = select_move(&mut selection, position, observation, session.legal_actions());
            menu = if movement.is_some() {
                None
            } else {
                menu::Menu::new(
                    position,
                    options_at(&selection, position, observation, session.legal_actions(), &data.assets, |id| session.recall_info(id)),
                    |from| session.advancement_choices(from),
                )
            };
            if let Some(attack_menu) = menu.as_mut()
                && let Action::Attack(choice) = attack_menu.focused_action()
            {
                let previews = session.attack_previews(choice.from, choice.to);
                let window = render::attack_dialog::AttackDialog::new(observation, previews, &data.assets, &mut art, &data.wesnoth_root);
                let action = if let Some(window) = window {
                    window.choose(attack_menu, &data.assets, || {
                        render::draw(session.state(), Some(observation), camera.layout(), &mut art,
                            render::MovementView {
                                session: &session,
                                route: None,
                                global_time: session.global_time_of_day(),
                                selected: selection.unit(), actions: session.legal_actions(), message: None,
                                assets: &data.assets, menu: None,
                            });
                    }).await
                } else { None };
                if action.is_none() { menu = None; }
                action
            } else {
                movement
            }
        } else {
            None
        };
        if let Some(action) = requested {
            let reselect = match &action {
                Action::Attack(attack) => Some(attack.from),
                _ => None,
            };
            let advancement = menu.as_ref().and_then(menu::Menu::preferred_advancement);
            let mut walk = Vec::new();
            match session.apply_recording_walk(action, advancement, Some(&mut walk)) {
                Ok(_) => {
                    if let Some(lesson) = &mut data.lesson { lesson.advance(&mut session); }
                    message = None;
                    menu = None;
                    selection.clear();
                    if let Some(observation) = session.observation() {
                        if let Some(position) = walk.last().copied().or(reselect) {
                            selection.select_own(position, observation);
                        }
                        art.load(observation, &data.assets, session.map_item_images());
                        art.start_walk(walk, observation, &data.assets);
                    }
                }
                Err(error) => { message = Some(error.to_string()); menu = None; }
            }
        }
        if let Some(fight) = session.take_combat() && let Some(layout) = camera.layout() {
            render::combat_motion::show(fight, &session, layout, &mut art, &data.assets).await;
        }
        let hovered = selection.unit().zip(camera.hex_at([mouse_x, mouse_y]))
            .filter(|_| !input_blocked && menu.is_none() && session.dialogue().is_none());
        session.track_hovered_move(hovered);
        render::draw(
            session.state(),
            session.observation(),
            camera.layout(),
            &mut art,
            render::MovementView {
                session: &session,
                global_time: session.global_time_of_day(),
                route: session.hovered_move(),
                selected: selection.unit(),
                actions: session.legal_actions(),
                message: message.as_deref(),
                assets: &data.assets,
                menu: menu.as_ref().filter(|menu| !menu.offers_recruit()),
            },
        );
        if let Some(lesson) = &data.lesson && session.dialogue().is_none() {
            lesson.draw_prompt(selection.unit().is_some(), art.font.as_ref());
        }
        if session.legal_actions().contains(&Action::EndTurn)
            && let Some(bounds) = button
        {
            let highlighted = bounds.contains(pointer) || end_turn::modifier_down() && is_key_down(KeyCode::Space);
            render::draw_turn_button(bounds, highlighted, &art, locale.game("End Turn"));
        } else if campaign && session.next_scenario().is_some()
            && let Some(bounds) = render::end_turn_button(screen_width(), screen_height(), true)
        {
            render::draw_turn_button(bounds, bounds.contains(pointer), &art, "Continue");
        }
        top_menu.draw(&top_art, art.font.as_ref(), &commands);
        if let Some(prompt) = end_prompt {
            confirmed_end_turn = end_turn::confirm(&data.wesnoth_root, &locale, prompt).await;
            next_frame().await;
            continue;
        }
        if menu.as_ref().is_some_and(menu::Menu::offers_recruit)
            && let Some(view) = session.observation()
            && let Some(choices) = menu.take()
        {
            match recruit_dialog::show(&data, choices.options(), view.side).await {
                recruit_dialog::Result::Recruit(action) => confirmed_recruit = Some(action),
                recruit_dialog::Result::Recall => menu = choices.into_recall(),
                recruit_dialog::Result::Cancel => {}
            }
        }
        if !art.walk.active() && let Some(line) = session.dialogue() {
            let last_page = render::draw_dialogue(line, &data.assets, &mut art, dialogue_page);
            if dialogue_open && (is_key_pressed(KeyCode::Enter) || is_key_pressed(KeyCode::Space)
                || is_key_pressed(KeyCode::Escape) || is_mouse_button_pressed(MouseButton::Left))
            {
                if last_page { session.dismiss_dialogue(); dialogue_page = 0; }
                else { dialogue_page += 1; }
            }
        }
        if session.dialogue().is_none() && !matches!(session.state(), engine_lite::fixed_match::MatchState::Playing(_))
            && let Some(roster) = opening_roster.take() && session.save_victory_replay() {
            let result = saved_game::directory().and_then(|base| saved_game::save(
                &base, &format!("{} replay",data.title), &data, players, saved_map.as_deref(),
                session.history(), None, None, Some(&roster)));
            if let Err(e) = result { replay_viewer::save_error(&data.wesnoth_root, &e).await; }
        }
        if !end_report_shown && data.lesson.is_none() && !dialogue_open && session.dialogue().is_none() && session.show_end_report() {
            end_report_shown = true;
            if scenario_report::show(&data.wesnoth_root,&session,campaign).await {
                if let Some(route) = carryover.as_deref_mut() { session.store_campaign_survivors(route); }
                return Ok(if campaign { session.next_scenario().map(str::to_owned) } else { None });
            }
        }
        next_frame().await;
    }
}

fn select_move(
    selection: &mut Selection,
    position: Hex,
    observation: &Observation,
    legal_actions: &[Action],
) -> Option<Action> {
    if selection.select_own(position, observation) {
        return None;
    }
    selection
        .actions_at(position, legal_actions)
        .find(|action| matches!(action, Action::Move { .. }))
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_lite::observation::ObservedUnit;

    #[test]
    fn defaults_to_the_sibling_asset_checkout_and_accepts_an_override() {
        let args = ["map.cfg", "core.cfg", "5489"].map(OsString::from);
        let defaults = Options::parse(&args).unwrap();
        assert_eq!(defaults.wesnoth_root, PathBuf::from("../wesnoth_src"));
        assert_eq!(defaults.seed, 5489);
        let args = ["map.cfg", "core.cfg", "0", "--wesnoth-root", "Local Wesnoth"].map(OsString::from);
        let custom = Options::parse(&args).unwrap();
        assert_eq!(custom.wesnoth_root, PathBuf::from("Local Wesnoth"));
        assert_eq!(custom.seed, 0);
        let args = ["--skirmish", "core.cfg", "3"].map(OsString::from);
        assert_eq!(Options::parse(&args).unwrap().source, Source::Skirmish);
        let args = ["--campaign", "story.cfg", "maps", "core.cfg", "3", "--wesnoth-root", "game"].map(OsString::from);
        assert_eq!(Options::parse(&args).unwrap(), Options {
            source: Source::Campaign { file: "story.cfg".into(), maps: "maps".into() },
            core: "core.cfg".into(), seed: 3, wesnoth_root: "game".into(),
        });
    }

    #[test]
    fn a_map_click_moves_only_a_selected_own_unit_to_a_legal_target() {
        let start = Hex { x: 0, y: 0 };
        let destination = Hex { x: 1, y: 0 };
        let observation = Observation {
            side: 1,
            allied_sides: vec![1],
            gold: Some(10),
            width: 3,
            height: 2,
            tiles: Vec::new(),
            units: vec![ObservedUnit {
                facing: None,
                can_recruit: false, emits_zoc: true,
                max_hitpoints: 33, full_action_points: Some(true),
                position: start, side: 1, type_id: "Bowman".into(),
                experience: 0, max_experience: 35,
                hitpoints: 33, moves_left: Some(5), attacks_left: Some(1),
                poisoned: false, slowed: false,
                petrified: false,
            }],
        };
        let action = Action::Move { from: start, to: destination };
        let legal = [action.clone(), Action::EndTurn];
        let mut selection = Selection::default();
        assert_eq!(select_move(&mut selection, destination, &observation, &legal), None);
        assert_eq!(select_move(&mut selection, start, &observation, &legal), None);
        assert_eq!(selection.unit(), Some(start));
        assert_eq!(select_move(&mut selection, Hex { x: 2, y: 0 }, &observation, &legal), None);
        assert_eq!(select_move(&mut selection, destination, &observation, &legal), Some(action));
    }
}
