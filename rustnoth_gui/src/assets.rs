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
    fallback: PathBuf,
    units: BTreeMap<String, UnitAssets>,
    terrain: BTreeMap<String, Option<PathBuf>>,
}

#[derive(Debug)]
struct UnitAssets {
    image: Option<PathBuf>,
    portrait: Option<PathBuf>,
    dialogue_portrait: Option<PathBuf>,
    name: Option<String>,
    weapon_names: Vec<Option<String>>,
}

impl AssetCatalog {
    /// Reads core definitions before custom scenario definitions.
    pub fn parse(wesnoth_root: &Path, core: &[u8], bundle: &[u8]) -> Result<Self, Box<dyn std::error::Error>> {
        let mut prefixes = BTreeSet::from([String::new(), "data/core".into()]);
        let mut catalog = Self::default();
        let mut units = Node::default();
        let mut document = Node::default();
        for input in [core, bundle] {
            for (tag, node) in wml::parse(input)?.children {
                match tag.as_str() {
                    "units" => units.children.extend(node.children),
                    "terrain_type" => document.children.push((tag, node)),
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
        engine_lite::unit_type::resolve_base_units(&mut units)?;
        document.children.push(("units".into(), units));
        catalog.image_dirs = prefixes.iter().map(|prefix| wesnoth_root.join(prefix).join("images")).collect();
        catalog.image_dirs.push(wesnoth_root.to_owned());
        catalog.fallback = wesnoth_root.join("data/core/images");
        catalog.read_definitions(&document);
        Ok(catalog)
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
        self.resolve_image(self.units.get(id)?.dialogue_portrait.as_deref()?)
    }

    /// Returns the unit type's display name from its WML definition.
    pub fn unit_name(&self, id: &str) -> Option<&str> {
        self.units.get(id)?.name.as_deref()
    }

    /// Returns the display name at the referee's zero-based weapon index.
    pub fn weapon_name(&self, id: &str, index: usize) -> Option<&str> {
        self.units.get(id)?.weapon_names.get(index)?.as_deref()
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
                    self.terrain.entry(code.to_owned()).or_insert_with(|| {
                        terrain_image(node).map(|image| Path::new("terrain").join(image))
                    });
                }
                "units" => {
                    for (_, unit) in node.children.iter().filter(|(tag, _)| tag == "unit_type") {
                        let Some(id) = unit.attribute("id").filter(|id| !id.is_empty()) else {
                            continue;
                        };
                        self.units.entry(id.to_owned()).or_insert_with(|| UnitAssets {
                            image: unit
                                .attribute("image")
                                .and_then(image_filename)
                                .map(PathBuf::from),
                            dialogue_portrait: display_name(unit, "profile")
                                .filter(|value| *value != "unit_image").and_then(image_filename).map(PathBuf::from),
                            portrait: display_name(unit, "small_profile")
                                .or_else(|| display_name(unit, "profile"))
                                .filter(|value| *value != "unit_image")
                                .and_then(image_filename)
                                .map(PathBuf::from),
                            name: display_name(unit, "name").map(str::to_owned),
                            weapon_names: unit
                                .children
                                .iter()
                                .filter(|(tag, _)| tag == "attack")
                                .map(|(_, weapon)| {
                                    display_name(weapon, "description")
                                        .or_else(|| display_name(weapon, "name"))
                                        .map(str::to_owned)
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
    editor_image=grass/green
    symbol_image=grass/green-symbol
[/terrain_type]
[terrain_type]
    string=^Vh
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
    [/unit_type]
[/units]
"#;
        let catalog = AssetCatalog::parse(root, core, bundle).unwrap();

        assert_eq!(
            catalog.unit_image("Bowman"),
            Some(images.join("units/human-loyalists/bowman.png")),
        );
        assert_eq!(
            catalog.unit_image("Scout"),
            Some(images.join("units/custom/scout.png")),
        );
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
