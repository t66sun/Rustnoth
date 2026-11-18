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
use rustnoth_gui::{assets::AssetCatalog, choices::options_at, selection::Selection, session::Session};

use camera::BoardCamera;

mod camera;
mod menu;
mod render;
mod setup;

const USAGE: &str = "rustnoth_gui <bundle.cfg> <core.cfg> <seed> [--wesnoth-root <directory>]\n\
rustnoth_gui --skirmish <core.cfg> <seed> [--wesnoth-root <directory>]\n\
Scroll over the board to zoom. Pan with a middle-button drag or the arrow keys.\n\
Press Home to fit the board.";
const PAN_PIXELS_PER_SECOND: f32 = 480.0;

fn main() -> ExitCode {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if let [flag] = args.as_slice()
        && (flag == "--help" || flag == "-h")
    {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    let launch = match Options::parse(&args).and_then(Launch::load) {
        Ok(data) => data,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let configuration = Conf {
        window_title: format!("Rustnoth - {}", launch.title()),
        window_width: 1200,
        window_height: 820,
        window_resizable: true,
        ..Conf::default()
    };
    Window::from_config(configuration, async move {
        let result = match launch {
            Launch::Board(data) => show_board(data).await,
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
    Bundle(PathBuf),
    Skirmish,
}

impl Options {
    fn parse(args: &[OsString]) -> Result<Self, Box<dyn Error>> {
        let (bundle, core, seed, root) = match args {
            [bundle, core, seed] => (bundle, core, seed, PathBuf::from("../wesnoth_src")),
            [bundle, core, seed, flag, root] if flag == "--wesnoth-root" => {
                (bundle, core, seed, PathBuf::from(root))
            }
            _ => return Err(USAGE.into()),
        };
        let seed = seed
            .to_str()
            .and_then(|seed| seed.parse().ok())
            .ok_or("seed must be an unsigned 32-bit integer")?;
        let source = if bundle == "--skirmish" { Source::Skirmish } else { Source::Bundle(bundle.into()) };
        Ok(Self { source, core: core.into(), seed, wesnoth_root: root })
    }
}

enum Launch {
    Board(BoardData),
    Skirmish(setup::Setup),
}

impl Launch {
    fn load(options: Options) -> Result<Self, Box<dyn Error>> {
        let core = read_file(&options.core)?;
        match options.source {
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
            Self::Board(data) => &data.title,
            Self::Skirmish(_) => "Skirmish",
        }
    }
}

struct BoardData {
    scenario: Bundle,
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
        let title = wml::parse(source)?
            .children
            .into_iter()
            .find(|(tag, _)| tag == "scenario")
            .and_then(|(_, config)| config.attribute("name").map(str::to_owned))
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "Scenario".to_owned());
        Ok(Self {
            scenario,
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

async fn show_board(data: BoardData) -> Result<(), Box<dyn Error>> {
    let game = data.scenario.start(data.seed, |name| fs::read_to_string(data.directory.join(name)))?;
    let mut session = Session::new(game);
    let mut selection = Selection::default();
    let mut camera = BoardCamera::default();
    let mut art = render::BoardArt::new(&data.wesnoth_root);
    if let Some(observation) = session.observation() {
        art.load(observation, &data.assets);
    }
    let mut message = None;
    let mut menu: Option<menu::Menu> = None;
    loop {
        if is_key_pressed(KeyCode::Escape) {
            if menu.take().is_none() {
                return Ok(());
            }
        }
        if is_mouse_button_pressed(MouseButton::Right) {
            selection.clear();
            menu = None;
            message = None;
        }
        let (mouse_x, mouse_y) = mouse_position();
        let pointer = vec2(mouse_x, mouse_y);
        if let Some(observation) = session.observation() {
            let reset = is_key_pressed(KeyCode::Home);
            camera.fit(render::board_viewport(), observation.width, observation.height, reset);
            if !reset {
                camera.scroll([mouse_x, mouse_y], mouse_wheel().1);
                camera.drag(
                    [mouse_x, mouse_y],
                    is_mouse_button_pressed(MouseButton::Middle),
                    is_mouse_button_down(MouseButton::Middle),
                );
                let horizontal = is_key_down(KeyCode::Left) as i32 - is_key_down(KeyCode::Right) as i32;
                let vertical = is_key_down(KeyCode::Up) as i32 - is_key_down(KeyCode::Down) as i32;
                let distance = PAN_PIXELS_PER_SECOND * get_frame_time();
                camera.pan([horizontal as f32 * distance, vertical as f32 * distance]);
            }
        }
        let clicked = is_mouse_button_pressed(MouseButton::Left);
        let can_end = session.legal_actions().contains(&Action::EndTurn);
        let button = render::end_turn_button(screen_width(), screen_height(), can_end);
        let menu_action = menu.as_mut().and_then(|menu| {
            let area = render::menu_area();
            if is_key_pressed(KeyCode::P)
                || (clicked && menu.advancement_button(area).is_some_and(|bounds| bounds.contains(pointer)))
            {
                menu.cycle_advancement();
            }
            if is_key_pressed(KeyCode::Tab) {
                menu.cycle();
            }
            if is_key_pressed(KeyCode::Enter) {
                Some(menu.focused_action().clone())
            } else if clicked {
                menu.action_at(pointer, area).cloned()
            } else {
                None
            }
        });
        let requested = if can_end
            && (is_key_pressed(KeyCode::Space)
                || (clicked && button.is_some_and(|bounds| bounds.contains(pointer))))
        {
            Some(Action::EndTurn)
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
                    options_at(&selection, position, observation, session.legal_actions(), &data.assets),
                    |from| session.advancement_choices(from),
                )
            };
            movement
        } else {
            None
        };
        if let Some(action) = requested {
            let reselect = match &action {
                Action::Move { to, .. } => Some(*to),
                Action::Attack(attack) => Some(attack.from),
                _ => None,
            };
            let advancement = menu.as_ref().and_then(menu::Menu::preferred_advancement);
            match session.apply_with_advancement(action, advancement) {
                Ok(_) => {
                    message = None;
                    menu = None;
                    selection.clear();
                    if let Some(observation) = session.observation() {
                        if let Some(position) = reselect {
                            selection.select_own(position, observation);
                        }
                        art.load(observation, &data.assets);
                    }
                }
                Err(error) => message = Some(error.to_string()),
            }
        }
        render::draw(
            &data.title,
            session.state(),
            session.observation(),
            camera.layout(),
            &art,
            render::MovementView {
                global_time: session.global_time_of_day(),
                selected: selection.unit(),
                actions: session.legal_actions(),
                message: message.as_deref(),
                assets: &data.assets,
                menu: menu.as_ref(),
            },
        );
        if session.legal_actions().contains(&Action::EndTurn)
            && let Some(bounds) = button
        {
            let highlighted = bounds.contains(pointer) || is_key_down(KeyCode::Space);
            render::draw_end_turn_button(bounds, highlighted, &art);
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
                position: start, side: 1, type_id: "Bowman".into(),
                experience: 0, max_experience: 35,
                hitpoints: 33, moves_left: Some(5), attacks_left: Some(1),
                poisoned: false, slowed: false,
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
