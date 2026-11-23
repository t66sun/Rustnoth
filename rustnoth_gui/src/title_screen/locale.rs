//! The two gettext domains used on Wesnoth's title screen.
mod plural;
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
pub(super) struct Catalog {
    messages: HashMap<String, String>,
    plural_rule: Option<String>,
}

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
        let mut catalog = Self::default();
        for i in 0..count {
            let original = row(source, i)?;
            let target = row(translated, i)?;
            if original.is_empty() {
                catalog.plural_rule = target.lines().find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    if !name.eq_ignore_ascii_case("Plural-Forms") { return None; }
                    value.split(';').find_map(|field| {
                        let (key, expr) = field.split_once('=')?;
                        (key.trim() == "plural").then(|| expr.trim().to_owned())
                    })
                });
            } else if !target.is_empty() {
                catalog.messages.insert(original.to_owned(), target.to_owned());
            }
        }
        Ok(catalog)
    }

    pub(super) fn text<'a>(&'a self, key: &'a str) -> &'a str {
        self.messages.get(key).map(String::as_str).unwrap_or(key)
    }

    fn plural<'a>(&'a self, singular: &'a str, plural: &'a str, count: u64) -> &'a str {
        let fallback = if count == 1 { singular } else { plural };
        let key = format!("{singular}\0{plural}");
        let Some(forms) = self.messages.get(&key) else { return fallback; };
        self.plural_rule.as_deref().and_then(|rule| plural::index(rule, count))
            .and_then(|i| forms.split('\0').nth(i)).filter(|form| !form.is_empty()).unwrap_or(fallback)
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
    pub(super) fn language_name<'a>(&'a self, languages: &'a [Language]) -> &'a str {
        languages.iter().find(|language| language.id == self.effective && !language.id.is_empty())
            .map(|language| language.name.as_str())
            .unwrap_or_else(|| if self.effective.eq_ignore_ascii_case("C") {
                "English (US)"
            } else {
                self.effective.as_str()
            })
    }

    pub fn attribute(&mut self, root: &Path, node: &wml::Node, key: &str, fallback: &str) -> String {
        let Some(value) = node.attributes.get(key).filter(|v| !v.text.is_empty()) else {
            return fallback.to_owned();
        };
        self.value(root, value)
    }

    pub(crate) fn value(&mut self, root: &Path, value: &wml::Value) -> String {
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

    fn localized_file(&self, root: &Path, dir: &str, file: &str) -> Option<String> {
        // Wesnoth's translators can name several resource directories in priority order.
        let key = "language code for localized resources^en_US";
        let language = self.interface.text(key);
        let language = language.split_once('^').map_or(language, |(_, id)| id);
        language.split(',').map(str::trim).chain(std::iter::once("en_US"))
            .filter(|id| !id.is_empty() && id.bytes().all(|c| c.is_ascii_alphanumeric() || b"_@-".contains(&c)))
            .map(|id| format!("{dir}/l10n/{id}/{file}"))
            .find(|path| root.join(path).is_file())
    }

    pub fn logo(&self, root: &Path) -> String {
        self.localized_file(root, "images/misc", "logo.png")
            .unwrap_or_else(|| "images/misc/logo.png".into())
    }

    pub fn title_map(&self, root: &Path) -> (String, Option<String>) {
        let maps = "data/core/images/maps";
        let base = format!("{maps}/titlescreen.webp");
        let localized = |file: &str| self.localized_file(root, maps, file);
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

    pub fn game_plural<'a>(&'a self, singular: &'a str, plural: &'a str, count: u64) -> &'a str {
        self.game.plural(singular, plural, count)
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
        let locales = root.join("data/languages");
        fs::create_dir_all(&locales).unwrap();
        fs::write(locales.join("zh_CN.cfg"), "[locale]\nlocale=zh_CN\nname=\"简体中文 (Jiăntĭ Zhōngwén)\"\n[/locale]").unwrap();
        fs::write(locales.join("en_US.cfg"), "[locale]\nlocale=en_US\nname=\"English (US)\"\n[/locale]").unwrap();
        let languages = list(&root).unwrap();
        fs::write(root.join("images/misc/l10n/zh_CN/logo.png"), []).unwrap();
        assert_eq!(saved(&root).unwrap(), "");
        save(&root, "zh_CN").unwrap();
        let mut active = Locale::load(&root, saved(&root).unwrap()).unwrap();
        active.interface.messages.insert("language code for localized resources^en_US".into(), "zh_CN,zh".into());
        assert_eq!(active.effective, "zh_CN");
        assert_eq!(active.language_name(&languages), "简体中文 (Jiăntĭ Zhōngwén)");
        assert_eq!(Locale::load(&root, "en_US".into()).unwrap().language_name(&languages), "English (US)");
        assert_eq!(Locale::load(&root, "C".into()).unwrap().language_name(&languages), "English (US)");
        assert_eq!(Locale::load(&root, "x_TEST".into()).unwrap().language_name(&languages), "x_TEST");
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
        fs::create_dir_all(root.join("images/misc/l10n/en")).unwrap();
        fs::write(root.join("images/misc/l10n/en/logo.png"), []).unwrap();
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
        locale.game.messages.insert("Hard".into(), "困难".into());
        locale.domains.insert("wesnoth-did".into(), Catalog { messages: HashMap::from([
            ("Descent".into(), "堕落".into()),
        ]), ..Default::default() });
        let data = wml::parse(b"#textdomain wesnoth-did\nname = _ \"Descent\" + \" / \" +\n\xFEtextdomain wesnoth\n_ \"Hard\"\n").unwrap();
        assert_eq!(locale.attribute(Path::new(""), &data, "name", ""), "堕落 / 困难");
    }

    #[test]
    fn gettext_uses_the_catalogs_plural_rule_and_all_translated_forms() {
        let singular = "(this turn left)";
        let plural = "($remaining_turns turns left)";
        let key = format!("{singular}\0{plural}");
        let header = "Plural-Forms: nplurals=3; plural=n%10==1 && n%100!=11 ? 0 : n%10>=2 && n%10<=4 && (n%100<12 || n%100>14) ? 1 : 2;\n";
        let forms = "form one\0form few\0form many";
        // Two MO rows: the header and the pair of native message IDs.
        let mut bytes = Vec::new();
        let mut strings = Vec::new();
        for word in [0x950412de_u32, 0, 2, 28, 44, 0, 0] {
            bytes.extend(word.to_le_bytes());
        }
        for value in ["", key.as_str(), header, forms] {
            bytes.extend((value.len() as u32).to_le_bytes());
            bytes.extend((60 + strings.len() as u32).to_le_bytes());
            strings.extend(value.bytes());
            strings.push(0);
        }
        bytes.extend(strings);
        let locale = Locale { game: Catalog::read(&bytes).unwrap(), ..Default::default() };
        assert_eq!(locale.game_plural(singular, plural, 1), "form one");
        assert_eq!(locale.game_plural(singular, plural, 21), "form one");
        assert_eq!(locale.game_plural(singular, plural, 22), "form few");
        assert_eq!(locale.game_plural(singular, plural, 12), "form many");
        assert_eq!(locale.game_plural("missing singular", "missing plural", 2), "missing plural");
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
