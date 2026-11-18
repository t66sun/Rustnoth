//! Rustnoth's per-user window mode, shared by every desktop launch path.
use std::{env, fs, io, path::{Path, PathBuf}};

pub(super) fn config_directory() -> io::Result<PathBuf> {
    #[cfg(target_os = "windows")]
    let directory = env::var_os("APPDATA").filter(|p| !p.is_empty()).map(PathBuf::from);
    #[cfg(target_os = "macos")]
    let directory = env::var_os("HOME").map(|home| PathBuf::from(home).join("Library/Application Support"));
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let directory = env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).filter(|path| path.is_absolute())
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")));
    directory.map(|path| path.join("Rustnoth"))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Cannot locate the user configuration directory"))
}

fn load_from(directory: &Path) -> io::Result<bool> {
    match fs::read_to_string(directory.join("window-mode")) {
        Ok(value) => match value.trim() {
            "fullscreen" => Ok(true),
            "windowed" => Ok(false),
            _ => Err(io::Error::new(io::ErrorKind::InvalidData, "Unknown Rustnoth window mode")),
        },
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn save_to(directory: &Path, fullscreen: bool) -> io::Result<()> {
    fs::create_dir_all(directory)?;
    let pending = directory.join(format!("window-mode.{}.tmp", std::process::id()));
    fs::write(&pending, if fullscreen { "fullscreen\n" } else { "windowed\n" })?;
    fs::rename(pending, directory.join("window-mode"))
}

pub fn load() -> io::Result<bool> { load_from(&config_directory()?) }

pub fn save(fullscreen: bool) -> io::Result<()> { save_to(&config_directory()?, fullscreen) }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_modes_are_restored_across_launches() {
        let unique = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = env::temp_dir().join(format!("rustnoth-display-{}-{unique}", std::process::id()));
        assert!(!load_from(&dir).unwrap());
        save_to(&dir, true).unwrap();
        assert!(load_from(&dir).unwrap());
        save_to(&dir, false).unwrap();
        assert!(!load_from(&dir).unwrap());
        fs::remove_file(dir.join("window-mode")).unwrap();
        fs::remove_dir(dir).unwrap();
    }
}
