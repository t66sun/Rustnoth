//! Baseline decisions using only a side's observation and candidate actions.

use crate::{fixed_match::Action, hex::Hex, observation::Observation};

/// Attack a visible enemy, or take the first move that brings one closer.
pub fn attack_or_approach<'a>(observation: &Observation, actions: &'a [Action]) -> Option<&'a Action> {
    let nearest_enemy = |hex: Hex| {
        observation
            .units
            .iter()
            .filter(|unit| !observation.allied_sides.contains(&unit.side))
            .map(|unit| hex.distance(unit.position))
            .min()
    };
    let mut approach = None;
    for action in actions {
        match action {
            Action::Attack(_) => return Some(action),
            Action::Move { from, to } if approach.is_none() => {
                if let (Some(before), Some(after)) = (nearest_enemy(*from), nearest_enemy(*to))
                    && after < before
                {
                    approach = Some(action);
                }
            }
            _ => {}
        }
    }
    approach
}
