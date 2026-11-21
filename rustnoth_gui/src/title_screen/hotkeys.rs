//! Keyboard bindings for the commands registered on the 1.18 title screen.
use super::Command;
use macroquad::prelude::*;
use std::{fs, io, path::{Path, PathBuf}};

pub(super) const ACTIONS: &[(Command, &str, &str)] = &[
    (Command::Language, "changelanguage", "Change Language"),
    (Command::Help, "help", "Help"),
    (Command::Load, "load", "Load Game"),
    (Command::Addons, "title_screen__addons", "Manage Add-ons"),
    (Command::Next, "title_screen__next_tip", "Next Tip of the Day"),
    (Command::Preferences, "preferences", "Preferences"),
    (Command::Previous, "title_screen__previous_tip", "Previous Tip of the Day"),
    (Command::Quit, "quit-to-desktop", "Quit to Desktop"),
    (Command::Achievements, "achievements", "Achievements"),
    (Command::Credits, "title_screen__credits", "Show Credits"),
    (Command::Campaigns, "title_screen__campaign", "Start Campaign"),
    (Command::Editor, "title_screen__editor", "Start Editor"),
    (Command::Multiplayer, "title_screen__multiplayer", "Start Multiplayer Game"),
];

const CTRL: u8 = 1;
const ALT: u8 = 2;
const SHIFT: u8 = 4;
const SUPER: u8 = 8;

fn chord(mut key: String, mods: u8) -> String {
    for (flag, prefix) in [(SUPER,"Cmd+"),(SHIFT,"Shift+"),(ALT,"Alt+"),(CTRL,"Ctrl+")] {
        if mods & flag != 0 { key.insert_str(0,prefix); }
    }
    key
}

pub(super) fn key_name(key: KeyCode) -> Option<String> {
    use KeyCode::*;
    match key {
        LeftControl | RightControl | LeftAlt | RightAlt | LeftShift | RightShift |
        LeftSuper | RightSuper | Escape | Enter | KpEnter | Up | Down | Tab | F12 | Unknown => None,
        _ => Some(format!("{key:?}").to_ascii_uppercase()),
    }
}

pub(super) fn pressed() -> Option<String> {
    use KeyCode::*;
    let mut mods = 0;
    for (left,right,flag) in [(LeftControl,RightControl,CTRL), (LeftAlt,RightAlt,ALT),
        (LeftShift,RightShift,SHIFT), (LeftSuper,RightSuper,SUPER)] {
        if is_key_down(left) || is_key_down(right) { mods |= flag; }
    }
    get_keys_pressed().into_iter().filter_map(key_name).min().map(|key| chord(key, mods))
}

#[derive(Clone)]
pub(super) struct Bindings {
    defaults: Vec<Vec<String>>,
    keys: Vec<Vec<String>>,
    path: PathBuf,
}

impl Bindings {
    // The installed core file uses two platform branches and one modifier macro.
    // Read only the keyboard bindings registered by the title screen.
    pub fn native(source: &str, path: PathBuf) -> Self {
        let mut defaults = vec![Vec::new(); ACTIONS.len()];
        let apple = cfg!(target_os="macos");
        let mut active = true;
        let mut in_block = false;
        let mut id = "";
        let mut key = "";
        let mut mods = 0;
        for line in source.lines().map(str::trim) {
            match line {
                "#ifdef APPLE" => active = apple,
                "#ifndef APPLE" => active = !apple,
                "#else" => active = !active,
                "#endif" => active = true,
                _ if !active => {}
                "[hotkey]" => { in_block = true; id = ""; key = ""; mods = 0; }
                "[/hotkey]" => {
                    if let Some(i) = ACTIONS.iter().position(|a| a.1 == id) && !key.is_empty() {
                        let key = if key == "," { "COMMA".to_owned() } else { key.to_ascii_uppercase() };
                        let value = chord(key,mods);
                        if !defaults[i].contains(&value) { defaults[i].push(value); }
                    }
                    in_block = false;
                }
                "{IF_APPLE_CMD_ELSE_CTRL}" if in_block => mods |= if apple {SUPER} else {CTRL},
                _ if in_block => {
                    if let Some((name,value)) = line.split_once('=') {
                        let value = value.trim().trim_matches('"');
                        match name.trim() {
                            "command" => id = value, "key" => key = value,
                            "ctrl" if value=="yes" => mods |= CTRL, "alt" if value=="yes" => mods |= ALT,
                            "shift" if value=="yes" => mods |= SHIFT, "cmd" if value=="yes" => mods |= SUPER,
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
        Self { keys: defaults.clone(), defaults, path }
    }

    pub fn load(root: &Path) -> io::Result<Self> {
        let source = fs::read_to_string(root.join("data/core/hotkeys.cfg"))?;
        let path = crate::display_preferences::config_directory()?.join("title-hotkeys");
        let mut result = Self::native(&source,path);
        match fs::read_to_string(&result.path) {
            Ok(data) => result.restore(&data),
            Err(e) if e.kind()==io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        Ok(result)
    }

    fn restore(&mut self, data: &str) {
        for line in data.lines() {
            let Some((id,keys)) = line.split_once('=') else { continue };
            if let Some(i) = ACTIONS.iter().position(|a| a.1 == id) {
                self.keys[i] = keys.split_whitespace().map(str::to_owned).collect();
            }
        }
    }

    pub fn display(&self, index: usize) -> String { self.keys[index].join(", ") }

    pub fn owner(&self, key: &str) -> Option<usize> {
        self.keys.iter().position(|keys| keys.iter().any(|s| s == key))
    }

    pub fn shortcut(&self, key: &str, available: &[Command]) -> Option<Command> {
        let action = ACTIONS[self.owner(key)?].0;
        (available.contains(&action) || matches!(action, Command::Help | Command::Language | Command::Next | Command::Previous)).then_some(action)
    }

    pub fn add(&mut self, index: usize, key: &str) -> io::Result<()> {
        let mut next = self.keys.clone();
        for keys in &mut next { keys.retain(|v| v != key); }
        next[index].push(key.to_owned());
        self.save(next)
    }

    pub fn clear(&mut self, index: usize) -> io::Result<()> {
        let mut next = self.keys.clone();
        next[index].clear();
        self.save(next)
    }

    pub fn reset(&mut self) -> io::Result<()> { self.save(self.defaults.clone()) }

    fn save(&mut self, next: Vec<Vec<String>>) -> io::Result<()> {
        let mut data = String::new();
        for (i,keys) in next.iter().enumerate().filter(|(i,keys)| *keys != &self.defaults[*i]) {
            data.push_str(&format!("{}={}\n",ACTIONS[i].1,keys.join(" ")));
        }
        if let Some(dir) = self.path.parent() { fs::create_dir_all(dir)?; }
        let tmp = self.path.with_extension(format!("{}.tmp",std::process::id()));
        fs::write(&tmp,data)?;
        fs::rename(tmp,&self.path)?;
        self.keys = next;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reassign_clear_and_reset_are_used_by_title_routing_after_reload() {
        let source = "[hotkey]\ncommand=title_screen__campaign\nkey=c\n[/hotkey]\n[hotkey]\ncommand=preferences\nkey=p\n{IF_APPLE_CMD_ELSE_CTRL}\n[/hotkey]";
        let file = std::env::temp_dir().join(format!("rustnoth-title-hotkeys-test-{}",std::process::id()));
        let mut keys = Bindings::native(source,file.clone());
        let menu = super::super::menu(true);
        let preferences = ACTIONS.iter().position(|a| a.0 == Command::Preferences).unwrap();
        assert_eq!(keys.shortcut("C",menu), Some(Command::Campaigns));
        let chord = if cfg!(target_os="macos") {"Cmd+P"} else {"Ctrl+P"};
        assert_eq!(keys.shortcut(chord,menu), Some(Command::Preferences));
        keys.add(preferences,"C").unwrap();
        let mut reloaded = Bindings::native(source,file.clone());
        reloaded.restore(&fs::read_to_string(&file).unwrap());
        assert_eq!(reloaded.shortcut("C",menu),Some(Command::Preferences));
        assert_eq!(reloaded.shortcut(chord,menu),Some(Command::Preferences));
        reloaded.clear(preferences).unwrap();
        assert_eq!(reloaded.shortcut("C",menu),None);
        reloaded.reset().unwrap();
        assert_eq!(reloaded.shortcut("C",menu),Some(Command::Campaigns));
        assert_eq!(reloaded.shortcut("C",super::super::menu(false)),None);
        fs::remove_file(file).unwrap();
    }
}
