//! Resolve the ordered terrain aliases used by movement costs and defense.

use std::collections::HashMap;

use crate::wml::Node;

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

#[derive(Debug)]
pub struct TerrainAliases {
    definitions: HashMap<String, Definition>,
}

#[derive(Debug)]
struct Definition {
    id: String,
    movement: Vec<String>,
    defense: Vec<String>,
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

    /// Resolve one layer of aliasing. Callers can evaluate terrain references
    /// recursively with their own movement or defense values.
    pub fn terms<'a>(&'a self, code: &'a str, kind: AliasKind) -> Vec<AliasTerm<'a>> {
        let code = canonical_code(code);
        if let Some(definition) = self.definitions.get(code) {
            return alias_terms(definition.terms(kind)).collect();
        }

        if let Some(overlay_start) = code.find('^') {
            let (base, overlay) = code.split_at(overlay_start);
            if let (Some(base), Some(overlay)) =
                (self.definitions.get(base), self.definitions.get(overlay))
            {
                return merge_aliases(base.terms(kind), overlay.terms(kind));
            }
        }

        vec![AliasTerm::Terrain(code)]
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
    use super::TerrainAliases;
    use crate::wml;

    fn aliases(config: &str) -> TerrainAliases {
        TerrainAliases::from_wml(&wml::parse(config).unwrap())
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
}
