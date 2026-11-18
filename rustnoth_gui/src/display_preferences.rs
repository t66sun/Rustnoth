//! Rustnoth's per-user window mode, shared by every desktop launch path.
use std::{env, fs, io, path::{Path, PathBuf}};

fn config_directory() -> io::Result<PathBuf> {
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

struct Setting { file: &'static str, off: &'static str, on: &'static str }

const WINDOW: Setting = Setting { file: "window-mode", off: "windowed", on: "fullscreen" };
const GRID: Setting = Setting { file: "map-grid", off: "off", on: "on" };

impl Setting {
    fn read(&self, dir: &Path) -> io::Result<bool> {
        match fs::read_to_string(dir.join(self.file)) {
            Ok(value) if value.trim() == self.on => Ok(true),
            Ok(value) if value.trim() == self.off => Ok(false),
            Ok(_) => Err(io::Error::new(io::ErrorKind::InvalidData, format!("Unknown Rustnoth {}", self.file))),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    }

    fn write(&self, dir: &Path, enabled: bool) -> io::Result<()> {
        fs::create_dir_all(dir)?;
        let pending = dir.join(format!("{}.{}.tmp", self.file, std::process::id()));
        fs::write(&pending, format!("{}\n", if enabled { self.on } else { self.off }))?;
        fs::rename(pending, dir.join(self.file))
    }
}

pub fn load() -> io::Result<bool> { WINDOW.read(&config_directory()?) }
pub fn save(fullscreen: bool) -> io::Result<()> { WINDOW.write(&config_directory()?, fullscreen) }
pub fn grid() -> io::Result<bool> { GRID.read(&config_directory()?) }
pub fn save_grid(enabled: bool) -> io::Result<()> { GRID.write(&config_directory()?, enabled) }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_modes_are_restored_across_launches() {
        let unique = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = env::temp_dir().join(format!("rustnoth-display-{}-{unique}", std::process::id()));
        assert!(!WINDOW.read(&dir).unwrap());
        WINDOW.write(&dir, true).unwrap();
        assert!(WINDOW.read(&dir).unwrap());
        WINDOW.write(&dir, false).unwrap();
        assert!(!WINDOW.read(&dir).unwrap());
        assert!(!GRID.read(&dir).unwrap());
        GRID.write(&dir, true).unwrap();
        assert!(GRID.read(&dir).unwrap());
        assert!(!WINDOW.read(&dir).unwrap());
        fs::remove_file(dir.join("map-grid")).unwrap();
        fs::remove_file(dir.join("window-mode")).unwrap();
        fs::remove_dir(dir).unwrap();
    }
}
