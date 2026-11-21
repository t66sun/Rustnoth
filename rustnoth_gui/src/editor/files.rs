use crate::dialog::{self, Dialog, ListAction, ListPosition};
use macroquad::prelude::*;
use std::{
    fs,
    io::{self, Write},
    path::{Component, Path, PathBuf},
};

pub(super) async fn confirm(dialog: &Dialog) -> bool {
    confirm_choice(dialog, "Discard unsaved map changes?", "Keep editing", "Discard changes").await
}

async fn confirm_choice(dialog: &Dialog, title: &str, cancel: &str, accept: &str) -> bool {
    next_frame().await;
    let mut picker = ListPosition::default();
    loop {
        let frame = dialog.begin(245.0);
        let response = dialog.choose(
            &frame,
            &mut picker,
            2,
            None,
            title,
            None,
            |index| {
                (
                    if index == 0 {
                        cancel
                    } else {
                        accept
                    }
                    .into(),
                    None,
                )
            },
        );
        set_default_camera();
        next_frame().await;
        match response {
            ListAction::Choose(index) => return index == 1,
            ListAction::Back => return false,
            ListAction::Idle => {}
        }
    }
}

async fn prompt(dialog: &Dialog, title: &str, default: &str, accept: &str) -> Option<String> {
    let mut value = default.to_owned();
    let mut replace = !value.is_empty();
    next_frame().await;
    while get_char_pressed().is_some() {}
    loop {
        let frame = dialog.begin(202.0);
        let r = frame.panel;
        let font = dialog.font.as_ref();
        dialog::text(font, title, r.x + 18.0, r.y + 38.0, 25, dialog::GOLD);
        let field = Rect::new(r.x + 20.0, r.y + 60.0, r.w - 40.0, 40.0);
        draw_rectangle(
            field.x,
            field.y,
            field.w,
            field.h,
            if replace { DARKBLUE } else { BLACK },
        );
        draw_rectangle_lines(field.x, field.y, field.w, field.h, 1.5, dialog::GOLD);
        if is_key_pressed(KeyCode::Backspace) {
            if replace {
                value.clear();
            } else {
                value.pop();
            }
            replace = false;
        }
        while let Some(c) = get_char_pressed() {
            if !c.is_control() {
                if replace {
                    value.clear();
                    replace = false;
                }
                value.push(c);
            }
        }
        let shown = dialog::fit(font, value.clone(), field.w - 14.0, 18);
        dialog::text(font, &shown, field.x + 6.0, field.y + 26.0, 18, WHITE);
        let cancel = Rect::new(r.right() - 117.0, r.bottom() - 52.0, 99.0, 36.0);
        let ok = Rect::new(cancel.x - 116.0, cancel.y, 105.0, 36.0);
        for (b, name) in [(ok, accept), (cancel, "Cancel")] {
            dialog::button(b, b.contains(frame.mouse), false);
            dialog::text(font, name, b.x + 10.0, b.y + 24.0, 17, dialog::PALE);
        }
        let canceled =
            is_key_pressed(KeyCode::Escape) || frame.clicked && cancel.contains(frame.mouse);
        let confirmed = is_key_pressed(KeyCode::Enter) || frame.clicked && ok.contains(frame.mouse);
        set_default_camera();
        next_frame().await;
        if canceled {
            return None;
        }
        if confirmed && !value.trim().is_empty() {
            return Some(value.trim().to_owned());
        }
    }
}

fn maps_in(folder: &Path, label: &str, out: &mut Vec<(String, PathBuf)>) -> io::Result<()> {
    if !folder.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(folder)? {
        let path = entry?.path();
        if path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("map"))
            && path.is_file()
        {
            let name = path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .replace('_', " ");
            out.push((format!("{label} / {name}"), path));
        }
    }
    Ok(())
}

pub(super) async fn open(
    root: &Path,
    folder: &Path,
    dialog: &Dialog,
) -> io::Result<Option<PathBuf>> {
    let mut files = Vec::new();
    maps_in(folder, "Saved Maps", &mut files)?;
    maps_in(
        &root.join("data/multiplayer/maps"),
        "Multiplayer",
        &mut files,
    )?;
    let campaigns = root.join("data/campaigns");
    if campaigns.is_dir() {
        for e in fs::read_dir(campaigns)? {
            let e = e?;
            maps_in(
                &e.path().join("maps"),
                &e.file_name().to_string_lossy().replace('_', " "),
                &mut files,
            )?;
        }
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    let mut picker = ListPosition::default();
    next_frame().await;
    loop {
        let frame = dialog.begin(580.0);
        let choice = dialog.choose(
            &frame,
            &mut picker,
            1 + files.len(),
            None,
            "Open Map",
            None,
            |i| {
                (
                    if i == 0 {
                        "Enter file path…".to_owned()
                    } else {
                        files[i - 1].0.clone()
                    },
                    None,
                )
            },
        );
        set_default_camera();
        next_frame().await;
        match choice {
            ListAction::Choose(0) => {
                return Ok(prompt(dialog, "Open Map", "", "Open")
                    .await
                    .map(PathBuf::from));
            }
            ListAction::Choose(i) => return Ok(Some(files[i - 1].1.clone())),
            ListAction::Back => return Ok(None),
            ListAction::Idle => {}
        }
    }
}

pub(super) async fn save(
    folder: &Path,
    installation: &Path,
    current: Option<&Path>,
    save_as: bool,
    other_open: &[std::path::PathBuf],
    dialog: &Dialog,
    proposed: &str,
    data: &str,
) -> io::Result<Option<PathBuf>> {
    if !save_as && let Some(path) = current {
        let installation = fs::canonicalize(installation)?;
        if !path.starts_with(installation) {
            write_map(path, data)?;
            return Ok(Some(path.to_owned()));
        }
    }
    let Some(name) = prompt(dialog, "Save Map As", proposed, "Save").await else {
        return Ok(None);
    };
    let path = destination(folder, &name)?;
    fs::create_dir_all(folder)?;
    if path.exists() {
        let existing = path.canonicalize()?;
        if other_open.contains(&existing) {
            return Err(io::Error::new(io::ErrorKind::AlreadyExists, "This map is open in another editor window."));
        }
        if !confirm_choice(dialog, "Overwrite the existing map?", "Cancel", "Overwrite").await { return Ok(None); }
        write_map(&path, data)?;
        return Ok(Some(path));
    }
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&path)?;
    if let Err(error) = file.write_all(data.as_bytes()).and_then(|_| file.flush()) {
        drop(file);
        let _ = fs::remove_file(&path);
        return Err(error);
    }
    Ok(Some(path))
}

fn write_map(path: &Path, data: &str) -> io::Result<()> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static WRITES: AtomicU64 = AtomicU64::new(0);
    let folder = path.parent().ok_or(io::ErrorKind::InvalidInput)?;
    let (pending, mut file) = loop {
        let id = WRITES.fetch_add(1, Ordering::Relaxed);
        let pending = folder.join(format!(".rustnoth-map-{}-{id}.tmp", std::process::id()));
        match fs::OpenOptions::new().write(true).create_new(true).open(&pending) {
            Ok(file) => break (pending, file),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    };
    let result = file.write_all(data.as_bytes()).and_then(|_| file.flush());
    drop(file);
    if let Err(e) = result.and_then(|_| fs::rename(&pending, path)) {
        let _ = fs::remove_file(&pending);
        return Err(e);
    }
    Ok(())
}

fn destination(folder: &Path, name: &str) -> io::Result<PathBuf> {
    let mut parts = Path::new(name).components();
    if !matches!(parts.next(), Some(Component::Normal(_)))
        || parts.next().is_some()
        || name.starts_with('.')
        || name.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|'])
        || name.ends_with(['.', ' '])
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Enter a file name inside the Saved Maps folder.",
        ));
    }
    Ok(folder.join(if name.to_ascii_lowercase().ends_with(".map") {
        name.to_owned()
    } else {
        format!("{name}.map")
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn saving_a_named_map_replaces_the_existing_contents() {
        let dir = std::env::temp_dir().join(format!("rustnoth-map-save-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("My Map.map");
        fs::write(&path, "Gg, Gg\n").unwrap();
        write_map(&path, "Ke, Ff\n").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "Ke, Ff\n");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        fs::remove_file(path).unwrap();
        fs::remove_dir(dir).unwrap();
    }
    #[test]
    fn map_copies_can_only_be_named_inside_the_editor_folder() {
        let folder = Path::new("maps");
        assert_eq!(
            destination(folder, "East Bank").unwrap(),
            folder.join("East Bank.map")
        );
        assert!(destination(folder, "../installed.map").is_err());
        assert!(destination(folder, r"D:\game\stock.map").is_err());
    }
}
