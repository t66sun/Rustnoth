use std::collections::{BTreeMap, BTreeSet};

use super::{Node, UnitState, UnitType};

/// The literal unit selectors used on recall leaders in the stock campaigns.
#[derive(Clone, Debug, Default)]
pub(super) struct RecallFilter(Vec<Selector>);

#[derive(Clone, Debug)]
enum Selector {
    Id(BTreeSet<String>),
    Type(BTreeSet<String>),
    Race(BTreeSet<String>),
    Unsupported,
}

impl RecallFilter {
    // Store resolved type trees from the previous scenario's catalog.
    pub(super) fn config(&self) -> Option<Node> {
        if self.0.is_empty() { return None; }
        let mut node = Node::default();
        let mut resolved_types: Option<BTreeSet<String>> = None;
        for selector in &self.0 {
            let (key, ids) = match selector {
                Selector::Unsupported => {
                    node.children.push(("unsupported".into(), Node::default()));
                    return Some(node);
                }
                Selector::Type(ids) => {
                    if let Some(types) = &mut resolved_types { types.retain(|id| ids.contains(id)); }
                    else { resolved_types = Some(ids.clone()); }
                    continue;
                }
                Selector::Race(ids) => ("race", ids),
                Selector::Id(ids) => ("id", ids),
            };
            node.set_literal(key, ids.iter().cloned().collect::<Vec<_>>().join(","));
        }
        if let Some(types) = resolved_types { node.set_literal("type", types.into_iter().collect::<Vec<_>>().join(",")); }
        Some(node)
    }

    pub(super) fn read(node: Option<Node>, types: &BTreeMap<String, UnitType>) -> Self {
        let Some(node) = node else { return Self::default() };
        if !node.children.is_empty() {
            return Self(vec![Selector::Unsupported]);
        }
        let selectors = node.attributes.into_iter().map(|(key, value)| {
            if !value.translations.is_empty() || value.text.contains('$') {
                return Selector::Unsupported;
            }
            let ids = value.text.split(',').map(str::trim).filter(|id| !id.is_empty());
            match key.as_str() {
                "id" => Selector::Id(ids.map(str::to_owned).collect()),
                "race" => Selector::Race(ids.map(str::to_owned).collect()),
                "type" => Selector::Type(ids.map(str::to_owned).collect()),
                "type_adv_tree" => {
                    let mut tree = BTreeSet::new();
                    let mut pending: Vec<_> = ids.filter_map(|id| types.get(id)).collect();
                    while let Some(unit_type) = pending.pop() {
                        if tree.insert(unit_type.id.clone()) {
                            pending.extend(unit_type.advances_to.iter().filter_map(|id| types.get(id)));
                        }
                    }
                    Selector::Type(tree)
                }
                _ => Selector::Unsupported,
            }
        }).collect();
        Self(selectors)
    }
}

impl UnitState<'_> {
    pub(crate) fn can_recall_unit(&self, veteran: &UnitState<'_>) -> bool {
        self.recall_filter.0.iter().all(|selector| match selector {
            Selector::Id(ids) => veteran.id.as_ref().is_some_and(|id| ids.contains(id)),
            Selector::Race(races) => races.contains(&veteran.unit_type.race),
            Selector::Type(types) => types.contains(&veteran.unit_type.id),
            Selector::Unsupported => false,
        })
    }
}
