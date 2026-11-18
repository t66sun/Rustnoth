use super::{ArrivingUnit, UnitFilter};
use crate::{board::UnitState, hex::Hex, wml::Node};

#[derive(Clone, Debug)]
pub(crate) struct KillAction {
    victims: UnitFilter,
    killer: Option<UnitFilter>,
    event_hex: bool,
    on_map: bool,
    on_recall: bool,
    pub(crate) fire_event: bool,
}

impl KillAction {
    pub(super) fn read(cfg: &Node) -> Option<Self> {
        let mut filter = cfg.clone();
        let mut killer = None;
        for (tag, child) in std::mem::take(&mut filter.children) {
            match tag.as_str() {
                "secondary_unit" if killer.is_none() => killer = Some(UnitFilter::read(&child)?),
                "primary_attack" | "secondary_attack" => {} // Death-animation weapons.
                _ => return None,
            }
        }
        let flag = filter.attributes.remove("fire_event");
        let fire_event = match flag.as_ref().map(|value| value.text.as_str()) {
            None | Some("" | "no" | "false" | "0") => false,
            Some("yes" | "true" | "1") => true,
            _ => return None,
        };
        filter.attributes.remove("animate");
        let (x, y) = (filter.attribute("x"), filter.attribute("y"));
        let event_hex = x == Some("$x1") && y == Some("$y1");
        let on_map = x != Some("recall") && y != Some("recall");
        let on_recall = [x, y].iter().all(|v| matches!(v, None | Some("recall")));
        if event_hex || (!on_map && on_recall) {
            filter.attributes.remove("x");
            filter.attributes.remove("y");
        }
        Some(Self { victims: UnitFilter::read(&filter)?, killer, event_hex, on_map, on_recall, fire_event })
    }

    pub(crate) fn matches_map(&self, position: Hex, unit: &UnitState<'_>, primary: Option<Hex>) -> bool {
        self.on_map && (!self.event_hex || primary == Some(position)) && matches(&self.victims, position, unit)
    }

    pub(crate) fn matches_recall(&self, unit: &UnitState<'_>) -> bool {
        self.on_recall && self.victims.matches_fields(None, unit.side, unit.id.as_deref(), &unit.unit_type.id)
    }

    pub(crate) fn matches_killer(&self, position: Hex, unit: &UnitState<'_>) -> bool {
        self.killer.as_ref().is_some_and(|filter| matches(filter, position, unit))
    }
}

fn matches(filter: &UnitFilter, position: Hex, unit: &UnitState<'_>) -> bool {
    filter.matches(&ArrivingUnit { position, side: unit.side, id: unit.id.clone(), type_id: &unit.unit_type.id })
}
