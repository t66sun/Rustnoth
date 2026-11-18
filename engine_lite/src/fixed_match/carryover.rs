//! Campaign treasury settlement and transfer between scenarios.

use std::collections::BTreeMap;
use crate::{hex::Hex, income::DEFAULT_BASE_INCOME, scenario_sides::bool_attribute, wml::{Node, Value}};
use super::FixedMatch;

/// Unclaimed treasuries for the current campaign route. Used by the scenario loader.
#[derive(Default)]
pub struct CampaignGold(BTreeMap<String, (i64, bool)>);

fn save_id(side: &Node) -> Option<&str> {
    side.attribute("save_id").filter(|id| !id.is_empty())
        .or_else(|| side.attribute("id").filter(|id| !id.is_empty()))
        .or_else(|| side.children.iter().find(|(tag, _)| tag == "leader")
            .and_then(|(_, leader)| leader.attribute("id")).filter(|id| !id.is_empty()))
}

impl CampaignGold {
    /// Consume each matching record before the next scenario's opening events.
    pub fn transfer_to(&mut self, scenario: &mut Node) {
        for (_, side) in scenario.children.iter_mut().filter(|(tag, _)| tag == "side") {
            let Some((retained, add)) = save_id(side).and_then(|id| self.0.remove(id)) else { continue };
            let minimum = side.attribute("gold").filter(|s| !s.is_empty())
                .map_or(100, |s| s.trim().parse::<i32>().unwrap_or(0)) as i64;
            let gold = if add && retained > 0 { minimum.saturating_add(retained) } else { minimum.max(retained) };
            side.attributes.insert("gold".into(), Value {
                text: gold.clamp(i32::MIN.into(), i32::MAX.into()).to_string(), translations: vec![],
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wml;

    #[test]
    fn absent_sides_keep_their_treasury_and_transfer_once_above_the_minimum() {
        let mut gold = CampaignGold([("ally".into(), (67, false)), ("hero".into(), (-4, true))].into());
        let mut first = wml::parse("[side]\nid=hero\ngold=30\n[/side]").unwrap();
        gold.transfer_to(&mut first);
        assert_eq!(first.children[0].1.attribute("gold"), Some("30"));
        let mut later = wml::parse("[side]\ngold=20\n[leader]\nid=ally\n[/leader]\n[/side]\n[side]\nid=ally\ngold=5\n[/side]").unwrap();
        gold.transfer_to(&mut later);
        assert_eq!(later.children[0].1.attribute("gold"), Some("67"));
        assert_eq!(later.children[1].1.attribute("gold"), Some("5"));
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct GoldOverride {
    percentage: Option<i32>,
    add: Option<bool>,
    bonus: Option<f64>,
}

fn bonus(raw: &str) -> Option<f64> {
    match raw {
        "yes" | "true" => Some(1.0),
        "no" | "false" => Some(0.0),
        _ => raw.trim().parse::<f64>().ok().filter(|v| v.is_finite()),
    }
}

impl GoldOverride {
    pub(crate) fn endlevel(cfg: &Node, sides: usize) -> Vec<Self> {
        let mut settings = Vec::with_capacity(sides);
        for side in 1..=sides {
            // The final [result] for a side takes precedence over the global command.
            let specific = cfg.children.iter().rev().find(|(tag, child)| tag == "result" &&
                child.attribute("side").and_then(|id| id.trim().parse().ok()) == Some(side));
            let value = |key| specific.and_then(|(_, child)| child.attribute(key)).or_else(|| cfg.attribute(key));
            settings.push(Self {
                percentage: value("carryover_percentage").and_then(|s| s.trim().parse().ok()),
                add: value("carryover_add").and_then(|s| match s {
                    "yes" | "true" => Some(true), "no" | "false" => Some(false), _ => None,
                }),
                bonus: value("bonus").and_then(bonus),
            });
        }
        settings
    }
}

#[derive(Debug)]
pub(crate) struct Treasury {
    save_id: Option<String>,
    percentage: i32,
    add: bool,
    bonus: f64,
    remove_on_defeat: bool,
}

impl Treasury {
    pub(crate) fn read(scenario: &Node, side: &Node) -> Self {
        let persistent = bool_attribute(side, "persistent", side.attribute("controller") == Some("human"));
        let fallback = |key| side.attribute(key).filter(|v| !v.is_empty()).or_else(|| scenario.attribute(key));
        Self {
            save_id: save_id(side).filter(|_| persistent && !bool_attribute(side, "lost", false)).map(str::to_owned),
            percentage: fallback("carryover_percentage").and_then(|p| p.trim().parse().ok()).unwrap_or(80),
            add: matches!(fallback("carryover_add"), Some("yes" | "true")),
            bonus: side.attribute("carryover_bonus").and_then(bonus).unwrap_or(1.0),
            remove_on_defeat: bool_attribute(scenario, "remove_from_carryover_on_defeat", true),
        }
    }

    pub(crate) fn override_with(&mut self, settings: GoldOverride) {
        if let Some(value) = settings.percentage { self.percentage = value; }
        if let Some(value) = settings.add { self.add = value; }
        if let Some(value) = settings.bonus { self.bonus = value; }
    }
}

impl FixedMatch<'_> {
    /// Settle only when a player continues a won campaign scenario.
    pub fn store_campaign_gold(&self, campaign: &mut CampaignGold) {
        if self.next_scenario_after_victory().is_none() { return; }
        let map = self.scenario.board.map();
        let villages = (0..map.width()).flat_map(|x| (0..map.height()).map(move |y| Hex { x, y }))
            .filter(|&hex| map.terrain_at(hex).is_some_and(|code| self.rules.terrain.features(code).village))
            .count() as i64;
        let turns_left = self.scenario.last_round.map_or(0, |limit| limit.saturating_sub(self.current_round).max(0));
        for (side, treasury) in self.scenario.sides.iter().zip(&self.scenario.treasuries) {
            let Some(id) = &treasury.save_id else { continue };
            if treasury.remove_on_defeat && !self.side_survives(side.side) { continue; }
            let per_turn = villages * i64::from(side.village_gold) + i64::from(side.income_bonus) + i64::from(DEFAULT_BASE_INCOME);
            let early = (treasury.bonus * per_turn as f64 * f64::from(turns_left)) as i64;
            let scaled = (i128::from(self.gold[side.side - 1]) + i128::from(early)) * i128::from(treasury.percentage);
            let retained = (scaled + scaled.signum() * 50) / 100;
            campaign.0.insert(id.clone(), (retained.clamp(i64::MIN.into(), i64::MAX.into()) as i64, treasury.add));
        }
    }
}
