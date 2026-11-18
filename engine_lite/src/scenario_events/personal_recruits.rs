use crate::{board::UnitState, hex::Hex, wml::Node};
use super::{ArrivingUnit, UnitFilter};

#[derive(Clone, Debug)]
enum Operation { Allow, Disallow, Set }

#[derive(Clone, Debug)]
pub(crate) struct PersonalRecruits {
    filter: UnitFilter,
    operation: Operation,
    types: Vec<String>,
}

impl PersonalRecruits {
    pub(super) fn read(tag: &str, cfg: &Node) -> Option<Self> {
        let operation = match tag {
            "allow_extra_recruit" => Operation::Allow,
            "disallow_extra_recruit" => Operation::Disallow,
            "set_extra_recruit" => Operation::Set,
            _ => return None,
        };
        let mut selector = cfg.clone();
        let types = selector.attributes.remove("extra_recruit")?;
        if types.text.contains('$') || !types.translations.is_empty() { return None; }
        Some(Self { filter: UnitFilter::read(&selector)?, operation,
                    types: crate::scenario_sides::names(&types.text).collect() })
    }

    pub(crate) fn apply(&self, position: Hex, unit: &mut UnitState<'_>) {
        let context = ArrivingUnit { position, side: unit.side, id: unit.id.clone(), type_id: &unit.unit_type.id };
        if !self.filter.matches(&context) { return; }
        match self.operation {
            Operation::Allow => unit.extra_recruits.extend(self.types.iter().cloned()),
            Operation::Set => unit.extra_recruits.clone_from(&self.types),
            Operation::Disallow => for id in &self.types {
                if let Some(i) = unit.extra_recruits.iter().position(|existing| existing == id) {
                    unit.extra_recruits.remove(i);
                }
            },
        }
    }
}
