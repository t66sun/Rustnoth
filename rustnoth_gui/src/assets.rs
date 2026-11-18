//! Resolve desktop interface art and labels from Wesnoth's preprocessed WML.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use engine_lite::wml::{self, Node};

/// Original presentation data and local image paths indexed by WML identifier.
#[derive(Debug, Default)]
pub struct AssetCatalog {
    image_dirs: Vec<PathBuf>,
    terrain_rules: Vec<Node>,
    fallback: PathBuf,
    units: BTreeMap<String, UnitAssets>,
    terrain: BTreeMap<String, Option<PathBuf>>,
    terrain_names: BTreeMap<String, String>,
    minimap_symbols: BTreeMap<String, PathBuf>,
    villages: BTreeSet<String>,
}

#[derive(Debug)]
struct UnitAssets {
    image: Option<PathBuf>,
    portrait: Option<PathBuf>,
    dialogue_profile: Option<String>,
    name: Option<String>,
    race_name: Option<String>,
    abilities: Vec<String>,
    weapons: Vec<(Option<String>, Option<PathBuf>)>,
    ellipse: String,
    bars: UnitBars,
    standing: Vec<Node>,
}

#[derive(Clone, Copy, Debug)]
pub struct UnitBars {
    pub offset: Option<[i32; 2]>,
    pub hp_scale: f32,
    pub xp_scale: f32,
    pub level: i32,
    /// Some(true) for a major advancement, Some(false) for AMLA only.
    pub advancement: Option<bool>,
}

impl UnitBars {
    fn from_unit(unit: &Node) -> Self {
        let x = unit.attribute("bar_offset_x");
        let y = unit.attribute("bar_offset_y");
        let advances_to = unit.attribute("advances_to")
            .is_some_and(|ids| ids.split(',').map(str::trim).any(|id| !id.is_empty() && id != "null"));
        let mut advancement = advances_to.then_some(true);
        for (_, advance) in unit.children.iter().filter(|(tag, _)| tag == "advancement") {
            let major = matches!(advance.attribute("major_amla"), Some("yes" | "true" | "1" | "on"));
            advancement = Some(advancement.unwrap_or(false) || major);
        }
        Self {
            offset: (x.is_some() || y.is_some()).then(|| [x, y].map(|v| v.and_then(|s| s.parse().ok()).unwrap_or(0))),
            hp_scale: unit.attribute("hp_bar_scaling").and_then(|s| s.parse().ok()).unwrap_or(0.6),
            xp_scale: unit.attribute("xp_bar_scaling").and_then(|s| s.parse().ok()).unwrap_or(0.5),
            level: unit.attribute("level").and_then(|s| s.parse().ok()).unwrap_or(0),
            advancement,
        }
    }
}

impl AssetCatalog {
    /// Reads core definitions before custom scenario definitions.
    pub fn parse(wesnoth_root: &Path, core: &[u8], bundle: &[u8]) -> Result<Self, Box<dyn std::error::Error>> {
        Self::from_config(wesnoth_root, core, bundle, true)
    }

    /// Load presentation metadata for the map and leader previews in multiplayer setup.
    pub fn for_setup(root: &Path, core: &[u8]) -> Result<Self, Box<dyn std::error::Error>> {
        Self::from_config(root, core, &[], false)
    }

    fn from_config(wesnoth_root: &Path, core: &[u8], bundle: &[u8], terrain_graphics: bool) -> Result<Self, Box<dyn std::error::Error>> {
        let mut prefixes = BTreeSet::from([String::new(), "data/core".into()]);
        let mut catalog = Self::default();
        let mut units = Node::default();
        let mut document = Node::default();
        let mut core_graphics = Vec::new();
        for (is_core, input) in [(true, core), (false, bundle)] {
            for (tag, node) in wml::parse(input)?.children {
                match tag.as_str() {
                    "units" => units.children.extend(node.children),
                    "terrain_type" => document.children.push((tag, node)),
                    "terrain_graphics" if !terrain_graphics => {},
                    "terrain_graphics" if is_core => core_graphics.push(node),
                    "terrain_graphics" => catalog.terrain_rules.push(node),
                    "binary_path" => {
                        if let Some(path) = node.attribute("path").map(|s| s.trim_end_matches('/'))
                            && !path.contains("..") && safe_relative_path(Path::new(path))
                        {
                            prefixes.insert(path.to_owned());
                        }
                    }
                    _ => {}
                }
            }
        }
        if terrain_graphics && core_graphics.is_empty() && wesnoth_root.join("data/core/terrain-graphics.cfg").is_file() {
            core_graphics = crate::terrain_wml::stock_rules(wesnoth_root)?;
        }
        catalog.terrain_rules.extend(core_graphics);
        engine_lite::unit_type::resolve_base_units(&mut units)?;
        document.children.push(("units".into(), units));
        catalog.image_dirs = prefixes.iter().map(|prefix| wesnoth_root.join(prefix).join("images")).collect();
        catalog.image_dirs.push(wesnoth_root.to_owned());
        catalog.fallback = wesnoth_root.join("data/core/images");
        catalog.read_definitions(&document);
        Ok(catalog)
    }

    pub fn standing_animations(&self, id: &str) -> &[Node] {
        self.units.get(id).map_or(&[], |unit| unit.standing.as_slice())
    }

    pub fn unit_race_name(&self, id: &str) -> Option<&str> {
        self.units.get(id)?.race_name.as_deref()
    }

    pub fn unit_ability_names(&self, id: &str) -> &[String] {
        self.units.get(id).map_or(&[], |u| u.abilities.as_slice())
    }

    pub fn unit_bars(&self, id: &str) -> Option<UnitBars> {
        self.units.get(id).map(|unit| unit.bars)
    }

    pub fn terrain_rules(&self) -> &[Node] { &self.terrain_rules }

    pub fn image_roots(&self) -> &[PathBuf] { &self.image_dirs }

    pub fn ellipse_prefix(&self, id: &str) -> Option<&str> {
        let prefix = self.units.get(id)?.ellipse.as_str();
        (prefix != "none" && safe_relative_path(Path::new(prefix))).then_some(prefix)
    }

    /// Returns the original sprite path of a unit type.
    pub fn unit_image(&self, id: &str) -> Option<PathBuf> {
        self.resolve_image(self.units.get(id)?.image.as_deref()?)
    }

    /// Original sidebar portrait. A missing profile or `unit_image` uses the team's board sprite.
    pub fn unit_portrait(&self, id: &str) -> Option<PathBuf> {
        self.resolve_image(self.units.get(id)?.portrait.as_deref()?)
    }

    /// Full-size portrait used when a campaign speaker has no explicit profile.
    pub fn dialogue_portrait(&self, id: &str) -> Option<PathBuf> {
        let image = self.dialogue_profile(id).filter(|value| *value != "unit_image")?;
        self.resolve_image(Path::new(image_filename(image)?))
    }

    /// The original WML expression, including the profile's default orientation.
    pub fn dialogue_profile(&self, id: &str) -> Option<&str> {
        self.units.get(id)?.dialogue_profile.as_deref()
    }

    /// Returns the unit type's display name from its WML definition.
    pub fn unit_name(&self, id: &str) -> Option<&str> {
        self.units.get(id)?.name.as_deref()
    }

    /// Returns the display name at the referee's zero-based weapon index.
    pub fn weapon_name(&self, id: &str, index: usize) -> Option<&str> {
        self.units.get(id)?.weapons.get(index)?.0.as_deref()
    }

    pub fn weapon_icon(&self, id: &str, index: usize) -> Option<PathBuf> {
        self.resolve_image(self.units.get(id)?.weapons.get(index)?.1.as_deref()?)
    }

    /// Name of the highlighted tile as displayed in the terrain report.
    pub fn terrain_name(&self, code: &str) -> Option<&str> {
        self.terrain_names.get(code).or_else(|| {
            let (base, overlay) = code.split_at(code.find('^')?);
            self.terrain_names.get(overlay).or_else(|| self.terrain_names.get(base))
        }).map(String::as_str)
    }

    /// Returns an exact terrain image or the known base and overlay in draw order.
    pub fn terrain_images(&self, code: &str) -> Vec<PathBuf> {
        if let Some(Some(image)) = self.terrain.get(code) {
            return self.resolve_image(image).into_iter().collect();
        }
        let Some(caret) = code.find('^') else {
            return Vec::new();
        };
        let (base, overlay) = code.split_at(caret);
        [base, overlay]
            .into_iter()
            .filter_map(|layer| self.resolve_image(self.terrain.get(layer)?.as_deref()?))
            .collect()
    }

    /// Native terrain symbols (distinct from editor previews) and village classification.
    pub fn minimap_terrain(&self, code: &str) -> (Vec<PathBuf>, bool) {
        let layers = if self.terrain.contains_key(code) {
            vec![code]
        } else if let Some(caret) = code.find('^') {
            vec![&code[..caret], &code[caret..]]
        } else {
            vec![code]
        };
        let village = layers.iter().any(|layer| self.villages.contains(*layer));
        let paths = layers.iter().map_while(|layer| self.resolve_image(self.minimap_symbols.get(*layer)?)).collect();
        (paths, village)
    }

    pub fn resolve_image(&self, image: &Path) -> Option<PathBuf> {
        // Native terrain image lookup treats "../" as a reset to the image directory.
        let text = image.to_str()?;
        let path = Path::new(text.rsplit("../").next()?);
        if path.as_os_str().is_empty() || !safe_relative_path(path) { return None; }
        Some(self.image_dirs.iter().map(|dir| dir.join(path)).find(|p| p.is_file())
            .unwrap_or_else(|| self.fallback.join(path)))
    }

    fn read_definitions(&mut self, document: &Node) {
        for (tag, node) in &document.children {
            match tag.as_str() {
                "terrain_type" => {
                    let Some(code) = node.attribute("string").filter(|code| !code.is_empty()) else {
                        continue;
                    };
                    if let Some(symbol) = node.attribute("symbol_image").and_then(image_filename) {
                        let mut path = Path::new("terrain").join(symbol);
                        if path.extension().is_none() { path.set_extension("png"); }
                        self.minimap_symbols.entry(code.to_owned()).or_insert(path);
                    }
                    if matches!(node.attribute("gives_income"), Some("yes" | "true" | "on" | "1")) {
                        self.villages.insert(code.to_owned());
                    }
                    if let Some(name) = display_name(node, "name") {
                        self.terrain_names.entry(code.to_owned()).or_insert_with(|| name.to_owned());
                    }
                    self.terrain.entry(code.to_owned()).or_insert_with(|| {
                        terrain_image(node).map(|image| Path::new("terrain").join(image))
                    });
                }
                "units" => {
                    let races: BTreeMap<_, _> = node.children.iter().filter(|(tag, _)| tag == "race")
                        .filter_map(|(_, race)| {
                            let label = |name| display_name(race, name).map(|s| s.rsplit('^').next().unwrap_or(s));
                            let shared_name = label("name").or_else(|| {
                                let male = label("male_name")?;
                                label("female_name").is_none_or(|female| female == male).then_some(male)
                            })?;
                            Some((race.attribute("id")?, shared_name))
                        }).collect();
                    for (_, unit) in node.children.iter().filter(|(tag, _)| tag == "unit_type") {
                        let Some(id) = unit.attribute("id").filter(|id| !id.is_empty()) else {
                            continue;
                        };
                        self.units.entry(id.to_owned()).or_insert_with(|| UnitAssets {
                            standing: unit.children.iter().filter(|(tag, _)| tag == "standing_anim")
                                .map(|(_, definition)| definition.clone()).collect(),
                            image: unit
                                .attribute("image")
                                .and_then(image_filename)
                                .map(PathBuf::from),
                            dialogue_profile: display_name(unit, "profile").map(str::to_owned),
                            portrait: display_name(unit, "small_profile")
                                .or_else(|| display_name(unit, "profile"))
                                .filter(|value| *value != "unit_image")
                                .and_then(image_filename)
                                .map(PathBuf::from),
                            name: display_name(unit, "name").map(str::to_owned),
                            race_name: unit.attribute("race").and_then(|race| races.get(race)).map(|s| (*s).to_owned()),
                            abilities: unit.children.iter().filter(|(tag, _)| tag == "abilities")
                                .flat_map(|(_, list)| &list.children)
                                .filter_map(|(_, ability)| display_name(ability, "name"))
                                .map(|s| s.rsplit('^').next().unwrap_or(s).to_owned()).collect(),
                            bars: UnitBars::from_unit(unit),
                            ellipse: unit.attribute("ellipse").filter(|s| !s.is_empty()).unwrap_or("misc/ellipse").into(),
                            weapons: unit
                                .children
                                .iter()
                                .filter(|(tag, _)| tag == "attack")
                                .map(|(_, weapon)| {
                                    let name = display_name(weapon, "description").or_else(|| display_name(weapon, "name"));
                                    let image = display_name(weapon, "icon").and_then(image_filename).map(PathBuf::from)
                                        .or_else(|| Some(PathBuf::from(format!("attacks/{}.png", display_name(weapon, "name")?))));
                                    (name.map(str::to_owned), image)
                                })
                                .collect(),
                        });
                    }
                }
                _ => {}
            }
        }
    }
}

fn safe_relative_path(path: &Path) -> bool {
    path.components().all(|part| matches!(part, std::path::Component::Normal(_) | std::path::Component::CurDir))
}

fn display_name<'a>(node: &'a Node, attribute: &str) -> Option<&'a str> {
    node.attribute(attribute).filter(|value| !value.trim().is_empty())
}

fn terrain_image(node: &Node) -> Option<PathBuf> {
    let filename = node
        .attribute("editor_image")
        .and_then(image_filename)
        .or_else(|| node.attribute("symbol_image").and_then(image_filename))?;
    let mut path = PathBuf::from(filename);
    if path.extension().is_none() {
        path.set_extension("png");
    }
    Some(path)
}

fn image_filename(value: &str) -> Option<&str> {
    let filename = value.split_once('~').map_or(value, |(path, _)| path).trim();
    (!filename.is_empty()).then_some(filename)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimap_uses_symbol_art_and_identifies_neutral_village_overlays() {
        let core = b"[terrain_type]\nstring=Gg\nsymbol_image=grass/green-symbol\neditor_image=grass/green\n[/terrain_type]\n[terrain_type]\nstring=^Vh\nsymbol_image=village/human\ngives_income=yes\n[/terrain_type]";
        let root = Path::new("wesnoth");
        let catalog = AssetCatalog::parse(root, core, b"").unwrap();
        let terrain = root.join("data/core/images/terrain");
        assert_eq!(catalog.minimap_terrain("Gg^Vh"),
            (vec![terrain.join("grass/green-symbol.png"), terrain.join("village/human.png")], true));
        assert_eq!(catalog.minimap_terrain("Gg"), (vec![terrain.join("grass/green-symbol.png")], false));
    }

    #[test]
    fn original_campaign_art_is_resolved_from_the_declared_binary_path() {
        use std::{fs, time::{SystemTime, UNIX_EPOCH}};
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("rustnoth-campaign-art-{}-{nonce}", std::process::id()));
        let sprite = root.join("data/campaigns/Guard/images/units/veteran.png");
        fs::create_dir_all(sprite.parent().unwrap()).unwrap();
        fs::write(&sprite, []).unwrap();
        let bundle = b"[binary_path]\npath=data/campaigns/Guard\n[/binary_path]\n[units]\n[unit_type]\nid=Veteran\nimage=units/veteran.png\n[/unit_type]\n[/units]";
        let catalog = AssetCatalog::parse(&root, b"", bundle).unwrap();
        assert_eq!(catalog.unit_image("Veteran"), Some(sprite));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn campaign_units_can_use_a_core_base_units_sprite_and_weapon_names() {
        let core = b"[units]\n[unit_type]\nid=Bowman\nimage=units/human/bow.png\n[attack]\ndescription=longbow\n[/attack]\n[/unit_type]\n[/units]";
        let scenario = b"[units]\n[unit_type]\nid=Hero\n[base_unit]\nid=Bowman\n[/base_unit]\n[/unit_type]\n[/units]";
        let catalog = AssetCatalog::parse(Path::new("wesnoth"), core, scenario).unwrap();
        assert_eq!(catalog.unit_image("Hero"), Some(Path::new("wesnoth/data/core/images/units/human/bow.png").into()));
        assert_eq!(catalog.weapon_name("Hero", 0), Some("longbow"));
    }

    #[test]
    fn sidebar_prefers_small_profile_then_profile_and_honors_unit_image() {
        let units = br#"[units]
[unit_type]
id=Guard
small_profile=portraits/guard-small.png
profile=portraits/guard.webp
[/unit_type]
[unit_type]
id=Knight
profile=portraits/knight.webp
[/unit_type]
[unit_type]
id=Scout
small_profile=unit_image
profile=portraits/scout.webp
[/unit_type]
[/units]"#;
        let dir = Path::new("wesnoth");
        let catalog = AssetCatalog::parse(dir, units, b"").unwrap();
        let portraits = dir.join("data/core/images/portraits");
        assert_eq!(catalog.unit_portrait("Guard"), Some(portraits.join("guard-small.png")));
        assert_eq!(catalog.unit_portrait("Knight"), Some(portraits.join("knight.webp")));
        assert_eq!(catalog.unit_portrait("Scout"), None);
    }

    #[test]
    fn preserves_original_unit_names_and_weapons_in_referee_order() {
        let core = br#"
[units]
    [unit_type]
        id=Bowman
        name= _ "Bowman"
        [attack]
            name=bow
            description= _ "longbow"
        [/attack]
        [attack]
            name="short sword"
            description=""
        [/attack]
        [attack]
        [/attack]
        [attack]
            name=dagger
        [/attack]
    [/unit_type]
[/units]
"#;
        let bundle = br#"
[units]
    [unit_type]
        id=Bowman
        name="Custom Bowman"
        [attack]
            name=spear
        [/attack]
    [/unit_type]
    [unit_type]
        id=Scout
        name="Dune Scout"
    [/unit_type]
[/units]
"#;
        let catalog = AssetCatalog::parse(Path::new("wesnoth_src"), core, bundle).unwrap();

        assert_eq!(catalog.unit_name("Bowman"), Some("Bowman"));
        assert_eq!(catalog.unit_name("Scout"), Some("Dune Scout"));
        assert_eq!(catalog.unit_name("Missing"), None);
        assert_eq!(catalog.weapon_name("Bowman", 0), Some("longbow"));
        assert_eq!(catalog.weapon_name("Bowman", 1), Some("short sword"));
        assert_eq!(catalog.weapon_name("Bowman", 2), None);
        assert_eq!(catalog.weapon_name("Bowman", 3), Some("dagger"));
        assert_eq!(catalog.weapon_name("Bowman", 4), None);
    }

    #[test]
    fn uses_core_sprites_and_draws_exact_or_layered_terrain_from_the_source_tree() {
        let root = Path::new("wesnoth_src");
        let images = root.join("data/core/images");
        let core = br#"
[terrain_type]
    string=Gg
    name=Grassland
    editor_image=grass/green
    symbol_image=grass/green-symbol
[/terrain_type]
[terrain_type]
    string=^Vh
    name=Village
    symbol_image="village/human-tile~O(0.5)"
[/terrain_type]
[terrain_type]
    string=Ch
    symbol_image=castle/castle-tile
[/terrain_type]
[units]
    [unit_type]
        id=Bowman
        image="units/human-loyalists/bowman.png~RC(magenta>red)"
    [/unit_type]
[/units]
"#;
        let bundle = br#"
[terrain_type]
    string=Gg
    editor_image=grass/custom
[/terrain_type]
[terrain_type]
    string=Ch^Vh
    editor_image=castle/village.png
[/terrain_type]
[units]
    [unit_type]
        id=Bowman
        image=units/custom/bowman.png
    [/unit_type]
    [unit_type]
        id=Scout
        image=units/custom/scout.png
        bar_offset_x=3
        hp_bar_scaling=0.8
        level=2
        [advancement]
            id=amla
        [/advancement]
    [/unit_type]
[/units]
"#;
        let catalog = AssetCatalog::parse(root, core, bundle).unwrap();

        assert_eq!(catalog.terrain_name("Gg"), Some("Grassland"));
        assert_eq!(catalog.terrain_name("Gg^Vh"), Some("Village"));
        assert_eq!(
            catalog.unit_image("Bowman"),
            Some(images.join("units/human-loyalists/bowman.png")),
        );
        assert_eq!(
            catalog.unit_image("Scout"),
            Some(images.join("units/custom/scout.png")),
        );
        let bars = catalog.unit_bars("Scout").unwrap();
        assert_eq!(bars.offset, Some([3, 0]));
        assert_eq!((bars.level, bars.hp_scale, bars.xp_scale), (2, 0.8, 0.5));
        assert_eq!(bars.advancement, Some(false));
        assert_eq!(catalog.unit_image("Missing"), None);
        assert_eq!(
            catalog.terrain_images("Gg^Vh"),
            [
                images.join("terrain/grass/green.png"),
                images.join("terrain/village/human-tile.png"),
            ],
        );
        assert_eq!(
            catalog.terrain_images("Ch^Vh"),
            [images.join("terrain/castle/village.png")],
        );
        assert_eq!(
            catalog.terrain_images("Ch^Missing"),
            [images.join("terrain/castle/castle-tile.png")],
        );
    }
}
