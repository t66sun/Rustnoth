//! Referee-side [have_unit] checks at the point a campaign branch is executed.

use std::ops::RangeInclusive;

use super::UnitFilter;
use crate::{board::{Battlefield, UnitState}, hex::Hex, wml::Node};

#[derive(Clone, Debug)]
pub(super) struct HaveUnit {
    filter: UnitFilter,
    exclude: Vec<UnitFilter>,
    counts: Vec<RangeInclusive<usize>>,
    search_recall: bool,
    recall_only: bool,
}

impl HaveUnit {
    pub(super) fn read(cfg: &Node) -> Option<Self> {
        if cfg.attributes.values().any(|v| !v.translations.is_empty() || v.text.contains('$')) { return None; }
        let search_recall = match cfg.attribute("search_recall_list") {
            None | Some("no" | "false" | "0") => false,
            Some("yes" | "true" | "1") => true,
            _ => return None,
        };
        let counts = cfg.attribute("count").unwrap_or("1-infinity").split(',').map(|part| {
            let (min, max) = part.trim().split_once('-').unwrap_or((part, part));
            let min: usize = min.trim().parse().ok()?;
            let max = match max.trim() {
                "infinity" => usize::MAX,
                n => n.parse().ok()?,
            };
            Some(min..=max.max(min))
        }).collect::<Option<Vec<_>>>()?;
        let mut unit = cfg.clone();
        unit.attributes.remove("count");
        unit.attributes.remove("search_recall_list");
        let recall_only = unit.attribute("x") == Some("recall") && unit.attribute("y") == Some("recall");
        if recall_only {
            unit.attributes.remove("x");
            unit.attributes.remove("y");
        }
        let exclude = std::mem::take(&mut unit.children).iter().map(|(tag, child)| {
            if tag == "not" { UnitFilter::read(child) } else { None }
        }).collect::<Option<_>>()?;
        Some(Self { filter: UnitFilter::read(&unit)?, counts, search_recall, recall_only, exclude })
    }

    fn matches_unit(&self, position: Option<Hex>, unit: &UnitState<'_>) -> bool {
        let matches = |filter: &UnitFilter| filter.matches_fields(position, unit.side, unit.id.as_deref(), &unit.unit_type.id);
        matches(&self.filter) && !self.exclude.iter().any(matches)
    }

    pub(super) fn matches(&self, board: &Battlefield<'_>, side_count: usize) -> bool {
        let living = board.units().filter(|(hex, unit)| !self.recall_only && unit.hitpoints > 0
            && self.matches_unit(Some(*hex), unit));
        let reserves = (1..=side_count).filter(|_| self.search_recall)
            .flat_map(|side| board.recall_roster(side)).filter(|unit| self.matches_unit(None, unit));
        let mut matches = living.map(|_| ()).chain(reserves.map(|_| ()));
        if self.counts.len() == 1 && self.counts[0] == (1..=usize::MAX) { return matches.next().is_some(); }
        let count = matches.count();
        self.counts.iter().any(|range| range.contains(&count))
    }
}

#[cfg(test)]
mod tests {
    use crate::{fixed_match::Action, scenario_bundle::Bundle};

    #[test]
    fn branch_counts_live_matches_after_placement_and_searches_recall_when_requested() {
        let source = br#"[units]
[unit_type]
id=Guard
hitpoints=24
[/unit_type]
[/units]
[scenario]
map_data="Xv,Xv,Xv,Xv,Xv
Xv,1 Gg,Gg,2 Gg,Xv
Xv,Xv,Xv,Xv,Xv"
[side]
id=Leader
type=Guard
[unit]
id=Veteran
type=Guard
placement=recall
[/unit]
[/side]
[side]
type=Guard
[/side]
[event]
name=start
[unit]
id=New
type=Guard
side=1
x,y=2,1
[/unit]
[if]
[have_unit]
side=1
type=Guard
count=2,4-infinity
[/have_unit]
[have_unit]
side=1
id=Veteran
count=0
[/have_unit]
[have_unit]
id=Veteran
search_recall_list=yes
x,y=recall,recall
[/have_unit]
[have_unit]
x,y=2,1
[not]
id=Leader
[/not]
[/have_unit]
[then]
[set_variable]
name=flag
value=present
[/set_variable]
[/then]
[/if]
[if]
[variable]
name=flag
equals=present
[/variable]
[then]
[gold]
side=1
amount=7
[/gold]
[/then]
[/if]
[/event]
[event]
name=side 2 turn
[if]
[have_unit]
side=1
search_recall_list=yes
count=2
[/have_unit]
[then]
[gold]
side=1
amount=98
[/gold]
[/then]
[else]
[gold]
side=1
amount=3
[/gold]
[/else]
[/if]
[/event]
[event]
name=side 2 turn
[if]
[have_unit]
formula=unsupported
[/have_unit]
[then]
[gold]
side=1
amount=100
[/gold]
[/then]
[/if]
[/event]
[/scenario]"#;
        let data = Bundle::parse(source).unwrap();
        let mut game = data.start(1, |_| unreachable!()).unwrap();
        assert_eq!(game.audit_snapshot().gold[0], 107);
        game.step(Action::EndTurn).unwrap();
        assert_eq!(game.audit_snapshot().gold[0], 110);
    }
}
