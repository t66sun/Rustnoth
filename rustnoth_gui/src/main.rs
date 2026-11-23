use std::{
    env,
    error::Error,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
};

use engine_lite::{
    fixed_match::Action, hex::Hex, observation::Observation, scenario_bundle::Bundle,
};
use macroquad::{Window, miniquad::conf::Conf, prelude::*};
use rustnoth_gui::{assets::AssetCatalog, choices::options_at, selection::Selection, session::{Players, Session}};

use camera::BoardCamera;

mod camera;
mod campaign_browser;
mod list_scroll;
mod desktop;
mod editor;
mod minimap_buttons;
mod dialog;
mod display_preferences;
mod end_turn;
mod turn_notice;
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
    let (window_width, window_height) = desktop::window::initial_size();
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
        window_width,
        window_height,
        window_resizable: true,
        ..Conf::default()
    };
    Window::from_config(sprite_batches(configuration), async move {
        macroquad::experimental::coroutines::start_coroutine(screenshot::listen());
        let result = match launch {
            Launch::Title(title) => title.run(fullscreen, vec2(window_width as f32, window_height as f32)).await,
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

fn sprite_batches(window: Conf) -> macroquad::conf::Conf {
    // Changing terrain textures starts a fresh GPU buffer pair. Most pairs hold
    // one quad. Consecutive glyphs or sprites sharing a texture can still batch.
    let mut config = macroquad::conf::Conf::from(window);
    config.draw_call_vertex_capacity = 64 * 4;
    config.draw_call_index_capacity = 64 * 6;
    config
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
    tutorial_award: Option<title_screen::tutorial_progress::Award>,
    source: Vec<u8>,
    core: Vec<u8>,
    restore: Option<saved_game::Restore>,    lesson: Option<tutorial::Lesson>,
    scenario: Bundle,
    story: Vec<story::Part>,
    skip_turn_confirmation: Vec<bool>,
    turn_names: Vec<String>,
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
        let (scenario, (assets, title, story, skip_turn_confirmation, turn_names, tutorial_award, fresh_attack_seeds)) =
            Bundle::parse_with_core_and(source, core, |sources| {
                let config = sources.scenario;
                let fresh_attack_seeds = sources.bundle.attribute("rustnoth_interactive_rng") == Some("yes")
                    && sources.bundle.attribute("random_mode").unwrap_or_default().is_empty();
                let assets = AssetCatalog::from_board(&wesnoth_root, sources)?;
                let title = config.attribute("name").filter(|name| !name.is_empty()).unwrap_or("Scenario").to_owned();
                let story = story::read(config, &title);
                Ok((assets, title, story, end_turn::side_overrides(config),
                    turn_notice::names(config), title_screen::tutorial_progress::Award::from_victory(config),
                    fresh_attack_seeds))
            })?;
        Ok(Self {
            source: source.to_vec(), core: core.to_vec(), restore: None,
            fresh_attack_seeds,
            scenario,
            lesson: None,
            tutorial_award,
            story,
            skip_turn_confirmation,
            turn_names,
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

async fn show_board(mut data: BoardData, players: Players) -> Result<(), Box<dyn Error>> {
    if let Some(checkpoint) = data.restore.as_mut()
        && !checkpoint.watch_replay
        && let Some(browser) = checkpoint.campaign.take()
    {
        let route = checkpoint.replay_roster.take().ok_or("Missing incoming campaign roster")?;
        return browser.play_route(data, route, players).await;
    }
    play_board(data, players, None).await.map(|_| ())
}

async fn play_board(mut data: BoardData, players: Players, journey: Option<(&campaign_browser::Browser, &mut engine_lite::fixed_match::carryover::CampaignCarryover)>) -> Result<Option<String>, Box<dyn Error>> {
    let (browser, mut carryover) = journey.unzip();
    let campaign = browser.is_some();
    let mut restore = data.restore.take();
    let mut replay_route = restore.as_mut().and_then(|slot| slot.replay_roster.take());
    // Starting the scenario consumes its incoming veterans. Keep the opening state
    // for manual saves even when the player has disabled automatic victory replays.
    let opening_roster = carryover.as_deref().map(|route| route.replay_roster());
    let mut save_victory_replay = campaign && display_preferences::save_replays()?;
    let mut saved_map = None;
    let game = data.scenario.start_with_controllers(data.seed, |name| {
        let map = match &restore {
            Some(checkpoint) => checkpoint.map.clone().ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData,"Saved scenario is missing its external map"))?,
            None => fs::read_to_string(data.directory.join(name))?,
        };
        saved_map = Some(map.clone());
        Ok(map)
    }, replay_route.as_mut().or(carryover.as_deref_mut()), players == Players::Campaign)?;
    let mut session = Session::with_players(game, players);
    if data.fresh_attack_seeds { session.enable_fresh_attack_seeds(); }
    let mut selection = Selection::default();
    let mut camera = BoardCamera::default();
    let speed = display_preferences::speed::Speed::load()?;
    let mouse_scroll_threshold = title_screen::profile::mouse_scroll_threshold(
        &title_screen::profile::read_profile().unwrap_or_default(),
    );
    let skip_ai = display_preferences::skip_ai_moves()?;
    let preference_dir = display_preferences::config_directory()?;
    let mut locale = title_screen::locale::Locale::load(&data.wesnoth_root, title_screen::locale::saved(&preference_dir)?)?;
    let mut art = render::BoardArt::new(&data.wesnoth_root, speed, &locale)?;
    if let Some(checkpoint) = restore {
        if replay_route.is_some() || checkpoint.watch_replay {
            replay_viewer::watch(&mut session, &checkpoint.events, &data, &mut art, &mut camera).await?;
            return Ok(None);
        }
        if let Some((lesson, hints)) = data.lesson.as_mut().zip(checkpoint.tutorial_hints) {
            lesson.resume(&mut session, &checkpoint.events, hints, &data.wesnoth_root, &mut locale)?;
        } else {
            for event in checkpoint.events { event.apply(&mut session)?; }
        }
        if let Some(view) = session.observation() {
            if let Some(hex) = checkpoint.selected { selection.select_own(hex,view); }
            camera.update(render::board_viewport(),view,false);
            if let Some(focus) = checkpoint.camera { camera.restore(focus); }
        }
    } else {
        if let Some(lesson) = &mut data.lesson { lesson.start(&mut session, &data.wesnoth_root, &mut locale); }
        story::show(&data.story, &data.assets, &data.wesnoth_root, art.font.as_ref(), locale.ui("Skip")).await;
    }
    if let Some(observation) = session.observation() {
        art.load(observation, &data.assets, &session);
    }
    let mut message = None;
    let mut menu: Option<menu::Menu> = None;
    let mut dialogue_page = 0;
    let mut confirmed_action = None;
    let mut after_walk: Option<(engine_lite::fixed_match::attack_preview::AttackPreview, Option<String>)> = None;
    let mut confirmed_end_turn = false;
    let turn_confirmation = end_turn::Mode::load(&preference_dir)?;
    let notify_turn = display_preferences::turn_dialog()?;
    let mut notified = None;
    let mut end_report_shown = false;
    // Replay playback returns above. Read the award from the original scenario.
    let mut tutorial_award = data.lesson.as_ref().and_then(|lesson| lesson.victory_award).or(data.tutorial_award);
    let top_art = map_menus::Art::new(&data.wesnoth_root);
    let mut top_menu = map_menus::State::default();
    // Combat plays after the action is committed. Keep the controls the viewer
    // last saw until the final strike or a skip returns to the updated board.
    let end_label = locale.game("End Turn").to_owned();
    let end_scenario = locale.game("End Scenario").to_owned();
    let mut visible_turn_label = None;
    let mut turn_enabled = true;
    let mut visible_prompt = String::new();
    session.enable_combat_playback();
    loop {
        let now = (get_time() * 1000.0) as u64;
        let walking = art.walk.advance(now, is_key_pressed(KeyCode::Escape) || is_key_pressed(KeyCode::Space));
        let dialogue_open = !walking && session.dialogue().is_some();
        if notify_turn && !walking && !dialogue_open
            && let Some(turn) = turn_notice::new_turn(session.state(), !session.legal_actions().is_empty(), notified) {
            notified = Some(turn);
            let name = data.turn_names.get(turn.side - 1).map(String::as_str).unwrap_or("");
            let message = locale.game("It is now $name|’s turn").replace("$name|", name);
            turn_notice::show(&data.wesnoth_root, art.font.as_ref(), &message).await;
            continue;
        }
        let mut commands = map_menus::available(
            session.has_authored_objectives(),
            session.observation().and_then(|v| map_menus::next_unit(v, selection.unit())).is_some(),
            session.legal_actions(),
        );
        if session.observation().is_some_and(|view| view.units.iter().any(|unit| unit.side == view.side)) {
            commands.insert(0, map_menus::Command::UnitList);
        }
        commands.insert(0, map_menus::Command::Save);
        if session.can_undo_tutorial_move() || data.lesson.as_ref().and_then(|lesson| lesson.undo_from(&session)).is_some() {
            commands.push(map_menus::Command::Undo);
        }
        if session.can_show_enemy_moves() { commands.push(map_menus::Command::ShowEnemyMoves); }
        let hovered = camera.hex_at(top_menu.target_point(Vec2::from(mouse_position()).to_array()));
        let described = session.observation().and_then(|view| {
            map_menus::described_unit(hovered, selection.unit(), |at| view.units.iter().any(|u| u.position == at))
        });
        if let Some(at) = described.filter(|_| menu.is_none()) {
            if data.lesson.is_some() && let Some(target) = hovered
                && session.legal_actions().iter().any(|a| matches!(a, Action::Recruit { to, .. } if *to == target)) {
                commands.push(map_menus::Command::RecruitAt(target));
            }
            commands.push(map_menus::Command::Describe(at));
        }
        let auto_objectives = !walking && session.has_objective_notice();
        let top = if walking || dialogue_open || auto_objectives { map_menus::Response {handled:false,command:None} } else { top_menu.input(&commands) };
        let show_objectives = auto_objectives || session.has_authored_objectives() && !walking && !dialogue_open
            && (top.command == Some(map_menus::Command::Objectives) || !top.handled && render::objectives::hotkey());
        let unit_list = top.command == Some(map_menus::Command::UnitList)
            || !walking && !dialogue_open && !show_objectives && !top.handled && commands.contains(&map_menus::Command::UnitList)
                && is_key_pressed(KeyCode::U) && (is_key_down(KeyCode::LeftAlt) || is_key_down(KeyCode::RightAlt));
        let modal_input = walking || dialogue_open || top.handled || show_objectives || unit_list;
        let save = menu.is_none()
            && (top.command == Some(map_menus::Command::Save) || !modal_input && save_dialog::save_pressed());
        let unit_command = top.command.or_else(|| {
            if !modal_input && !save && menu.is_none() && is_key_pressed(KeyCode::D) {
                map_menus::unit_shortcut(described, is_key_down)
            } else {
                None
            }
        });
        let describe = match unit_command {
            Some(map_menus::Command::Describe(at)) => Some(at),
            _ => None,
        };
        let input_blocked = modal_input || save || describe.is_some();
        if !input_blocked && is_key_pressed(KeyCode::G)
            && (is_key_down(KeyCode::LeftControl) || is_key_down(KeyCode::RightControl)
                || cfg!(target_os = "macos") && (is_key_down(KeyCode::LeftSuper) || is_key_down(KeyCode::RightSuper))) {
            match display_preferences::save_grid(!art.show_grid) {
                Ok(()) => art.show_grid = !art.show_grid,
                Err(error) => message = Some(format!("Could not save grid preference: {error}")),
            }
        }
        if menu.is_none() && (top.command == Some(map_menus::Command::ShowEnemyMoves)
            || !input_blocked && map_menus::enemy_moves_hotkey()) {
            session.show_enemy_moves(camera.hex_at(Vec2::from(mouse_position()).to_array()));
        }
        match top.command {
            Some(map_menus::Command::Quit) => return Ok(None),
            Some(map_menus::Command::Undo) => {
                let restart = || -> Result<_, Box<dyn Error>> {
                    let mut route = opening_roster.as_ref()
                        .and_then(engine_lite::fixed_match::carryover::CampaignCarryover::from_replay_roster);
                    let game = data.scenario.start_with_controllers(data.seed, |name| match &saved_map {
                        Some(map) => Ok(map.clone()),
                        None => fs::read_to_string(data.directory.join(name)),
                    }, route.as_mut(), players == Players::Campaign)?;
                    Ok(Session::with_players(game, players))
                };
                let from = if let Some(lesson) = &mut data.lesson {
                    lesson.undo(&mut session, restart)?
                } else {
                    session.undo_tutorial_move(restart)?
                };
                if let Some(from) = from {
                    if data.fresh_attack_seeds { session.enable_fresh_attack_seeds(); }
                    session.enable_combat_playback();
                    selection.clear();
                    if let Some(view) = session.observation() {
                        selection.select_own(from, view);
                        art.load(view, &data.assets, &session);
                    }
                    menu = None;
                    message = None;
                }
            }
            Some(map_menus::Command::NextUnit) => {
                session.clear_enemy_moves();
                if let Some(view) = session.observation() && let Some(hex) = map_menus::next_unit(view, selection.unit()) {
                    selection.select_own(hex, view);
                    camera.center_world([hex.x as f32 * 0.75 + 0.5, hex.y as f32 + hex.x.rem_euclid(2) as f32 * 0.5 + 0.5]);
                    menu = None;
                }
            }
            Some(cmd @ (map_menus::Command::Recruit | map_menus::Command::RecruitAt(_) | map_menus::Command::Recall)) => {
                let target = match cmd {
                    map_menus::Command::RecruitAt(hex) => Some(hex),
                    _ => map_menus::destination(cmd, session.legal_actions(), selection.unit()),
                };
                if let Some(view) = session.observation()
                    && let Some(hex) = target {
                    selection.clear();
                    let mut options = options_at(&selection,hex,view,session.legal_actions(),&data.assets,|id| session.recall_info(id));
                    options.retain(|option| matches!((cmd,&option.action),
                        (map_menus::Command::Recruit | map_menus::Command::RecruitAt(_),Action::Recruit{..}) | (map_menus::Command::Recall,Action::Recall{..})));
                    menu = menu::Menu::new(hex, options, |_|Vec::new());
                }
            }
            _ => {}
        }
        if !input_blocked && is_key_pressed(KeyCode::Escape) {
            if menu.take().is_none() {
                return Ok(None);
            }
        }
        if !input_blocked && is_mouse_button_pressed(MouseButton::Right) {
            session.dismiss_enemy_moves(camera.hex_at(Vec2::from(mouse_position()).to_array()));
            selection.clear();
            menu = None;
            message = None;
            let pointer = Vec2::from(mouse_position());
            let view = render::board_viewport();
            if data.lesson.is_some() && Rect::new(view.x, view.y, view.width, view.height).contains(pointer)
                && let Some(hex) = camera.hex_at(pointer.to_array()) {
                top_menu.open_recruit_at(hex, pointer, session.legal_actions());
            }
        }
        let (mouse_x, mouse_y) = mouse_position();
        let pointer = vec2(mouse_x, mouse_y);
        let mut briefing_scroll = false;
        art.briefing_highlight = None;
        let mut minimap_action = false;
        if let Some(observation) = session.observation() {
            let reset = is_key_pressed(KeyCode::Home) && !dialogue_open;
            camera.update(render::board_viewport(), observation, reset);
            if let Some(focus) = session.dialogue().and_then(|line| line.focus) {
                briefing_scroll = camera.scroll_briefing(focus, get_frame_time(), speed.scroll, art.animation_rate());
                if !briefing_scroll { art.briefing_highlight = Some(focus.hex); }
            }
            if let Some(next) = art.walk.next_hex(now) { camera.show_movement_hex(next); }
            if !reset && !input_blocked {
                match art.minimap_input(pointer, &mut camera) {
                    Ok(handled) => minimap_action = handled,
                    Err(error) => { message = Some(error.to_string()); minimap_action = true; }
                }
                if !minimap_action && is_mouse_button_pressed(MouseButton::Left)
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
                let edge = if menu.is_none() && !is_mouse_button_down(MouseButton::Middle) {
                    top_menu.vertical_edge_scroll(pointer, vec2(screen_width(), screen_height()), mouse_scroll_threshold)
                } else {
                    0
                };
                let distance = speed.pan_distance(get_frame_time());
                camera.pan([horizontal as f32 * distance, (vertical + edge) as f32 * distance]);
            }
        }
        let input_blocked = input_blocked || minimap_action;
        let mut ai_walk = None;
        let computer_acted = !input_blocked
            && session.play_computer_recording_walk((!skip_ai).then_some(&mut ai_walk))?;
        if computer_acted && let Some(lesson) = &mut data.lesson {
            lesson.after_computer(&mut session);
        }
        if computer_acted && let Some(view) = session.observation() {
            art.load(view, &data.assets, &session);
            if let Some(walk) = ai_walk { art.start_observed_walk(walk, &data.assets); }
        }
        let clicked = !input_blocked && !computer_acted && is_mouse_button_pressed(MouseButton::Left);
        if data.lesson.is_none() && !input_blocked && !computer_acted && unit_command == Some(map_menus::Command::Disband) {
            menu = menu::Menu::disband(&session, |id| data.assets.unit_name(id).unwrap_or(id).to_owned());
            if menu.is_some() {
                selection.clear();
                message = None;
            }
        }
        let hovered = selection.unit().zip(camera.hex_at([mouse_x, mouse_y]))
            .filter(|_| !input_blocked && !computer_acted && menu.is_none());
        let allow_move_attack = data.lesson.as_ref().is_none_or(tutorial::Lesson::permits_move_attack);
        session.track_pointer(hovered, allow_move_attack);
        let can_end = session.legal_actions().contains(&Action::EndTurn);
        let denial = data.lesson.as_ref().and_then(|lesson| lesson.end_turn_denial(&session))
            .or_else(|| session.tutorial_end_turn_denial());
        let show_end = can_end || denial.is_some();
        if campaign && session.human_won() && let Some(award) = tutorial_award.take() {
            if let Err(e) = award.record(&data.wesnoth_root, &preference_dir) {
                message = Some(format!("Could not record tutorial achievement: {e}"));
            }
        }
        let next = if campaign && !input_blocked { session.next_scenario() } else { None };
        let final_victory = campaign && session.final_victory_ready(end_report_shown);
        let button = render::end_turn_button(screen_width(), screen_height(), show_end || next.is_some() || final_victory);
        if !input_blocked && (next.is_some() || final_victory) && !save_victory_replay && (is_key_pressed(KeyCode::Enter)
            || clicked && button.is_some_and(|bounds| bounds.contains(pointer)))
        {
            if let Some(route) = carryover.as_deref_mut() { session.store_campaign_survivors(route); }
            return Ok(next.map(str::to_owned));
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
        let mut followup_advancement = None;
        let mut end_prompt = None;
        let mut attack_dialog = None;
        let mut denial_message = None;
        let mut selected_student = false;
        let requested = if std::mem::take(&mut confirmed_end_turn) {
            Some(Action::EndTurn)
        } else if let Some(action) = confirmed_action.take() {
            Some(action)
        } else if !walking && let Some((preview, advancement)) = after_walk.take() {
            followup_advancement = advancement;
            session.can_finish_attack(&preview).then_some(Action::Attack(preview.choice))
        } else if !input_blocked && !computer_acted && show_end
            && (end_turn::modifier_down() && is_key_pressed(KeyCode::Space)
                || (clicked && button.is_some_and(|bounds| bounds.contains(pointer))))
        {
            if let Some(command) = denial {
                denial_message = Some(if command.attribute("reason").unwrap_or_default().is_empty() {
                    locale.game("You cannot end your turn yet!").to_owned()
                } else {
                    locale.attribute(&data.wesnoth_root, command, "reason", "")
                });
                None
            } else {
                end_prompt = turn_confirmation.prompt(&session, &data.skip_turn_confirmation);
                end_prompt.is_none().then_some(Action::EndTurn)
            }
        } else if menu_action.is_some() {
            menu_action
        } else if clicked
            && let Some(observation) = session.observation()
            && let Some(position) = camera.hex_at([mouse_x, mouse_y])
        {
            let movement = select_move(&mut selection, position, observation, session.legal_actions());
            selected_student = data.lesson.is_some() && selection.unit() == Some(position)
                && observation.units.iter().any(|unit| unit.side == observation.side && unit.can_recruit && unit.position == position);
            let approach = session.hovered_attack().filter(|plan| plan.target == position);
            menu = if let Some(plan) = approach {
                let attacker = observation.units.iter().find(|unit| unit.position == plan.origin);
                let options = plan.weapons.iter().map(|row| rustnoth_gui::choices::ActionOption {
                    label: attacker.and_then(|unit| data.assets.weapon_name(&unit.type_id, row.choice.weapon))
                        .map_or_else(|| format!("Weapon {}", row.choice.weapon + 1), str::to_owned),
                    action: Action::Attack(row.choice),
                }).collect();
                menu::Menu::new(position, options, |_| session.advancement_choices(plan.origin))
            } else if movement.is_some() {
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
                let origin = approach.map_or(choice.from, |plan| plan.origin);
                let previews = approach.map_or_else(|| session.attack_previews(choice.from, choice.to), |plan| plan.weapons.clone());
                attack_dialog = render::attack_dialog::AttackDialog::new(observation, origin, previews, &data.assets, &mut art, &data.wesnoth_root);
                if attack_dialog.is_none() { menu = None; }
                None
            } else {
                movement
            }
        } else {
            None
        };
        if selected_student && let Some(lesson) = &mut data.lesson && lesson.select_student(&mut session) {
            menu = None;
        }
        if clicked && let Some(selected) = selection.unit()
            && camera.hex_at([mouse_x, mouse_y]) == Some(selected) && let Some(lesson) = &mut data.lesson {
            lesson.selected(selected, &mut session);
        }
        if let Some(action) = requested {
            let reselect = match &action {
                Action::Attack(attack) => Some(attack.from),
                _ => None,
            };
            let advancement = if matches!(action, Action::Attack(_)) {
                followup_advancement.as_deref().or_else(|| menu.as_ref().and_then(menu::Menu::preferred_advancement))
            } else { None };
            let weapon_hint = data.lesson.as_ref().and_then(|lesson| lesson.weapon_hint(&session, &action));
            let mut walk = Vec::new();
            match session.apply_recording_walk(action, advancement, Some(&mut walk)) {
                Ok(_) => {
                    if let Some(lesson) = &mut data.lesson {
                        lesson.after_player(&mut session, weapon_hint);
                    }
                    if after_walk.as_ref().is_some_and(|(row, _)| walk.last() != Some(&row.choice.from)
                        || session.dialogue().is_some() || session.has_objective_notice()) { after_walk = None; }
                    message = None;
                    menu = None;
                    selection.clear();
                    if let Some(observation) = session.observation() {
                        if let Some(position) = walk.last().copied().or(reselect) {
                            selection.select_own(position, observation);
                        }
                        art.load(observation, &data.assets, &session);
                        art.start_walk(walk, observation, &data.assets);
                    }
                }
                Err(error) => { after_walk = None; message = Some(error.to_string()); menu = None; }
            }
        }
        let draw_controls = |art: &mut render::BoardArt, prompt: &str, turn: Option<&str>, enabled: bool| {
            if let Some(lesson) = &data.lesson { lesson.draw_labels(art, camera.layout()); }
            if !prompt.is_empty() {
                art.draw_tutorial_print(prompt, &data.wesnoth_root);
            }
            if let Some(label) = turn
                && let Some(bounds) = render::end_turn_button(screen_width(), screen_height(), true) {
                let highlighted = bounds.contains(Vec2::from(mouse_position()))
                    || label == end_label && end_turn::modifier_down() && is_key_down(KeyCode::Space);
                render::draw_turn_button(bounds, highlighted, enabled, art, label);
            }
            top_menu.draw(&top_art, art.font.as_ref(), &commands);
        };
        if let Some(fight) = session.take_combat() && let Some(layout) = camera.layout() {
            render::combat_motion::show(fight, &session, layout, &mut art, &data.assets,
                |art| draw_controls(art, &visible_prompt, visible_turn_label, turn_enabled)).await;
        }
        let under_mouse = camera.hex_at([mouse_x, mouse_y]);
        if clicked && under_mouse.is_some() {
            session.dismiss_enemy_moves(under_mouse);
        } else if !input_blocked && menu.is_none() && session.dialogue().is_none()
            && !is_mouse_button_pressed(MouseButton::Right) {
            session.hover_enemy_moves(under_mouse, selection.unit().is_some());
        }
        turn_enabled = session.dialogue().is_none();
        visible_turn_label = if session.legal_actions().contains(&Action::EndTurn)
            || session.tutorial_end_turn_denial().is_some()
            || data.lesson.as_ref().is_some_and(|lesson| lesson.end_turn_denial(&session).is_some()) {
            Some(end_label.as_str())
        } else if campaign && (session.next_scenario().is_some() || final_victory) {
            Some(if data.lesson.is_some() || final_victory { end_scenario.as_str() } else { "Continue" })
        } else {
            None
        };
        visible_prompt.clear();
        if session.dialogue().is_none() && let Some(prompt) = data.lesson.as_ref()
            .and_then(|lesson| lesson.prompt(selection.unit().is_some())) {
            visible_prompt.push_str(prompt);
        } else if session.dialogue().is_none() && let Some(print) = session.tutorial_print() {
            visible_prompt = locale.attribute(&data.wesnoth_root, print, "text", "");
        }
        art.linger_color = if art.walk.active() { None } else { session.linger_color(end_report_shown) };
        let last_page = render::draw(
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
                menu: menu.as_ref().filter(|menu| attack_dialog.is_none() && !menu.offers_recruit()),
            },
            |art| {
                draw_controls(art, &visible_prompt, visible_turn_label, turn_enabled);
                session.dialogue().filter(|_| !briefing_scroll && !art.walk.active())
                    .map(|line| render::draw_dialogue(line, &data.assets, art, dialogue_page))
            },
        );
        if let Some(at) = describe && let Some(view) = session.observation()
            && let Some(unit) = view.units.iter().find(|u| u.position == at) {
            if let Err(error) = title_screen::show_unit(&data, view, &unit.type_id, &mut locale).await {
                message = Some(error.to_string());
            }
            next_frame().await;
            continue;
        }
        if save {
            if let Some(name) = save_dialog::name(&data.wesnoth_root,&data.title).await {
                message = Some(match saved_game::directory().and_then(|base| saved_game::save(
                    &base,&name,&data,players,saved_map.as_deref(),session.history(),camera.checkpoint(),selection.unit(),
                    browser.zip(opening_roster.as_ref()).map(|(b,r)| saved_game::CampaignSave::Checkpoint(b,r,session.tutorial_hint())))) {
                    Ok(()) => format!("Saved {name}"),
                    Err(e) => format!("Save failed: {e}"),
                });
            }
            next_frame().await;
            continue;
        }
        if unit_list && let Some(view) = session.observation() {
            if let Some(at) = unit_list::show(&data, &session, view, selection.unit()).await {
                selection.select_own(at, view);
                camera.center_world([at.x as f32 * 0.75 + 0.5, at.y as f32 + at.x.rem_euclid(2) as f32 * 0.5 + 0.5]);
                menu = None;
            }
            next_frame().await;
            continue;
        }
        if show_objectives {
            let contents = if auto_objectives { session.objectives().cloned() } else { session.evaluate_objectives() };
            render::objectives::show(&contents.unwrap_or_default(), &data.title, &locale, &mut art, &data.wesnoth_root).await;
            session.acknowledge_objectives();
            continue;
        }
        if let Some(window) = attack_dialog && let Some(choices) = menu.as_mut() {
            confirmed_action = window.choose(choices, &data.assets).await;
            if let Some(Action::Attack(choice)) = confirmed_action.as_ref()
                && let Some(plan) = session.hovered_attack().filter(|plan| plan.origin != plan.staging)
                && let Some(preview) = plan.weapons.iter().find(|row| row.choice == *choice) {
                after_walk = Some((preview.clone(), choices.preferred_advancement().map(str::to_owned)));
                confirmed_action = Some(Action::Move { from: plan.origin, to: plan.staging });
            }
            if confirmed_action.is_none() { menu = None; }
            next_frame().await;
            continue;
        }
        if let Some(reason) = denial_message {
            turn_notice::over_board(&data.wesnoth_root, art.font.as_ref(), &reason).await;
            continue;
        }
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
                recruit_dialog::Result::Recruit(action) => confirmed_action = Some(action),
                recruit_dialog::Result::Recall => menu = choices.into_recall(),
                recruit_dialog::Result::Cancel => {}
            }
        }
        if let Some(last_page) = last_page {
            if dialogue_open && (is_key_pressed(KeyCode::Enter) || is_key_pressed(KeyCode::Space)
                || is_key_pressed(KeyCode::Escape) || is_mouse_button_pressed(MouseButton::Left))
            {
                if last_page { session.dismiss_dialogue(); dialogue_page = 0; }
                else { dialogue_page += 1; }
            }
        }
        if session.dialogue().is_none() && !matches!(session.state(), engine_lite::fixed_match::MatchState::Playing(_))
            && std::mem::take(&mut save_victory_replay) && session.save_victory_replay()
            && let Some(roster) = opening_roster.as_ref() {
            let result = saved_game::directory().and_then(|base| saved_game::save(
                &base, &format!("{} replay",data.title), &data, players, saved_map.as_deref(),
                session.history(), None, None, Some(saved_game::CampaignSave::Replay(roster))));
            if let Err(e) = result { replay_viewer::save_error(&data.wesnoth_root, &e).await; }
        }
        if !end_report_shown && !dialogue_open && session.dialogue().is_none() && session.show_end_report() {
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
    fn textured_batches_fit_the_largest_board_primitive_without_reserving_a_full_mesh() {
        let config = sprite_batches(Conf::default());
        // A filled circle emits a 20-triangle fan in macroquad.
        assert!(config.draw_call_vertex_capacity >= 22 && config.draw_call_index_capacity >= 60);
        let bytes = config.draw_call_vertex_capacity * size_of::<Vertex>()
            + config.draw_call_index_capacity * size_of::<u16>();
        assert!(bytes <= 16 * 1024, "{bytes} bytes for each texture change");
    }

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
