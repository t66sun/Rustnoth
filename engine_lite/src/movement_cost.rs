use std::collections::BTreeMap;
use std::num::NonZeroU32;

use crate::terrain_aliases::{AliasKind, AliasTerm, TerrainAliases};
use crate::wml::Node;

const UNREACHABLE: u32 = 99;
const MAX_ALIAS_DEPTH: usize = 100;

/// A movetype's configured walking costs, keyed by WML terrain id.
#[derive(Debug, Default)]
pub struct MovementCosts {
    costs: BTreeMap<String, u32>,
}

impl MovementCosts {
    /// Read the contents of a preprocessed `[movement_costs]` tag.
    pub fn from_wml(table: &Node) -> Self {
        let costs = table
            .attributes
            .iter()
            .filter(|(_, value)| value.translations.is_empty())
            .filter_map(|(id, value)| {
                let cost = value.text.trim().parse::<i32>().ok()?;
                Some((id.clone(), cost.clamp(1, UNREACHABLE as i32) as u32))
            })
            .collect();
        Self { costs }
    }

    /// Return the walking cost of a terrain code, including slow if active.
    ///
    /// An unconfigured or impassable terrain returns `None`. Slow doubles a
    /// passable cost after alias evaluation, even when it rises above 99.
    pub fn cost(&self, aliases: &TerrainAliases, code: &str, slowed: bool) -> Option<NonZeroU32> {
        let base = self.base_cost(aliases, code, &mut Vec::new());
        if base == UNREACHABLE {
            return None;
        }
        NonZeroU32::new(if slowed { 2 * base } else { base })
    }

    fn base_cost<'a>(
        &self,
        aliases: &'a TerrainAliases,
        code: &'a str,
        ancestors: &mut Vec<&'a str>,
    ) -> u32 {
        if ancestors.len() > MAX_ALIAS_DEPTH || ancestors.contains(&code) {
            return UNREACHABLE;
        }
        let terms = aliases.terms(code, AliasKind::Movement);
        if terms.is_empty() || terms.as_slice() == [AliasTerm::Terrain(code)] {
            return aliases
                .terrain_id(code)
                .and_then(|id| self.costs.get(id).copied())
                .unwrap_or(UNREACHABLE);
        }

        let mut cost = if terms.first() == Some(&AliasTerm::Worst) {
            1
        } else {
            UNREACHABLE
        };
        let mut prefer_worst = false;
        ancestors.push(code);
        for term in terms {
            match term {
                AliasTerm::Best => prefer_worst = false,
                AliasTerm::Worst => prefer_worst = true,
                AliasTerm::Terrain(terrain) => {
                    let next = self.base_cost(aliases, terrain, ancestors);
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
