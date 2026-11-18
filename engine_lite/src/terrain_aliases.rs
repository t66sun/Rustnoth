//! Resolve terrain aliases and scenario rule features.

use std::collections::HashMap;

use crate::{scenario_time::integer, wml::Node};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AliasKind {
    Movement,
    Defense,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AliasTerm<'a> {
    Best,
    Worst,
    Terrain(&'a str),
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum TerrainLayer { Both, Base, Overlay }

/// Game rules attached to a terrain code.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TerrainFeatures {
    pub village: bool,
    pub castle: bool,
    pub keep: bool,
    pub healing: i32,
}

impl TerrainFeatures {
    fn from_wml(terrain: &Node) -> Self {
        let value = |key| {
            terrain
                .attributes
                .get(key)
                .filter(|value| value.translations.is_empty())
                .map(|value| value.text.as_str())
        };
        let enabled = |key| matches!(value(key), Some("yes" | "true"));
        Self {
            village: enabled("gives_income"),
            castle: enabled("recruit_onto"),
            keep: enabled("recruit_from"),
            healing: value("heals")
                .and_then(|text| text.trim_ascii().parse().ok())
                .unwrap_or_default(),
        }
    }
}

#[derive(Debug)]
pub struct TerrainAliases {
    definitions: HashMap<String, Definition>,
}

#[derive(Debug)]
struct Definition {
    id: String,
    movement: Vec<String>,
    defense: Vec<String>,
    features: TerrainFeatures,
    light: TerrainLight,
    default_base: Option<String>,
}

#[derive(Clone, Copy, Default, Debug)]
struct TerrainLight {
    change: i32,
    minimum: i32,
    maximum: i32,
}

impl TerrainLight {
    fn from_wml(terrain: &Node) -> Self {
        let change = integer(terrain, "light").unwrap_or(0);
        Self {
            change,
            minimum: integer(terrain, "min_light").unwrap_or(change),
            maximum: integer(terrain, "max_light").unwrap_or(change),
        }
    }

    fn apply(self, base: i32) -> i32 {
        let adjusted = base.saturating_add(self.change);
        if self.change >= 0 {
            adjusted.min(base.max(self.maximum))
        } else {
            adjusted.max(base.min(self.minimum))
        }
    }
}

impl Definition {
    fn terms(&self, kind: AliasKind) -> &[String] {
        match kind {
            AliasKind::Movement => &self.movement,
            AliasKind::Defense => &self.defense,
        }
    }
}

impl TerrainAliases {
    /// Load the `[terrain_type]` children of a preprocessed configuration.
    /// The first definition of each terrain code wins.
    pub fn from_wml(config: &Node) -> Self {
        let mut definitions = HashMap::new();
        for (tag, terrain) in &config.children {
            if tag != "terrain_type" {
                continue;
            }

            let code = canonical_code(terrain.attribute("string").unwrap_or_default());
            definitions.entry(code.to_owned()).or_insert_with(|| {
                let shared = terrain.attribute("aliasof");
                Definition {
                    id: terrain.attribute("id").unwrap_or_default().to_owned(),
                    movement: read_alias(terrain.attribute("mvt_alias"), shared, code),
                    defense: read_alias(terrain.attribute("def_alias"), shared, code),
                    features: TerrainFeatures::from_wml(terrain),
                    light: TerrainLight::from_wml(terrain),
                    default_base: terrain.attribute("default_base").map(str::to_owned),
                }
            });
        }
        Self { definitions }
    }

    /// Return the WML id of a directly defined code.
    pub fn terrain_id(&self, code: &str) -> Option<&str> {
        self.definitions
            .get(canonical_code(code))
            .map(|definition| definition.id.as_str())
    }

    /// Returns the features of an exact terrain code or a known base and overlay.
    /// Undefined terrain has no special features and heals zero hitpoints.
    pub fn features(&self, code: &str) -> TerrainFeatures {
        let code = canonical_code(code);
        if let Some(definition) = self.definitions.get(code) {
            return definition.features;
        }
        if let Some((base, overlay)) = self.layers(code) {
            return TerrainFeatures {
                village: base.features.village || overlay.features.village,
                castle: base.features.castle || overlay.features.castle,
                keep: base.features.keep || overlay.features.keep,
                healing: base.features.healing.max(overlay.features.healing),
            };
        }
        TerrainFeatures::default()
    }

    /// Terrain-adjusted lawful bonus, before illumination from nearby units.
    pub fn light_bonus(&self, code: &str, base_bonus: i32) -> i32 {
        let code = canonical_code(code);
        if let Some(definition) = self.definitions.get(code) {
            return definition.light.apply(base_bonus);
        }
        if let Some((base, overlay)) = self.layers(code) {
            return TerrainLight {
                change: base.light.change.saturating_add(overlay.light.change),
                minimum: base.light.minimum.min(overlay.light.minimum),
                maximum: base.light.maximum.max(overlay.light.maximum),
            }.apply(base_bonus);
        }
        base_bonus
    }

    /// Resolve one layer of aliasing. Callers can evaluate terrain references
    /// recursively with their own movement or defense values.
    pub fn terms<'a>(&'a self, code: &'a str, kind: AliasKind) -> Vec<AliasTerm<'a>> {
        let code = canonical_code(code);
        if let Some(definition) = self.definitions.get(code) {
            return alias_terms(definition.terms(kind)).collect();
        }

        if let Some((base, overlay)) = self.layers(code) {
            return merge_aliases(base.terms(kind), overlay.terms(kind));
        }

        vec![AliasTerm::Terrain(code)]
    }

    pub(crate) fn merge_terrain(&self, old: &str, new: &str, layer: TerrainLayer, fallback: bool) -> Option<String> {
        let split = |code: &str| code.find('^').unwrap_or(code.len());
        let (old_base, old_overlay) = old.split_at(split(old));
        let (new_base, new_overlay) = new.split_at(split(new));
        let merged = match layer {
            TerrainLayer::Both => new.to_owned(),
            TerrainLayer::Base => format!("{new_base}{old_overlay}"),
            TerrainLayer::Overlay => format!("{old_base}{new_overlay}"),
        };
        let known = |code: &str| self.definitions.contains_key(code) || self.layers(code).is_some();
        if !merged.is_empty() && !merged.starts_with('^') && known(&merged) {
            return Some(merged);
        }
        if !fallback || !known(new) { return None; }
        if !new_base.is_empty() { return Some(new.to_owned()); }
        let base = self.definitions.get(new)?.default_base.as_deref()?;
        Some(format!("{base}{new_overlay}"))
    }

    fn layers(&self, code: &str) -> Option<(&Definition, &Definition)> {
        let (base, overlay) = code.split_at(code.find('^')?);
        Some((self.definitions.get(base)?, self.definitions.get(overlay)?))
    }
}

fn canonical_code(text: &str) -> &str {
    let code = text.trim().rsplit(' ').next().unwrap_or_default();
    code.strip_suffix('^').unwrap_or(code)
}

fn read_alias(specific: Option<&str>, shared: Option<&str>, code: &str) -> Vec<String> {
    match specific
        .filter(|alias| !alias.is_empty())
        .or(shared.filter(|alias| !alias.is_empty()))
    {
        Some(alias) => alias
            .split_terminator(',')
            .map(|token| canonical_code(token).to_owned())
            .collect(),
        None => vec![code.to_owned()],
    }
}

fn alias_terms(codes: &[String]) -> impl DoubleEndedIterator<Item = AliasTerm<'_>> {
    codes.iter().map(|code| match code.as_str() {
        "+" => AliasTerm::Best,
        "-" => AliasTerm::Worst,
        code => AliasTerm::Terrain(code),
    })
}

fn merge_aliases<'a>(base: &'a [String], overlay: &'a [String]) -> Vec<AliasTerm<'a>> {
    let Some(base_ref) = overlay.iter().position(|code| code == "_bas") else {
        return alias_terms(overlay).collect();
    };

    let mode = alias_terms(&overlay[..base_ref])
        .rev()
        .find(|term| matches!(term, AliasTerm::Best | AliasTerm::Worst))
        .unwrap_or(AliasTerm::Best);
    alias_terms(&overlay[..base_ref])
        .chain(alias_terms(base))
        .chain([mode])
        .chain(alias_terms(&overlay[base_ref + 1..]))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::AliasKind::{Defense, Movement};
    use super::AliasTerm::{Best, Terrain, Worst};
    use super::{TerrainAliases, TerrainFeatures};
    use crate::wml;

    fn aliases(config: &str) -> TerrainAliases {
        TerrainAliases::from_wml(&wml::parse(config).unwrap())
    }

    #[test]
    fn both_light_layers_contribute_with_directional_bounds() {
        let catalog = aliases(concat!(
            "[terrain_type]\nstring=Ql\nlight=25\nmax_light=35\n[/terrain_type]\n",
            "[terrain_type]\nstring=^Ii\nlight=25\n[/terrain_type]\n",
            "[terrain_type]\nstring=^Dm\nlight=-40\nmin_light=-50\n[/terrain_type]\n",
            "[terrain_type]\nstring=Ql^Ex\nlight=-10\n[/terrain_type]",
        ));
        assert_eq!(catalog.light_bonus("Ql^Ii", -40), 10);
        assert_eq!(catalog.light_bonus("Ql^Ii", 25), 35);
        assert_eq!(catalog.light_bonus("Ql^Ii", 50), 50);
        assert_eq!(catalog.light_bonus("Ql^Dm", -45), -50);
        assert_eq!(catalog.light_bonus("Ql^Dm", -60), -60);
        assert_eq!(catalog.light_bonus("Ql^Ex", 0), -10);
    }

    #[test]
    fn specific_aliases_override_aliasof_and_duplicates_keep_first_definition() {
        let catalog = aliases(
            "[terrain_type]\nstring=Wwf\nid=ford\naliasof=Gt, Wst\nmvt_alias=\"-, Wst, +, Gt\"\n[/terrain_type]\n\
             [terrain_type]\nstring=Wwf\nid=duplicate\ndef_alias=Ht\n[/terrain_type]",
        );
        assert_eq!(catalog.terrain_id("Wwf"), Some("ford"));
        assert_eq!(
            catalog.terms("Wwf", Movement),
            [Worst, Terrain("Wst"), Best, Terrain("Gt")]
        );
        assert_eq!(catalog.terms("Wwf", Defense), [Terrain("Gt"), Terrain("Wst")]);
    }

    #[test]
    fn layered_alias_inserts_base_once_and_restores_previous_mode() {
        let catalog = aliases(
            "[terrain_type]\nstring=Gg\naliasof=\"Gt,-,Wst,+,Ht\"\n[/terrain_type]\n\
             [terrain_type]\nstring=^Fp\nmvt_alias=\"-,_bas,Ft,+,_bas\"\ndef_alias=_bas,Ft\n[/terrain_type]",
        );
        assert_eq!(
            catalog.terms("Gg^Fp", Movement),
            [
                Worst,
                Terrain("Gt"),
                Worst,
                Terrain("Wst"),
                Best,
                Terrain("Ht"),
                Worst,
                Terrain("Ft"),
                Best,
                Terrain("_bas"),
            ]
        );
        assert_eq!(
            catalog.terms("Gg^Fp", Defense),
            [
                Terrain("Gt"),
                Worst,
                Terrain("Wst"),
                Best,
                Terrain("Ht"),
                Best,
                Terrain("Ft"),
            ]
        );
    }

    #[test]
    fn exact_composites_take_precedence_and_unknown_codes_resolve_to_themselves() {
        let catalog = aliases(
            "[terrain_type]\nstring=Gg\nid=grass\n[/terrain_type]\n\
             [terrain_type]\nstring=^Fp\naliasof=Ft\n[/terrain_type]\n\
             [terrain_type]\nstring=Gg^Fp\naliasof=Ht\n[/terrain_type]\n\
             [terrain_type]\nstring=^Vh\naliasof=Vt\n[/terrain_type]",
        );
        assert_eq!(catalog.terms("Gg^Fp", Movement), [Terrain("Ht")]);
        assert_eq!(catalog.terms("Gg^Vh", Movement), [Terrain("Vt")]);
        assert_eq!(catalog.terms("Gg^", Defense), [Terrain("Gg")]);
        assert_eq!(catalog.terms("Hh^Fp", Defense), [Terrain("Hh^Fp")]);
    }

    #[test]
    fn terrain_features_combine_known_layers_and_default_for_unknown_codes() {
        let catalog = aliases(
            r#"
            [terrain_type]
                string=Ch
                recruit_onto=yes
                heals=3
            [/terrain_type]
            [terrain_type]
                string=^Vh
                gives_income=true
                recruit_from=yes
                heals=8
            [/terrain_type]
            [terrain_type]
                string=^Fp
                heals=-2
            [/terrain_type]
            "#,
        );
        assert_eq!(
            catalog.features("Ch^Vh"),
            TerrainFeatures {
                village: true,
                castle: true,
                keep: true,
                healing: 8,
            }
        );
        assert_eq!(
            catalog.features("Ch^Fp"),
            TerrainFeatures {
                castle: true,
                healing: 3,
                ..TerrainFeatures::default()
            }
        );
        assert_eq!(catalog.features("Ch^Fj"), TerrainFeatures::default());
        assert_eq!(catalog.features("Gg^Vh"), TerrainFeatures::default());
    }

    #[test]
    fn exact_terrain_features_override_layers_and_keep_the_first_definition() {
        let catalog = aliases(
            r#"
            [terrain_type]
                string=Ch
                recruit_onto=yes
            [/terrain_type]
            [terrain_type]
                string=^Vh
                gives_income=yes
                heals=8
            [/terrain_type]
            [terrain_type]
                string=Ch^Vh
                recruit_from=true
                gives_income=no
                recruit_onto=1
                heals=-1
            [/terrain_type]
            [terrain_type]
                string=Ch^Vh
                gives_income=yes
                heals=7
            [/terrain_type]
            "#,
        );
        assert_eq!(
            catalog.features("Ch^Vh"),
            TerrainFeatures {
                keep: true,
                healing: -1,
                ..TerrainFeatures::default()
            }
        );
        assert_eq!(
            catalog.features("Ch^"),
            TerrainFeatures {
                castle: true,
                ..TerrainFeatures::default()
            }
        );
    }
}
