use std::ops::RangeInclusive;

use crate::map::ScenarioMap;
use crate::time_of_day::{TimeOfDay, TimeSchedule, WmlRectangle};
use crate::wml::Node;

/// Reads the global time-of-day schedule of a preprocessed scenario.
pub fn read_time_schedule(scenario: &Node) -> TimeSchedule {
    let turn_at = integer(scenario, "turn_at").unwrap_or(1);
    let current_time = integer(scenario, "current_time").unwrap_or(0);
    let schedule = TimeSchedule::new(time_entries(scenario), turn_at, current_time);
    match integer(scenario, "liminal_bonus") {
        Some(bonus) => schedule.with_liminal_bonus(bonus),
        None => schedule,
    }
}

pub(crate) fn read_map_schedule(scenario: &Node, map: &ScenarioMap) -> TimeSchedule {
    let mut schedule = read_time_schedule(scenario);
    let turn_at = integer(scenario, "turn_at").unwrap_or(1);
    for (_, area) in scenario.children.iter().filter(|(tag, _)| tag == "time_area") {
        let locations = location_ranges(
            area.attribute("x").unwrap_or_default(), area.attribute("y").unwrap_or_default(), map,
        );
        let current_time = integer(area, "current_time").unwrap_or(0);
        schedule.add_area(locations, time_entries(area), turn_at, current_time);
    }
    schedule
}

fn time_entries(node: &Node) -> Vec<TimeOfDay> {
    node.children
        .iter()
        .filter(|(tag, _)| tag == "time")
        .map(|(_, time)| TimeOfDay {
            id: time.attribute("id").unwrap_or_default().to_owned(),
            lawful_bonus: integer(time, "lawful_bonus").unwrap_or(0),
        })
        .collect()
}

pub(crate) fn location_ranges(x: &str, y: &str, map: &ScenarioMap) -> Vec<WmlRectangle> {
    coordinate_ranges(x, y, map.width() + 1, map.height() + 1)
}

pub(crate) fn coordinate_ranges(x: &str, y: &str, max_x: i32, max_y: i32) -> Vec<WmlRectangle> {
    fn values(text: &str) -> Vec<&str> {
        text.split(',').map(str::trim).filter(|value| !value.is_empty()).collect()
    }
    let xs = values(x);
    let ys = values(y);
    (0..xs.len().max(ys.len())).map(|i| (
        axis_range(xs.get(i).copied(), max_x),
        axis_range(ys.get(i).copied(), max_y),
    )).collect()
}

fn axis_range(text: Option<&str>, max: i32) -> RangeInclusive<i32> {
    // The outermost WML map coordinates are 0 and width/height + 1.
    let Some(text) = text else { return 0..=max };
    let separator = text.char_indices().skip(1).find(|&(i, ch)| ch == '-' && i + 1 < text.len());
    let (start, end) = if let Some((i, _)) = separator {
        let (left, right) = (&text[..i], &text[i + 1..]);
        let start = if left == "-infinity" { i32::MIN } else { decimal(left).unwrap_or(0) };
        let end = if right == "infinity" { i32::MAX } else { decimal(right).unwrap_or(0).max(start) };
        (start, end)
    } else {
        let value = decimal(text).unwrap_or(0);
        (value, value)
    };
    start.max(0)..=end.min(max)
}

pub(crate) fn integer(node: &Node, key: &str) -> Option<i32> {
    let value = node.attributes.get(key)?;
    if !value.translations.is_empty() {
        return None;
    }

    decimal(&value.text)
}

// Native conversion of string attributes reads the leading decimal integer.
fn decimal(text: &str) -> Option<i32> {
    let text = text.trim_ascii_start();
    let sign = usize::from(text.starts_with(['+', '-']));
    let digits = text.as_bytes()[sign..]
        .iter()
        .take_while(|byte| byte.is_ascii_digit())
        .count();
    text[..sign + digits].parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time_of_day::Alignment;
    use crate::wml;

    #[test]
    fn direct_time_entries_cycle_in_document_order_from_the_saved_phase() {
        let scenario = wml::parse(
            r#"
            turn_at=9
            current_time=2
            [time]
                id=dawn
            [/time]
            [time_area]
                [time]
                    id=cave
                    lawful_bonus=-50
                [/time]
            [/time_area]
            [time]
                id=morning
                lawful_bonus=25
            [/time]
            [time]
                id=night
                lawful_bonus=-25
            [/time]
            "#,
        )
        .unwrap();
        let schedule = read_time_schedule(&scenario);
        let slots: Vec<_> = (8..=11)
            .map(|turn| {
                let time = schedule.at_turn(turn);
                (time.id.as_str(), time.lawful_bonus)
            })
            .collect();
        assert_eq!(
            slots,
            [("morning", 25), ("night", -25), ("dawn", 0), ("morning", 25)]
        );
    }

    #[test]
    fn scenario_liminal_override_accepts_zero_and_preserves_the_default_on_invalid_input() {
        for (setting, expected) in [("0", 0), ("", 25), ("broken", 25)] {
            let scenario = wml::parse(format!("liminal_bonus={setting}\n")).unwrap();
            let schedule = read_time_schedule(&scenario);
            assert_eq!(schedule.at_turn(1).id, "nulltod");
            assert_eq!(schedule.combat_modifier(0, Alignment::Liminal, false), expected);
        }
    }

    #[test]
    fn missing_phase_starts_at_first_time_on_turn_one() {
        let scenario = wml::parse(
            "[time]\nid=morning\nlawful_bonus=\" 25 bonus\"\n[/time]\n\
             [time]\nid=night\nlawful_bonus=_ \"-25\"\n[/time]\n",
        )
        .unwrap();
        let schedule = read_time_schedule(&scenario);
        assert_eq!(schedule.at_turn(1).lawful_bonus, 25);
        assert_eq!(schedule.at_turn(2).lawful_bonus, 0);
    }

    #[test]
    fn areas_pair_ranges_override_in_order_and_keep_their_own_phase() {
        use crate::hex::Hex;
        let map = ScenarioMap::parse(
            "Xv,Xv,Xv,Xv,Xv,Xv,Xv\nXv,Gg,Gg,Gg,Gg,Gg,Xv\nXv,Gg,Gg,Gg,Gg,Gg,Xv\nXv,Xv,Xv,Xv,Xv,Xv,Xv"
        ).unwrap();
        let scenario = wml::parse(r#"
            turn_at=6
            [time]
                id=outside
            [/time]
            [time_area]
                x=1-2,4
                y=2
                current_time=1
                [time]
                    id=dawn
                [/time]
                [time]
                    id=day
                [/time]
            [/time_area]
            [time_area]
                x=2-3
                y=2
                [time]
                    id=cave
                [/time]
            [/time_area]
            [time_area]
                x=2
                y=2
            [/time_area]
        "#).unwrap();
        let schedule = read_map_schedule(&scenario, &map);
        let time = |turn, x, y| schedule.at_hex(turn, Hex::from_wml(x, y).unwrap()).id.as_str();
        assert_eq!(time(6, 1, 1), "outside");
        assert_eq!(time(6, 1, 2), "day");
        assert_eq!(time(7, 1, 2), "dawn");
        assert_eq!(time(6, 2, 2), "cave");
        assert_eq!(time(6, 3, 2), "cave");
        assert_eq!(time(6, 4, 1), "day");
        assert_eq!(time(6, 4, 2), "day");
        assert_eq!(time(6, 5, 2), "outside");
    }
}
