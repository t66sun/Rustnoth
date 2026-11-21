use std::collections::BTreeSet;

use super::{AuthoredStartingUnit, Battlefield, BoardError, Modification, UnitState, recall_filter::RecallFilter};

/// Owned referee data that can be rebound to the successor's unit catalog.
#[derive(Clone)]
pub(crate) struct Veteran {
    unit: AuthoredStartingUnit,
    earned_amlas: u8,
    recall_filter: RecallFilter,
}

impl Veteran {
    pub(crate) fn replay_config(&self) -> crate::wml::Node {
        use crate::{income::Upkeep, wml::Node};
        let u = &self.unit;
        let mut n = Node::default();
        n.set_literal("type", &u.type_id);
        n.set_literal("canrecruit", u.can_recruit);
        if u.guardian { n.set_literal("ai_special", "guardian"); }
        n.set_literal("variation", &u.variation);
        n.set_literal("experience", u.experience);
        n.set_literal("earned_amlas", self.earned_amlas);
        for (key, value) in [("id",&u.id), ("name",&u.display_name), ("profile",&u.portrait),
                ("undead_variation",&u.undead_variation)] {
            if let Some(value) = value { n.set_literal(key, value); }
        }
        if let Some(cost) = u.recall_cost { n.set_literal("recall_cost", cost); }
        if let Some(facing) = u.facing { n.set_literal("facing", ["n","ne","se","s","sw","nw"][facing as usize]); }
        if let Some(upkeep) = u.upkeep {
            n.set_literal("upkeep", match upkeep { Upkeep::Full => "full".into(), Upkeep::Fixed(v) => v.to_string() });
        }
        if !u.extra_recruits.is_empty() { n.set_literal("extra_recruit", u.extra_recruits.join(",")); }
        let mut status = Node::default();
        for (key, enabled) in [("undrainable",u.immunities.undrainable),("unslowable",u.immunities.unslowable),
            ("unpoisonable",u.immunities.unpoisonable),("unpetrifiable",u.unpetrifiable),
            ("unhealable",u.unhealable),("unplagueable",u.unplagueable)] {
            if enabled { status.set_literal(key, "yes"); }
        }
        if !status.attributes.is_empty() { n.children.push(("status".into(),status)); }
        if let Some(filter) = self.recall_filter.config() { n.children.push(("filter_recall".into(),filter)); }
        let mut mods = Node::default();
        for m in &u.modifications {
            let kind = match m { Modification::Trait(_) => "trait", Modification::Object(_) => "object",
                Modification::DefaultAdvancement(_) => "advancement" };
            mods.children.push((kind.into(),m.definition().clone()));
        }
        if !mods.children.is_empty() { n.children.push(("modifications".into(),mods)); }
        n
    }

    pub(crate) fn read_replay(n: &crate::wml::Node) -> Option<Self> {
        let type_id = n.attribute("type").filter(|s| !s.is_empty())?;
        let mut unit = crate::starting_units::authored_unit(n, 1, type_id, None, false,
            matches!(n.attribute("canrecruit"), Some("yes" | "true")));
        let recall_filter = RecallFilter::read(unit.recall_filter.take(), &Default::default());
        Some(Self { unit, earned_amlas: n.attribute("earned_amlas")?.parse().ok()?, recall_filter })
    }

    fn snapshot(unit: &UnitState<'_>) -> Self {
        let modifications = unit.traits.iter().flat_map(|traits| &traits.definitions).map(|m| match m {
            Modification::Trait(cfg) => Modification::Trait(std::borrow::Cow::Owned(cfg.as_ref().clone())),
            Modification::Object(cfg) => Modification::Object(cfg.clone()),
            Modification::DefaultAdvancement(cfg) => Modification::DefaultAdvancement(cfg.clone()),
        }).collect();
        Self {
            unit: AuthoredStartingUnit {
                side: unit.side, id: unit.id.clone(), type_id: unit.unit_type.id.clone(),
                variation: unit.unit_type.variation_id.clone(), position: None,
                display_name: unit.display_name.clone(), portrait: unit.portrait.clone(),
                facing: unit.facing,
                can_recruit: unit.can_recruit, extra_recruits: unit.extra_recruits.clone(),
                upkeep: unit.upkeep_override, recall_cost: Some(unit.recall_cost), recall_filter: None,
                modifications, experience: unit.experience, hitpoints: None, moves_left: None, attacks_left: None,
                immunities: unit.immunities, unpetrifiable: unit.unpetrifiable, unhealable: unit.unhealable,
                unplagueable: unit.unplagueable, undead_variation: unit.undead_variation.clone(),
                poisoned: false, slowed: false, petrified: false, uncovered: false, guardian: unit.guardian,
            },
            earned_amlas: unit.default_amlas,
            recall_filter: unit.recall_filter.clone(),
        }
    }
}

impl Battlefield<'_> {
    pub(crate) fn surviving_veterans(&self, side: usize) -> Vec<Veteran> {
        self.units.values().filter(|u| u.side == side)
            .chain(self.recall_roster(side)).filter(|u| u.hitpoints > 0).map(Veteran::snapshot).collect()
    }

    pub(crate) fn restore_veterans(&mut self, side: usize, veterans: &[Veteran]) -> Result<(), BoardError> {
        let mut ids: BTreeSet<_> = self.units.values().chain(self.recall.values().flatten())
            .filter_map(|unit| unit.id.clone()).chain(veterans.iter().filter_map(|v| v.unit.id.clone())).collect();
        let mut sequence = 0;
        for veteran in veterans {
            let mut restored = veteran.unit.clone();
            restored.side = side;
            let position = if let Some(id) = &restored.id {
                self.units.iter().find(|(_, unit)| unit.side == side && unit.id.as_ref() == Some(id)).map(|(&p, _)| p)
            } else {
                // Recruits without authored IDs need an addressable ID on the recall list.
                loop {
                    sequence += 1;
                    let id = format!("{}-{}", restored.type_id, sequence);
                    if ids.insert(id.clone()) { restored.id = Some(id); break; }
                }
                None
            };
            restored.position = position;
            if let Some(position) = position {
                self.units.remove(&position);
            } else if let Some(roster) = self.recall.get_mut(&side) {
                roster.retain(|u| u.id != restored.id);
            }
            self.add_authored_unit(restored)?;
            let unit = match position {
                Some(p) => self.units.get_mut(&p).unwrap(),
                None => self.recall.get_mut(&side).unwrap().last_mut().unwrap(),
            };
            unit.recall_filter = veteran.recall_filter.clone();
            unit.default_amlas = veteran.earned_amlas;
            unit.hitpoints = unit.max_hitpoints();
        }
        Ok(())
    }
}
