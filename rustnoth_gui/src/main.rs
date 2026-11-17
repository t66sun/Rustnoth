use std::{
    env,
    error::Error,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
};

use engine_lite::{fixed_match::Action, hex::Hex, observation::Observation, scenario_bundle::Bundle, wml};
use macroquad::{Window, miniquad::conf::Conf, prelude::*};
use rustnoth_gui::{assets::AssetCatalog, selection::Selection, session::Session};

mod render;

const USAGE: &str = "rustnoth_gui <bundle.cfg> <core.cfg> <seed> [--wesnoth-root <directory>]";

fn main() -> ExitCode {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if let [flag] = args.as_slice()
        && (flag == "--help" || flag == "-h")
    {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    let data = match Options::parse(&args).and_then(BoardData::load) {
        Ok(data) => data,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let configuration = Conf {
        window_title: format!("Rustnoth - {}", data.title),
        window_width: 1200,
        window_height: 820,
        window_resizable: true,
        ..Conf::default()
    };
    Window::from_config(configuration, async move {
        if let Err(error) = show_board(data).await {
            eprintln!("{error}");
            std::process::exit(1);
        }
    });
    ExitCode::SUCCESS
}

#[derive(Debug, Eq, PartialEq)]
struct Options {
    bundle: PathBuf,
    core: PathBuf,
    seed: u32,
    wesnoth_root: PathBuf,
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
        Ok(Self { bundle: bundle.into(), core: core.into(), seed, wesnoth_root: root })
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
    fn load(options: Options) -> Result<Self, Box<dyn Error>> {
        let source = read_file(&options.bundle)?;
        let core = read_file(&options.core)?;
        let images = options.wesnoth_root.join("data/core/images");
        if !images.is_dir() {
            return Err(format!("Wesnoth image directory is missing at {}", images.display()).into());
        }
        let scenario = Bundle::parse_with_core(&source, &core)?;
        let assets = AssetCatalog::parse(&options.wesnoth_root, &core, &source)?;
        let title = wml::parse(&source)?
            .children
            .into_iter()
            .find(|(tag, _)| tag == "scenario")
            .and_then(|(_, config)| config.attribute("name").map(str::to_owned))
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "Scenario".to_owned());
        let directory = options.bundle.parent().unwrap_or_else(|| Path::new(".")).to_owned();
        Ok(Self {
            scenario,
            assets,
            title,
            directory,
            wesnoth_root: options.wesnoth_root,
            seed: options.seed,
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
    let mut art = render::BoardArt::new(&data.wesnoth_root);
    if let Some(observation) = session.observation() {
        art.load(observation, &data.assets);
    }
    let mut message = None;
    loop {
        if is_key_pressed(KeyCode::Escape) {
            return Ok(());
        }
        if is_mouse_button_pressed(MouseButton::Right) {
            selection.clear();
            message = None;
        }
        if is_mouse_button_pressed(MouseButton::Left)
            && let Some(observation) = session.observation()
            && let Some(layout) = render::board_layout(observation)
            && let Some(position) = layout.hex_at(mouse_position().into())
            && let Some(action) = select_move(&mut selection, position, observation, session.legal_actions())
        {
            match session.apply(action) {
                Ok(_) => {
                    message = None;
                    selection.clear();
                    if let Some(observation) = session.observation() {
                        selection.select_own(position, observation);
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
            &art,
            render::MovementView {
                selected: selection.unit(),
                actions: session.legal_actions(),
                message: message.as_deref(),
                assets: &data.assets,
            },
        );
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
    }

    #[test]
    fn a_map_click_moves_only_a_selected_own_unit_to_a_legal_target() {
        let start = Hex { x: 0, y: 0 };
        let destination = Hex { x: 1, y: 0 };
        let observation = Observation {
            side: 1,
            gold: Some(10),
            width: 3,
            height: 2,
            tiles: Vec::new(),
            units: vec![ObservedUnit {
                position: start, side: 1, type_id: "Bowman".into(),
                hitpoints: 33, moves_left: Some(5), attacks_left: Some(1),
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
