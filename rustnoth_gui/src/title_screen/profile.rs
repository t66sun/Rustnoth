use engine_lite::wml::{self, Node};
use std::{fs, io, path::PathBuf};

#[cfg(any(target_os = "macos", windows))]
fn preferences_file() -> Option<PathBuf> {
    native_data_directory().map(|dir| dir.join("preferences"))
}
#[cfg(not(any(target_os = "macos", windows)))]
fn preferences_file() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()).map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".config")))?;
    Some(base.join("wesnoth/preferences"))
}

pub(crate) fn read_profile() -> io::Result<Node> {
    let Some(path) = preferences_file() else { return Ok(Node::default()); };
    match fs::read(path) {
        Ok(bytes) => wml::parse(bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Node::default()),
        Err(e) => Err(e),
    }
}

#[cfg(windows)]
pub(super) fn native_data_directory() -> Option<PathBuf> {
    use std::{ffi::OsString, os::windows::ffi::OsStringExt};
    // CSIDL_PERSONAL is the actual Documents folder, including redirected OneDrive folders.
    #[link(name = "shell32")]
    unsafe extern "system" {
        fn SHGetFolderPathW(window: *mut std::ffi::c_void, folder: i32, token: *mut std::ffi::c_void,
            flags: u32, output: *mut u16) -> i32;
    }
    let mut path = [0u16; 260];
    let result = unsafe { SHGetFolderPathW(std::ptr::null_mut(), 5, std::ptr::null_mut(), 0, path.as_mut_ptr()) };
    if result != 0 { return None; }
    let end = path.iter().position(|&c| c == 0)?;
    Some(PathBuf::from(OsString::from_wide(&path[..end])).join("My Games/Wesnoth1.18"))
}

#[cfg(target_os = "macos")]
pub(super) fn native_data_directory() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("HOME")?).join("Library/Application Support/Wesnoth_1.18"))
}

#[cfg(not(any(target_os = "macos", windows)))]
pub(super) fn native_data_directory() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_DATA_HOME").filter(|v| !v.is_empty()).map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".local/share")))?;
    Some(base.join("wesnoth/1.18"))
}
