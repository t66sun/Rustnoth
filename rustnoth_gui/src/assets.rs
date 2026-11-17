//! Resolve desktop interface art from Wesnoth's preprocessed WML.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use engine_lite::wml::{self, Node};

/// Image paths under a local Wesnoth source tree, indexed by WML identifier.
#[derive(Debug, Default)]
pub struct AssetCatalog {
    units: BTreeMap<String, Option<PathBuf>>,
    terrain: BTreeMap<String, Option<PathBuf>>,
}

impl AssetCatalog {
    /// Reads core definitions before custom scenario definitions.
    pub fn parse(wesnoth_root: &Path, core: &[u8], bundle: &[u8]) -> Result<Self, wml::ParseError> {
        let images = wesnoth_root.join("data/core/images");
        let mut catalog = Self::default();
        for input in [core, bundle] {
            catalog.read_definitions(&wml::parse(input)?, &images);
        }
        Ok(catalog)
    }

    /// Returns the original sprite path of a unit type.
    pub fn unit_image(&self, id: &str) -> Option<PathBuf> {
        self.units.get(id)?.clone()
    }

    /// Returns an exact terrain image or the known base and overlay in draw order.
    pub fn terrain_images(&self, code: &str) -> Vec<PathBuf> {
        if let Some(Some(image)) = self.terrain.get(code) {
            return vec![image.clone()];
        }
        let Some(caret) = code.find('^') else {
            return Vec::new();
        };
        let (base, overlay) = code.split_at(caret);
        [base, overlay]
            .into_iter()
            .filter_map(|layer| self.terrain.get(layer).and_then(Option::as_ref).cloned())
            .collect()
    }

    fn read_definitions(&mut self, document: &Node, images: &Path) {
        for (tag, node) in &document.children {
            match tag.as_str() {
                "terrain_type" => {
                    let Some(code) = node.attribute("string").filter(|code| !code.is_empty()) else {
                        continue;
                    };
                    self.terrain.entry(code.to_owned()).or_insert_with(|| {
                        terrain_image(node).map(|image| images.join("terrain").join(image))
                    });
                }
                "units" => {
                    for (_, unit) in node.children.iter().filter(|(tag, _)| tag == "unit_type") {
                        let Some(id) = unit.attribute("id").filter(|id| !id.is_empty()) else {
                            continue;
                        };
                        self.units.entry(id.to_owned()).or_insert_with(|| {
                            unit.attribute("image")
                                .and_then(image_filename)
                                .map(|image| images.join(image))
                        });
                    }
                }
                _ => {}
            }
        }
    }
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
