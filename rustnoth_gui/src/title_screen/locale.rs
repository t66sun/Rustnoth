//! The two gettext domains used on Wesnoth's title screen.
use engine_lite::wml;
use std::{collections::HashMap, env, fs, io, path::Path};

#[derive(Default)]
pub(crate) struct Locale {
    pub selected: String,
    pub effective: String,
    game: Catalog,
    interface: Catalog,
    domains: HashMap<String, Catalog>,
}

#[derive(Default)]
pub(super) struct Catalog(HashMap<String, String>);

impl Catalog {
    pub(super) fn installed(root: &Path, language: &str, domain: &str) -> io::Result<Self> {
        let identifier = |name: &str| !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b"_@-".contains(&b));
        if !identifier(language) || !identifier(domain) || matches!(language, "en_US" | "C") {
            return Ok(Self::default());
        }
        for id in candidates(language) {
            let path = root.join("translations").join(id).join("LC_MESSAGES").join(format!("{domain}.mo"));
            match fs::read(path) {
                Ok(bytes) => return Self::read(&bytes),
                Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e),
            }
        }
        Ok(Self::default())
    }
    fn read(bytes: &[u8]) -> io::Result<Self> {
        let invalid = || io::Error::new(io::ErrorKind::InvalidData, "Invalid gettext catalog");
        let little = match bytes.get(..4) {
            Some([0xde, 0x12, 0x04, 0x95]) => true,
            Some([0x95, 0x04, 0x12, 0xde]) => false,
            _ => return Err(invalid()),
        };
        let word = |pos: usize| -> io::Result<usize> {
            let value = bytes
                .get(pos..)
                .and_then(|s| s.get(..4))
                .ok_or_else(invalid)?;
            let word = value.try_into().unwrap();
            Ok(if little {
                u32::from_le_bytes(word)
            } else {
                u32::from_be_bytes(word)
            } as usize)
        };
        let count = word(8)?;
        let source = word(12)?;
        let translated = word(16)?;
        let row = |table: usize, index: usize| -> io::Result<&str> {
            let entry = table
                .checked_add(index.checked_mul(8).ok_or_else(invalid)?)
                .ok_or_else(invalid)?;
            let len = word(entry)?;
            let offset = word(entry.checked_add(4).ok_or_else(invalid)?)?;
            let slice = bytes
                .get(offset..)
                .and_then(|s| s.get(..len))
                .ok_or_else(invalid)?;
            std::str::from_utf8(slice).map_err(|_| invalid())
        };
        let mut catalog = HashMap::new();
        for i in 0..count {
            let original = row(source, i)?;
            let target = row(translated, i)?;
            if !original.is_empty() && !original.contains('\0') && !target.is_empty() {
                catalog.insert(original.to_string(), target.to_string());
            }
        }
        Ok(Self(catalog))
    }

    pub(super) fn text<'a>(&'a self, key: &'a str) -> &'a str {
        self.0.get(key).map(String::as_str).unwrap_or(key)
    }
}

#[derive(Debug)]
pub(super) struct Language {
    pub id: String,
    pub name: String,
    sort: String,
    pub percent: u8,
}

pub(super) fn list(root: &Path) -> io::Result<Vec<Language>> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(root.join("data/languages"))? {
        let path = entry?.path();
        if path.extension().is_none_or(|e| e != "cfg") {
            continue;
        }
        let tree = wml::parse(fs::read(&path)?)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        for (_, node) in tree.children.iter().filter(|(tag, _)| tag == "locale") {
            let (Some(id), Some(name)) = (node.attribute("locale"), node.attribute("name")) else {
                continue;
            };
            entries.push(Language {
                id: id.into(),
                name: name.into(),
                sort: node.attribute("sort_name").unwrap_or(name).to_lowercase(),
                percent: node
                    .attribute("percent")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0),
            });
        }
    }
    entries.sort_by(|a, b| a.sort.cmp(&b.sort));
    entries.insert(
        0,
        Language {
            id: String::new(),
            name: "System default language".into(),
            sort: String::new(),
            percent: 100,
        },
    );
    Ok(entries)
}

fn system_language() -> String {
    for variable in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(value) = env::var(variable)
            && !value.is_empty()
        {
            return value.split('.').next().unwrap_or("").replace('-', "_");
        }
    }
    #[cfg(windows)]
    {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetUserDefaultLocaleName(name: *mut u16, capacity: i32) -> i32;
        }
        let mut tag = [0u16; 85];
        // Windows includes a terminating NUL in the number of written code units.
        let length = unsafe { GetUserDefaultLocaleName(tag.as_mut_ptr(), tag.len() as i32) };
        if length > 1 {
            return String::from_utf16_lossy(&tag[..length as usize - 1]).replace('-', "_");
        }
    }
    "en_US".into()
}

fn candidates(id: &str) -> Vec<String> {
    let (territory, modifier) = id.split_once('@').map_or((id, None), |(t, m)| (t, Some(m)));
    let base = territory.split('_').next().unwrap_or(id);
    let mut result = vec![id.to_owned()];
    if let Some(modifier) = modifier {
        result.push(format!("{base}@{modifier}"));
    }
    result.push(territory.into());
    result.push(base.into());
    result.dedup();
    result
}

pub(crate) fn font_name(id: &str) -> &'static str {
    match id.split(['_', '@']).next().unwrap_or("") {
        "zh" | "ko" => "DroidSansFallbackFull.ttf",
        "ja" => "NotoSansJP-VariableFont_wght.ttf",
        "bn" => "Lohit-Bengali.ttf",
        "ar" | "he" => "DejaVuSans.ttf",
        "en" if id.ends_with("@shaw") => "Andagii.ttf",
        _ => "Lato-Regular.ttf",
    }
}

impl Locale {
    pub fn attribute(&mut self, root: &Path, node: &wml::Node, key: &str, fallback: &str) -> String {
        let Some(value) = node.attributes.get(key).filter(|v| !v.text.is_empty()) else {
            return fallback.to_owned();
        };
        let mut text = String::new();
        let mut previous = 0;
        for segment in &value.translations {
            text.push_str(&value.text[previous..segment.bytes.start]);
            let original = &value.text[segment.bytes.clone()];
            let catalog = match segment.domain.as_str() {
                "wesnoth" => &self.game,
                "wesnoth-lib" => &self.interface,
                domain => self.domains.entry(domain.to_owned()).or_insert_with(|| {
                    Catalog::installed(root, &self.effective, domain).unwrap_or_default()
                }),
            };
            let translated = catalog.text(original);
            text.push_str(translated.split_once('^').map_or(translated, |(_, display)| display));
            previous = segment.bytes.end;
        }
        text.push_str(&value.text[previous..]);
        text
    }

    pub fn load(root: &Path, selected: String) -> io::Result<Self> {
        let effective = if selected.is_empty() {
            system_language()
        } else {
            selected.clone()
        };
        let mut locale = Self {
            selected,
            effective,
            ..Default::default()
        };
        locale.game = Catalog::installed(root, &locale.effective, "wesnoth")?;
        locale.interface = Catalog::installed(root, &locale.effective, "wesnoth-lib")?;
        Ok(locale)
    }

    pub fn logo(&self, root: &Path) -> String {
        if self
            .effective
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_@-".contains(&b))
        {
            for id in candidates(&self.effective) {
                let name = format!("images/misc/l10n/{id}/logo.png");
                if root.join(&name).is_file() {
                    return name;
                }
            }
        }
        "images/misc/logo.png".into()
    }

    pub fn title_map(&self, root: &Path) -> (String, Option<String>) {
        let maps = "data/core/images/maps";
        let base = format!("{maps}/titlescreen.webp");
        if !self.effective.bytes().all(|b| b.is_ascii_alphanumeric() || b"_@-".contains(&b)) {
            return (base, None);
        }
        let names = candidates(&self.effective);
        let localized = |file: &str| names.iter()
            .map(|id| format!("{maps}/l10n/{id}/{file}"))
            .find(|path| root.join(path).is_file());
        if let Some(replacement) = localized("titlescreen.webp") {
            return (replacement, None);
        }
        (base, localized("titlescreen--overlay.webp"))
    }

    pub fn ui<'a>(&'a self, key: &'a str) -> &'a str {
        self.interface.text(key)
    }
    pub fn game<'a>(&'a self, key: &'a str) -> &'a str {
        self.game.text(key)
    }
}

pub(crate) fn saved(dir: &Path) -> io::Result<String> {
    match fs::read_to_string(dir.join("language")) {
        Ok(value) => Ok(value.trim().to_owned()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(e),
    }
}

pub(super) fn save(dir: &Path, id: &str) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    let pending = dir.join(format!("language.{}.tmp", std::process::id()));
    fs::write(&pending, id)?;
    fs::rename(pending, dir.join("language"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_confirmed_language_reopens_with_the_installed_localized_logo() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = env::temp_dir().join(format!(
            "rustnoth-languages-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(root.join("images/misc/l10n/zh_CN")).unwrap();
        fs::write(root.join("images/misc/l10n/zh_CN/logo.png"), []).unwrap();
        assert_eq!(saved(&root).unwrap(), "");
        save(&root, "zh_CN").unwrap();
        let active = Locale::load(&root, saved(&root).unwrap()).unwrap();
        assert_eq!(active.effective, "zh_CN");
        assert_eq!(active.logo(&root), "images/misc/l10n/zh_CN/logo.png");
        let maps = "data/core/images/maps";
        let original = format!("{maps}/titlescreen.webp");
        let overlay = format!("{maps}/l10n/zh/titlescreen--overlay.webp");
        let replacement = format!("{maps}/l10n/zh_CN/titlescreen.webp");
        for path in [&overlay, &replacement] {
            fs::create_dir_all(root.join(path).parent().unwrap()).unwrap();
        }
        fs::write(root.join(&overlay), []).unwrap();
        assert_eq!(active.title_map(&root), (original.clone(), Some(overlay)));
        fs::write(root.join(&replacement), []).unwrap();
        assert_eq!(active.title_map(&root), (replacement, None));
        assert_eq!(Locale::load(&root, "en_US".into()).unwrap().title_map(&root), (original, None));
        save(&root, "en_US").unwrap();
        assert_eq!(
            Locale::load(&root, saved(&root).unwrap())
                .unwrap()
                .logo(&root),
            "images/misc/logo.png"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn translates_only_authored_segments_using_each_campaign_domain() {
        let mut locale = Locale::default();
        locale.game.0.insert("Hard".into(), "困难".into());
        locale.domains.insert("wesnoth-did".into(), Catalog(HashMap::from([
            ("Descent".into(), "堕落".into()),
        ])));
        let data = wml::parse(b"#textdomain wesnoth-did\nname = _ \"Descent\" + \" / \" +\n\xFEtextdomain wesnoth\n_ \"Hard\"\n").unwrap();
        assert_eq!(locale.attribute(Path::new(""), &data, "name", ""), "堕落 / 困难");
    }

    #[test]
    fn gettext_tables_decode_utf8_in_both_byte_orders_and_fall_back_to_the_message_id() {
        let value = "战役";
        for little in [true, false] {
            let mut bytes = Vec::new();
            for word in [
                0x950412de_u32,
                0,
                1,
                28,
                36,
                0,
                0,
                9,
                44,
                value.len() as u32,
                54,
            ] {
                bytes.extend(if little {
                    word.to_le_bytes()
                } else {
                    word.to_be_bytes()
                });
            }
            bytes.extend(b"Campaigns\0");
            bytes.extend(value.as_bytes());
            let catalog = Catalog::read(&bytes).unwrap();
            assert_eq!(catalog.text("Campaigns"), value);
            assert_eq!(catalog.text("Help"), "Help");
        }
        assert_eq!(
            candidates("sr_RS@latin"),
            ["sr_RS@latin", "sr@latin", "sr_RS", "sr"]
        );
        assert_eq!(font_name("zh_CN"), "DroidSansFallbackFull.ttf");
    }
}
