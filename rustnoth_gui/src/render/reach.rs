use engine_lite::{fixed_match::Action, hex::Hex};
use std::collections::HashSet;

/// A selected unit always occupies a reachable hex. Only the current attack
/// target is exempt from the dark mask on an otherwise unreachable enemy tile.
pub(super) fn reachable(selected: Option<Hex>, actions: &[Action], target: Option<Hex>) -> Option<HashSet<Hex>> {
    let origin = selected?;
    let mut reachable = HashSet::from([origin]);
    for action in actions {
        match action {
            Action::Move { from, to } if *from == origin => { reachable.insert(*to); }
            Action::Attack(attack) if attack.from == origin && Some(attack.to) == target => {
                reachable.insert(attack.to);
            }
            _ => {}
        }
    }
    Some(reachable)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_selected_unit_exposes_its_destination_range() {
        let start = Hex { x: 1, y: 2 };
        let goal = Hex { x: 2, y: 2 };
        let other = Hex { x: 8, y: 3 };
        let actions = [Action::Move { from: start, to: goal }, Action::Move { from: other, to: start }];
        assert_eq!(reachable(None, &actions, None), None);
        assert_eq!(reachable(Some(start), &actions, None), Some(HashSet::from([start, goal])));
    }
}
