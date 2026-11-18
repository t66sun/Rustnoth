use crate::{
    scenario_time::{coordinate_ranges, time_entries},
    time_of_day::{TimeOfDay, TimeSchedule, WmlRectangle},
    wml::Node,
};

#[derive(Clone, Debug)]
pub(crate) enum TimeAreaChange {
    Add { id: String, locations: Vec<WmlRectangle>, entries: Vec<TimeOfDay>, phase: i32 },
    Remove(Vec<String>),
}

impl TimeAreaChange {
    pub(super) fn read(tag: &str, cfg: &Node) -> Option<Self> {
        if cfg.attributes.values().any(|v| v.text.contains('$')) { return None; }
        if tag == "remove_time_area" || matches!(cfg.attribute("remove"), Some("yes" | "true")) {
            let ids = cfg.attribute("id")?.split(',').map(str::trim)
                .filter(|id| !id.is_empty()).map(str::to_owned).collect();
            return Some(Self::Remove(ids));
        }
        if cfg.attributes.keys().any(|key| !matches!(key.as_str(), "id" | "x" | "y" | "current_time" | "remove"))
            || cfg.children.iter().any(|(tag, child)| tag != "time" || ["id", "lawful_bonus"].iter().any(|k|
                child.attribute(k).is_some_and(|v| v.contains('$')))) { return None; }
        let entries = time_entries(cfg);
        let phase = cfg.attribute("current_time").unwrap_or("0").trim().parse::<i32>().ok()?;
        if usize::try_from(phase).ok()? >= entries.len() { return None; }
        let mut locations = coordinate_ranges(cfg.attribute("x").unwrap_or(""),
            cfg.attribute("y").unwrap_or(""), i32::MAX, i32::MAX);
        if locations.is_empty() { locations.push((0..=i32::MAX, 0..=i32::MAX)); }
        Some(Self::Add { id: cfg.attribute("id").unwrap_or("").to_owned(), locations, entries, phase })
    }

    pub(crate) fn apply(self, schedule: &mut TimeSchedule, round: i32) {
        match self {
            Self::Add { id, locations, entries, phase } => schedule.add_area(id, locations, entries, round, phase),
            Self::Remove(ids) => schedule.remove_areas(&ids),
        }
    }
}
