use std::collections::BTreeMap;
use std::num::NonZeroU32;

use crate::terrain_aliases::{AliasKind, AliasTerm, TerrainAliases};
use crate::wml::Node;

const UNREACHABLE: u32 = 99;
const MAX_ALIAS_DEPTH: usize = 100;

/// Walking or vision costs keyed by WML terrain id.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct MovementCosts {
    costs: BTreeMap<String, u32>,
}

impl MovementCosts {
    /// The literal replacement table supported by WML objects changing terrain costs.
    pub(crate) fn replacement(effect: &Node) -> Option<&Node> {
        if effect.attributes.len() != 2 || effect.attribute("apply_to") != Some("movement_costs")
            || !crate::scenario_sides::bool_attribute(effect, "replace", false) { return None; }
        let [(tag, table)] = effect.children.as_slice() else { return None };
        (tag == "movement_costs" && table.children.is_empty() && !table.attributes.is_empty()
            && table.attributes.values().all(|v| v.translations.is_empty() && v.text.trim().parse::<i32>().is_ok()))
            .then_some(table)
    }

    /// Read the contents of a preprocessed terrain cost table.
    pub fn from_wml(table: &Node) -> Self {
        Self::from_wml_tables([table])
    }

    /// Read tables in declaration order, with the last value for each id.
    pub(crate) fn from_wml_tables<'a>(tables: impl IntoIterator<Item = &'a Node>) -> Self {
        let mut costs = BTreeMap::new();
        for table in tables {
            for (id, value) in &table.attributes {
                let cost = if value.translations.is_empty() {
                    value.text.trim().parse::<i32>().unwrap_or(UNREACHABLE as i32)
                } else {
                    UNREACHABLE as i32
                };
                // An explicit invalid value uses the unreachable default,
                // including when it replaces an inherited cost.
                costs.insert(id.clone(), cost.clamp(1, UNREACHABLE as i32) as u32);
            }
        }
        Self { costs }
    }

    /// Return the walking cost of a terrain code, including slow if active.
    ///
    /// An unconfigured or impassable terrain returns `None`. Slow doubles a
    /// passable cost after alias evaluation, even when it rises above 99.
    pub fn cost(&self, aliases: &TerrainAliases, code: &str, slowed: bool) -> Option<NonZeroU32> {
        self.cost_with_fallback(aliases, code, slowed, None)
    }

    pub(crate) fn cost_with_fallback(
        &self,
        aliases: &TerrainAliases,
        code: &str,
        slowed: bool,
        fallback: Option<&Self>,
    ) -> Option<NonZeroU32> {
        let cost = Self::layered_cost(aliases, code, slowed, &[Some(self), fallback]);
        (cost.get() != UNREACHABLE).then_some(cost)
    }

    /// Resolve each alias leaf against the tables in order of precedence.
    pub(crate) fn layered_cost(
        aliases: &TerrainAliases,
        code: &str,
        slowed: bool,
        layers: &[Option<&Self>],
    ) -> NonZeroU32 {
        let base = Self::base_cost(aliases, code, &mut Vec::new(), layers);
        let cost = if slowed && base != UNREACHABLE { 2 * base } else { base };
        NonZeroU32::new(cost).expect("terrain costs are at least one")
    }

    fn base_cost<'a>(
        aliases: &'a TerrainAliases,
        code: &'a str,
        ancestors: &mut Vec<&'a str>,
        layers: &[Option<&Self>],
    ) -> u32 {
        if ancestors.len() > MAX_ALIAS_DEPTH || ancestors.contains(&code) {
            return UNREACHABLE;
        }
        let source = aliases.term_source(code, AliasKind::Movement);
        let mut terms = source.iter().peekable();
        let first = terms.next();
        if first.is_none() || (first == Some(AliasTerm::Terrain(code)) && terms.peek().is_none()) {
            return aliases
                .terrain_id(code)
                .and_then(|id| layers.iter().flatten().find_map(|table| table.costs.get(id)))
                .copied()
                .unwrap_or(UNREACHABLE);
        }

        let mut cost = if first == Some(AliasTerm::Worst) {
            1
        } else {
            UNREACHABLE
        };
        let mut prefer_worst = false;
        ancestors.push(code);
        for term in first.into_iter().chain(terms) {
            match term {
                AliasTerm::Best => prefer_worst = false,
                AliasTerm::Worst => prefer_worst = true,
                AliasTerm::Terrain(terrain) => {
                    let next = Self::base_cost(aliases, terrain, ancestors, layers);
                    cost = if prefer_worst {
                        cost.max(next)
                    } else {
                        cost.min(next)
                    };
                }
            }
        }
        ancestors.pop();
        cost
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wml;

    fn catalog(config: &str) -> TerrainAliases {
        TerrainAliases::from_wml(&wml::parse(config).unwrap())
    }

    fn costs(table: &str) -> MovementCosts {
        MovementCosts::from_wml(&wml::parse(table).unwrap())
    }

    #[test]
    fn walking_cost_borrows_stock_ground_aliases_and_preserves_table_overrides() {
        use crate::terrain_aliases::TERM_VECTOR_BUILDS;

        let aliases = catalog(
            "[terrain_type]\nstring=Gt\nid=flat\n[/terrain_type]\n\
             [terrain_type]\nstring=Gg\nid=grass\naliasof=Gt\n[/terrain_type]\n\
             [terrain_type]\nstring=Wst\nid=shallow_water\n[/terrain_type]\n\
             [terrain_type]\nstring=Ww\nid=water\naliasof=Wst\n[/terrain_type]",
        );
        let inherited = costs("flat=2\nshallow_water=1\n");
        let overrides = costs("shallow_water=unknown");
        let builds = TERM_VECTOR_BUILDS.get();

        assert_eq!(
            overrides.cost_with_fallback(&aliases, " 1 Gg^ ", true, Some(&inherited)),
            NonZeroU32::new(4),
        );
        assert_eq!(inherited.cost(&aliases, "Gt", false), NonZeroU32::new(2));
        assert_eq!(overrides.cost_with_fallback(&aliases, "Ww", false, Some(&inherited)), None);
        assert_eq!(TERM_VECTOR_BUILDS.get(), builds);
    }

    #[test]
    fn reads_terrain_ids_and_applies_slow_after_clamping() {
        let aliases = catalog(
            "[terrain_type]\nstring=Gg\nid=grassland\n[/terrain_type]\n\
             [terrain_type]\nstring=Mm\nid=mountains\n[/terrain_type]\n\
             [terrain_type]\nstring=Wst\nid=shallow_water\n[/terrain_type]\n\
             [terrain_type]\nstring=Xu\nid=impassable\n[/terrain_type]",
        );
        let costs = costs("grassland=0\nmountains=50\nshallow_water=120\nimpassable=unknown\n");

        assert_eq!(
            costs.cost(&aliases, "Gg", true).map(NonZeroU32::get),
            Some(2)
        );
        assert_eq!(
            costs.cost(&aliases, "Mm", true).map(NonZeroU32::get),
            Some(100)
        );
        assert_eq!(costs.cost(&aliases, "Wst", true), None);
        assert_eq!(costs.cost(&aliases, "Xu", false), None);
        assert_eq!(costs.cost(&aliases, "Xx", false), None);
        assert_eq!(MovementCosts::default().cost(&aliases, "Gg", false), None);
    }

    #[test]
    fn walks_aliases_in_order_and_looks_up_leaf_terrain_ids() {
        let aliases = catalog(
            "[terrain_type]\nstring=Gg\nid=grassland\n[/terrain_type]\n\
             [terrain_type]\nstring=Hh\nid=hills\n[/terrain_type]\n\
             [terrain_type]\nstring=Ft\nid=forest\n[/terrain_type]\n\
             [terrain_type]\nstring=^Fp\nmvt_alias=\"_bas,-,Hh,+,Ft\"\n[/terrain_type]\n\
             [terrain_type]\nstring=Mx\nid=forest\nmvt_alias=\"-,Hh,Gg^Fp\"\n[/terrain_type]\n\
             [terrain_type]\nstring=Ax\nmvt_alias=Mx\n[/terrain_type]",
        );
        let costs = costs("grassland=1\nhills=3\nforest=2\n");

        assert_eq!(
            costs.cost(&aliases, "Gg^Fp", true).map(NonZeroU32::get),
            Some(4)
        );
        assert_eq!(
            costs.cost(&aliases, "Ax", false).map(NonZeroU32::get),
            Some(3)
        );
    }

    #[test]
    fn missing_terrain_costs_take_part_in_the_alias_comparison() {
        let aliases = catalog(
            "[terrain_type]\nstring=Gg\nid=grassland\n[/terrain_type]\n\
             [terrain_type]\nstring=Wwf\nmvt_alias=\"Gg,Qt\"\n[/terrain_type]\n\
             [terrain_type]\nstring=Ww\nmvt_alias=\"-,Gg,Qt\"\n[/terrain_type]",
        );
        let costs = costs("grassland=2\n");

        assert_eq!(
            costs.cost(&aliases, "Wwf", false).map(NonZeroU32::get),
            Some(2)
        );
        assert_eq!(costs.cost(&aliases, "Ww", false), None);
    }
}
