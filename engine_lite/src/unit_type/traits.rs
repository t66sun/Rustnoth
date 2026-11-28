use std::{collections::BTreeMap, sync::Arc};

use crate::{rng::ReplayRng, scenario_time::integer, time_of_day::Alignment, wml::Node};

/// The requested trait count and the available WML definitions in native order.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct TraitPool {
    pub count: i32,
    /// Shared definitions retain eligibility attributes and ordered effects.
    pub available: Vec<Arc<Node>>,
}

impl TraitPool {
    pub(crate) fn mandatory(&self) -> Vec<&Node> {
        let mut selected = Vec::new();
        for definition in self.available.iter().map(Arc::as_ref) {
            if definition.attribute("availability") == Some("musthave")
                && !selected.iter().any(|&trait_| trait_id(trait_) == trait_id(definition))
            {
                selected.push(definition);
            }
        }

        selected
    }

    /// Selects the traits of a fresh non-leader recruit in native selection order.
    pub fn select_for_recruit(&self, rng: &mut ReplayRng) -> Vec<&Node> {
        self.select_for_unit(Vec::new(), false, rng)
    }

    /// Additional traits after the authored modifications, in native selection order.
    pub(crate) fn select_for_unit<'a>(&'a self, mut selected: Vec<&'a Node>, leader: bool, rng: &mut ReplayRng) -> Vec<&'a Node> {
        let existing = selected.len();
        for required in self.mandatory() {
            if !selected.iter().any(|t| trait_id(t) == trait_id(required)) { selected.push(required); }
        }
        let mut candidates = Vec::new();
        while selected.len() < self.count.max(0) as usize {
            let has_trait = |id| selected.iter().any(|&trait_| trait_id(trait_) == id);
            let exclusions: Vec<_> = selected
                .iter()
                .flat_map(|&trait_| listed_traits(trait_, "exclude_traits"))
                .collect();
            candidates.extend(self.available.iter().map(Arc::as_ref).filter(|definition| {
                let id = trait_id(definition);
                !has_trait(id)
                    && (!leader || definition.attribute("availability") == Some("any"))
                    && listed_traits(definition, "require_traits").all(has_trait)
                    && !listed_traits(definition, "exclude_traits")
                        .chain(exclusions.iter().copied())
                        .any(|excluded| excluded == id || has_trait(excluded))
            }));
            if candidates.is_empty() {
                break;
            }
            let index = rng.next_u32() as usize % candidates.len();
            selected.push(candidates[index]);
            candidates.clear();
        }
        selected.split_off(existing)
    }
}

fn trait_id(definition: &Node) -> &str {
    definition.attribute("id").unwrap_or_default()
}

fn listed_traits<'a>(definition: &'a Node, key: &str) -> impl Iterator<Item = &'a str> + use<'a> {
    definition
        .attribute(key)
        .unwrap_or_default()
        .split(',')
        .map(|id| id.trim_matches([' ', '\t', '\r', '\n']))
        .filter(|id| !id.is_empty())
}

struct RaceTraits {
    undead_variation: String,
    count: i32,
    ignore_global: bool,
    additional: Vec<Arc<Node>>,
}

#[derive(Default)]
pub(super) struct TraitCatalog {
    global: Vec<Arc<Node>>,
    races: BTreeMap<String, RaceTraits>,
}

impl TraitCatalog {
    pub(super) fn read(units: Node) -> (Self, Vec<Node>) {
        let mut catalog = Self::default();
        let mut unit_definitions = Vec::new();
        for (tag, definition) in units.children {
            match tag.as_str() {
                "unit_type" => unit_definitions.push(definition),
                "trait" => catalog.global.push(Arc::new(definition)),
                "race" => {
                    let id = definition.attribute("id").unwrap_or_default().to_owned();
                    catalog.races.entry(id).or_insert_with(|| RaceTraits {
                        undead_variation: definition.attribute("undead_variation").unwrap_or_default().to_owned(),
                        count: integer(&definition, "num_traits").unwrap_or_default(),
                        ignore_global: enabled(&definition, "ignore_global_traits"),
                        additional: definition
                            .children
                            .into_iter()
                            .filter(|(tag, _)| tag == "trait")
                            .map(|(_, trait_)| Arc::new(trait_))
                            .collect(),
                    });
                }
                _ => {}
            }
        }
        (catalog, unit_definitions)
    }

    pub(super) fn inherited_traits(&self, unit: &Node, alignment: Alignment) -> TraitPool {
        let race = self.races.get(unit.attribute("race").unwrap_or_default());
        let count = integer(unit, "num_traits").unwrap_or_else(|| race.map_or(0, |race| race.count));
        let ignore_race = race.is_some() && enabled(unit, "ignore_race_traits");
        let mut available = if ignore_race || race.is_some_and(|race| race.ignore_global) {
            Vec::new()
        } else {
            self.global.clone()
        };

        if let Some(race) = race.filter(|_| !ignore_race) {
            available.extend(
                race.additional
                    .iter()
                    .filter(|trait_| {
                        alignment != Alignment::Neutral || trait_.attribute("id") != Some("fearless")
                    })
                    .cloned(),
            );
        }
        TraitPool { count, available }
    }

    pub(super) fn undead_variation<'a>(&'a self, unit: &'a Node) -> &'a str {
        unit.attribute("undead_variation").filter(|value| !value.is_empty())
            .or_else(|| self.races.get(unit.attribute("race")?).map(|race| race.undead_variation.as_str()))
            .unwrap_or_default()
    }
}

fn enabled(node: &Node, key: &str) -> bool {
    node.attributes.get(key).is_some_and(|value| {
        value.translations.is_empty() && matches!(value.text.as_str(), "yes" | "true")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{unit_type::UnitType, wml};

    #[test]
    fn draws_in_catalog_order_without_repeating_traits() {
        let unit = UnitType::from_wml(
            wml::parse(
                r#"
                num_traits=2
                [trait]
                    id=strong
                [/trait]
                [trait]
                    id=quick
                [/trait]
                [trait]
                    id=intelligent
                [/trait]
                [trait]
                    id=resilient
                [/trait]
                "#,
            )
            .unwrap(),
        );
        let mut rng = ReplayRng::new(5489);

        let selected = unit.traits.select_for_recruit(&mut rng);
        assert_eq!(selected.into_iter().map(trait_id).collect::<Vec<_>>(), ["strong", "quick"]);
        assert_eq!(rng.seed_and_calls(), (5489, 2));
    }

    #[test]
    fn adds_mandatory_traits_then_rechecks_requirements_and_exclusions() {
        let mut unit = UnitType::from_wml(
            wml::parse(
                r#"
                num_traits=10
                [trait]
                    id=second
                    require_traits=first, root
                [/trait]
                [trait]
                    id=root
                    availability=musthave
                    exclude_traits=blocked_by_root
                [/trait]
                [trait]
                    id=blocked_by_root
                [/trait]
                [trait]
                    id=side
                    availability=musthave
                [/trait]
                [trait]
                    id=root
                    availability=musthave
                [/trait]
                [trait]
                    id=blocked_by_candidate
                    exclude_traits=side
                [/trait]
                [trait]
                    id=first
                    require_traits= root, side, ,
                [/trait]
                [trait]
                    id=incomplete_requirements
                    require_traits=first,missing
                [/trait]
                [trait]
                    id=self_excluding
                    exclude_traits=self_excluding
                [/trait]
                "#,
            )
            .unwrap(),
        );
        let mut rng = ReplayRng::new(5489);

        let selected = unit.traits.select_for_recruit(&mut rng);
        assert_eq!(
            selected.into_iter().map(trait_id).collect::<Vec<_>>(),
            ["root", "side", "first", "second"],
        );
        assert_eq!(rng.seed_and_calls(), (5489, 2));

        unit.traits.count = 1;
        let selected = unit.traits.select_for_recruit(&mut rng);
        assert_eq!(selected.into_iter().map(trait_id).collect::<Vec<_>>(), ["root", "side"]);
        assert_eq!(rng.seed_and_calls(), (5489, 2));
    }
}
