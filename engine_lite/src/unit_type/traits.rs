use std::{collections::BTreeMap, sync::Arc};

use crate::{scenario_time::integer, time_of_day::Alignment, wml::Node};

/// The requested trait count and the available WML definitions in native order.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct TraitPool {
    pub count: i32,
    /// Shared definitions retain eligibility attributes and ordered effects.
    pub available: Vec<Arc<Node>>,
}

struct RaceTraits {
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
}

fn enabled(node: &Node, key: &str) -> bool {
    node.attributes.get(key).is_some_and(|value| {
        value.translations.is_empty() && matches!(value.text.as_str(), "yes" | "true")
    })
}
