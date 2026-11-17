use crate::time_of_day::{TimeOfDay, TimeSchedule};
use crate::wml::Node;

/// Reads the global time-of-day schedule of a preprocessed scenario.
pub fn read_time_schedule(scenario: &Node) -> TimeSchedule {
    let entries = scenario
        .children
        .iter()
        .filter(|(tag, _)| tag == "time")
        .map(|(_, time)| TimeOfDay {
            id: time.attribute("id").unwrap_or_default().to_owned(),
            lawful_bonus: integer(time, "lawful_bonus").unwrap_or(0),
        })
        .collect();
    let turn_at = integer(scenario, "turn_at").unwrap_or(1);
    let current_time = integer(scenario, "current_time").unwrap_or(0);
    let schedule = TimeSchedule::new(entries, turn_at, current_time);
    match integer(scenario, "liminal_bonus") {
        Some(bonus) => schedule.with_liminal_bonus(bonus),
        None => schedule,
    }
}

pub(crate) fn integer(node: &Node, key: &str) -> Option<i32> {
    let value = node.attributes.get(key)?;
    if !value.translations.is_empty() {
        return None;
    }

    // Native conversion of string attributes reads the leading decimal integer.
    let text = value.text.trim_ascii_start();
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
}
