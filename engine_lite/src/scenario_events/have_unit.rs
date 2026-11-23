//! Referee-side [have_unit] checks at the point a campaign branch is executed.

use std::ops::RangeInclusive;

use super::UnitFilter;
use crate::{board::{Battlefield, UnitState}, hex::Hex, wml::Node};

#[derive(Clone, Debug)]
pub(super) struct HaveUnit {
    filter: UnitFilter,
    exclude: Vec<UnitFilter>,
    adjacent: Vec<UnitFilter>,
    counts: Vec<RangeInclusive<usize>>,
    levels: Vec<RangeInclusive<i32>>,
    races: Vec<String>,
    heals: bool,
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
        let races = unit.attributes.remove("race").map_or_else(Vec::new, |v|
            v.text.split(',').map(|r| r.trim().to_owned()).collect());
        let heals = match unit.attributes.remove("ability_type").as_ref().map(|v| v.text.as_str()) {
            Some("heals") => true,
            None => false,
            _ => return None,
        };
        let mut levels = Vec::new();
        if let Some(level) = unit.attributes.remove("level") {
            for part in level.text.split(',').map(str::trim) {
                let (min, max) = part.split_once('-').unwrap_or((part, part));
                levels.push(min.trim().parse().ok()?..=max.trim().parse().ok()?);
            }
        }
        let recall_only = unit.attribute("x") == Some("recall") && unit.attribute("y") == Some("recall");
        if recall_only {
            unit.attributes.remove("x");
            unit.attributes.remove("y");
        }
        let mut exclude = Vec::new();
        let mut adjacent = Vec::new();
        for (tag, child) in std::mem::take(&mut unit.children) {
            match tag.as_str() {
                "not" => exclude.push(UnitFilter::read(&child)?),
                "filter_adjacent" => adjacent.push(UnitFilter::read(&child)?),
                _ => return None,
            }
        }
        Some(Self { filter: UnitFilter::read(&unit)?, counts, levels, races, heals, search_recall, recall_only, exclude, adjacent })
    }

    fn matches_unit(&self, position: Option<Hex>, unit: &UnitState<'_>) -> bool {
        let matches = |filter: &UnitFilter| filter.matches_fields(position, unit.side, unit.id.as_deref(), &unit.unit_type.id);
        matches(&self.filter) && !self.exclude.iter().any(matches)
            && (self.races.is_empty() || self.races.contains(&unit.unit_type.race))
            && (!self.heals || unit.unit_type.has_heals)
            && (self.levels.is_empty() || self.levels.iter().any(|range| range.contains(&unit.unit_type.level)))
    }

    pub(super) fn matches(&self, board: &Battlefield<'_>, side_count: usize) -> bool {
        let living = board.units().filter(|(hex, unit)| !self.recall_only && unit.hitpoints > 0
            && self.matches_unit(Some(*hex), unit) && self.adjacent.iter().all(|filter|
                hex.neighbors().any(|at| board.unit_at(at).is_some_and(|other|
                    filter.matches_fields(Some(at), other.side, other.id.as_deref(), &other.unit_type.id)))));
        let reserves = (1..=side_count).filter(|_| self.search_recall && self.adjacent.is_empty())
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
