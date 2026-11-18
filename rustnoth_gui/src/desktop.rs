//! Locate original artwork at desktop startup and prepare game data on first use.
use std::{
    collections::hash_map::DefaultHasher,
    env, fs,
    hash::{Hash, Hasher},
    io,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};

const CAMPAIGN: &str = "data/campaigns/The_South_Guard";
pub(super) const TUTORIAL: &str = "data/campaigns/tutorial";
const BINARY: &str = if cfg!(windows) { "wesnoth.exe" } else { "wesnoth" };

pub(super) struct GameData {
    pub core: Vec<u8>,
    pub campaign: Option<(PathBuf, PathBuf)>,
}

pub(super) fn campaign_available(root: &Path) -> bool {
    root.join(CAMPAIGN).join("_main.cfg").is_file() && root.join(CAMPAIGN).join("maps").is_dir()
}

fn steam_libraries(config: &str) -> impl Iterator<Item = PathBuf> + '_ {
    config.lines().filter_map(|line| {
        let value = line.trim_start().strip_prefix("\"path\"")?.trim();
        let path = value.strip_prefix('"')?.strip_suffix('"')?;
        Some(PathBuf::from(path.replace("\\\\", "\\")))
    })
}

fn steam_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(steam) = env::var_os("STEAM_DIR") { roots.push(PathBuf::from(steam)); }
    if let Some(programs) = env::var_os("ProgramFiles(x86)") {
        roots.push(PathBuf::from(programs).join("Steam"));
    }
    if let Some(home) = env::var_os("HOME") {
        let home = PathBuf::from(home);
        roots.extend([".steam/steam", ".local/share/Steam", "Library/Application Support/Steam"]
            .map(|dir| home.join(dir)));
    }
    roots
}

fn has_art(root: &Path) -> bool {
    root.join("data/core/_main.cfg").is_file() && root.join("images/misc/logo.png").is_file()
}

pub(super) fn locate(explicit: Option<&Path>) -> io::Result<PathBuf> {
    if let Some(root) = explicit.map(PathBuf::from).or_else(|| env::var_os("WESNOTH_ROOT").map(PathBuf::from)) {
        if has_art(&root) { return std::path::absolute(root); }
        return Err(io::Error::new(io::ErrorKind::NotFound, format!("Missing Wesnoth data in {}", root.display())));
    }
    let mut candidates = Vec::new();
    if let Ok(exe) = env::current_exe() && let Some(parent) = exe.parent() {
        candidates.push(parent.to_owned());
    }
    for steam in steam_roots() {
        let libraries = fs::read_to_string(steam.join("steamapps/libraryfolders.vdf")).unwrap_or_default();
        candidates.push(steam.join("steamapps/common/wesnoth"));
        candidates.extend(steam_libraries(&libraries).map(|dir| dir.join("steamapps/common/wesnoth")));
    }
    if cfg!(unix) {
        candidates.extend(["/usr/share/games/wesnoth/1.18", "/usr/local/share/wesnoth",
            "/Applications/The Battle for Wesnoth.app/Contents/Resources"].map(PathBuf::from));
    }
    candidates.extend([PathBuf::from("."),PathBuf::from("../wesnoth_src")]);
    let root = candidates.into_iter().find(|dir| has_art(dir)).ok_or_else(|| io::Error::new(
        io::ErrorKind::NotFound, "Install Wesnoth 1.18.8 or set WESNOTH_ROOT to its installation directory."
    ))?;
    std::path::absolute(root)
}

fn executable(root: &Path) -> io::Result<PathBuf> {
    let mut candidates = vec![root.join(BINARY), root.join("bin").join(BINARY)];
    if let Some(paths) = env::var_os("PATH") {
        candidates.extend(env::split_paths(&paths).map(|path| path.join(BINARY)));
    }
    candidates.into_iter().find(|p| p.is_file()).ok_or_else(|| io::Error::new(io::ErrorKind::NotFound,
        "Game data preparation requires the Wesnoth 1.18.8 executable in the installation directory or PATH."))
}

fn cache_directory() -> io::Result<PathBuf> {
    let base = if cfg!(windows) { env::var_os("LOCALAPPDATA").map(PathBuf::from) }
        else { env::var_os("XDG_CACHE_HOME").map(PathBuf::from).or_else(||
            env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache"))) };
    Ok(base.ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "No user cache directory found"))?
        .join("Rustnoth").join("1.18.8"))
}

fn prepared(root: &Path, binary: &Path, cache: &Path, source: &str, defines: &str) -> io::Result<PathBuf> {
    let input = root.join(source).join("_main.cfg");
    let mut hash = DefaultHasher::new();
    input.hash(&mut hash);
    defines.hash(&mut hash);
    fs::metadata(&input)?.modified()?.hash(&mut hash);
    fs::metadata(binary)?.modified()?.hash(&mut hash);
    let output = cache.join(format!("{:016x}.cfg", hash.finish()));
    if output.is_file() { return Ok(output); }

    engine_lite::native_process::check_version(binary)?;
    fs::create_dir_all(cache)?;
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
    let stage = cache.join(format!("prepare-{}-{nonce}", std::process::id()));
    fs::create_dir(&stage)?;
    let result = (|| {
        let mut process = Command::new(binary);
        #[cfg(windows)] {
            use std::os::windows::process::CommandExt;
            process.creation_flags(0x0800_0000).arg("--wnoconsole");
        }
        let result = process.arg("--data-dir").arg(root)
            .arg("--userconfig-dir").arg(stage.join("config"))
            .arg("--usercache-dir").arg(stage.join("cache"))
            .arg("--userdata-dir").arg(stage.join("userdata"))
            .arg("--no-log-to-file").arg(format!("--preprocess-defines={defines}"))
            .arg("--preprocess").arg(&input).arg(&stage)
            .stdin(Stdio::null()).output()?;
        if !result.status.success() {
            let stderr = String::from_utf8_lossy(&result.stderr);
            let detail = stderr.trim().lines().next_back().unwrap_or("preprocessor failed");
            return Err(io::Error::other(format!("{}: {detail} ({})", input.display(), result.status)));
        }
        let generated = stage.join("_main.cfg");
        if fs::metadata(&generated)?.len() == 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidData,"Wesnoth produced an empty configuration"));
        }
        match fs::rename(generated, &output) {
            Ok(()) => Ok(()),
            Err(_) if output.is_file() => Ok(()),
            Err(error) => Err(error),
        }
    })();
    let _ = fs::remove_dir_all(&stage);
    result?;
    Ok(output)
}

pub(super) fn prepare(root: &Path, campaign: bool) -> io::Result<GameData> {
    let binary = executable(root)?;
    let cache = cache_directory()?;
    let core = prepared(root,&binary,&cache,"data/core","SKIP_CORE,NO_TERRAIN_GFX")?;
    let campaign = if campaign {
        let file = prepared(root,&binary,&cache,CAMPAIGN,"NO_TERRAIN_GFX,CAMPAIGN_THE_SOUTH_GUARD,NORMAL")?;
        Some((file,root.join(CAMPAIGN).join("maps")))
    } else { None };
    Ok(GameData { core: fs::read(core)?, campaign })
}

pub(super) fn prepare_tutorial(root: &Path) -> io::Result<PathBuf> {
    prepared(root, &executable(root)?, &cache_directory()?, TUTORIAL, "NO_TERRAIN_GFX,TUTORIAL,NORMAL")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installation_opens_the_title_before_any_game_data_is_prepared() {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let root = env::temp_dir().join(format!("rustnoth-launch-{}-{nonce}", std::process::id()));
        for relative in ["data/core/_main.cfg","images/misc/logo.png"] {
            let file = root.join(relative);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, []).unwrap();
        }
        let args = ["--wesnoth-root".into(), root.as_os_str().to_owned()];
        let crate::Launch::Title(title) = crate::Launch::from_args(&args).unwrap() else {
            panic!("expected installed title screen");
        };
        assert_eq!(title.root, root);
        assert!(matches!(title.games, crate::title_screen::Games::Installed));
        assert!(!campaign_available(&root));
        fs::create_dir_all(root.join(CAMPAIGN).join("maps")).unwrap();
        fs::write(root.join(CAMPAIGN).join("_main.cfg"), []).unwrap();
        assert!(campaign_available(&root));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn discovers_secondary_steam_libraries_with_spaces_and_escaped_windows_paths() {
        let config = r#""libraryfolders"
{
    "0"
    {
        "path"  "C:\\Program Files (x86)\\Steam"
    }
    "1"
    {
        "path"  "D:\\Installed Games"
    }
}"#;
        assert_eq!(steam_libraries(config).collect::<Vec<_>>(), [
            PathBuf::from(r"C:\Program Files (x86)\Steam"), PathBuf::from(r"D:\Installed Games")]);
    }
}
