use super::{TraitCatalog, UnitType};
use crate::wml::Node;
use std::collections::{BTreeMap, BTreeSet};

impl UnitType {
    pub(crate) fn variation(&self, id: &str) -> &Self {
        self.variations.get(id).unwrap_or(self)
    }
}

/// The native [variation] config inherits same-tag children by ordinal, including
/// nested weapon specials. Only the parts used by the rules catalog need copies.
pub(super) fn read(base: &Node, catalog: &TraitCatalog) -> BTreeMap<String, UnitType> {
    let mut variations = BTreeMap::new();
    for (_, variation) in base.children.iter().filter(|(tag, _)| tag == "variation") {
        let Some(id) = variation.attribute("variation_id") else { continue };
        let mut config = rules_config(variation);
        if matches!(variation.attribute("inherit"), Some("yes" | "true")) {
            let mut inherited = rules_config(base);
            merge(&mut inherited, config);
            config = inherited;
        }
        // Native variations retain their parent's type ID.
        config.attributes.insert("id".into(), base.attributes.get("id").cloned().unwrap_or_default());
        variations.entry(id.to_owned()).or_insert_with(|| UnitType::from_catalog(config, catalog));
    }
    variations
}

fn rules_config(node: &Node) -> Node {
    Node {
        attributes: node.attributes.clone(),
        children: node.children.iter().filter(|(tag, _)| matches!(tag.as_str(),
            "abilities" | "attack" | "movement_costs" | "vision_costs" |
            "defense" | "resistance" | "trait"
        )).cloned().collect(),
    }
}

fn merge(base: &mut Node, overlay: Node) {
    base.attributes.extend(overlay.attributes);
    let mut indices = BTreeMap::<String, Vec<usize>>::new();
    for (index, (tag, _)) in base.children.iter().enumerate().rev() {
        indices.entry(tag.clone()).or_default().push(index);
    }
    let mut removed = BTreeSet::new();
    for (tag, child) in overlay.children {
        if let Some(index) = indices.get_mut(&tag).and_then(Vec::pop) {
            if matches!(child.attribute("__remove"), Some("yes" | "true")) {
                removed.insert(index);
            } else {
                merge(&mut base.children[index].1, child);
            }
        } else {
            base.children.push((tag, child));
        }
    }
    for index in removed.into_iter().rev() {
        base.children.remove(index);
    }
}
