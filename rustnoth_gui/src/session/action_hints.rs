use super::{Action, Hex};
use std::collections::HashSet;

/// Action origins for the status orbs and destinations for the selected unit.
#[derive(Default)]
pub struct ActionHints {
    current: bool,
    selected: Option<Hex>,
    origins: HashSet<Hex>,
    moves: HashSet<Hex>,
    attacks: HashSet<Hex>,
}

impl ActionHints {
    pub fn from_actions(selected: Option<Hex>, actions: &[Action]) -> Self {
        let mut hints = Self::default();
        hints.update(selected, actions);
        hints
    }

    pub(super) fn invalidate(&mut self) {
        self.current = false;
    }

    pub(super) fn update<'a>(
        &mut self,
        selected: Option<Hex>,
        actions: impl IntoIterator<Item = &'a Action>,
    ) {
        if self.current && self.selected == selected {
            return;
        }

        self.current = true;
        self.selected = selected;
        self.origins.clear();
        self.moves.clear();
        self.attacks.clear();
        if let Some(origin) = selected {
            self.moves.insert(origin);
        }
        for action in actions {
            match action {
                Action::Move { from, to } => {
                    self.origins.insert(*from);
                    if Some(*from) == selected {
                        self.moves.insert(*to);
                    }
                }
                Action::Attack(attack) => {
                    self.origins.insert(attack.from);
                    if Some(attack.from) == selected {
                        self.attacks.insert(attack.to);
                    }
                }
                _ => {}
            }
        }
    }

    pub fn is_actionable(&self, origin: Hex) -> bool {
        self.origins.contains(&origin)
    }

    pub fn is_reachable(&self, at: Hex) -> bool {
        self.moves.contains(&at)
    }

    pub fn can_attack(&self, target: Hex) -> bool {
        self.attacks.contains(&target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_lite::board::AttackChoice;
    use std::cell::Cell;

    #[test]
    fn reuses_hints_until_actions_or_selection_change() {
        let start = Hex { x: 1, y: 2 };
        let goal = Hex { x: 2, y: 2 };
        let enemy = Hex { x: 3, y: 2 };
        let other = Hex { x: 8, y: 3 };
        let actions = [
            Action::Move { from: start, to: goal },
            Action::Move { from: other, to: start },
            Action::Attack(AttackChoice { from: start, to: enemy, weapon: 0 }),
        ];
        let visited = Cell::new(0);
        let mut hints = ActionHints::default();
        for _ in 0..3 {
            hints.update(Some(start), actions.iter().inspect(|_| visited.set(visited.get() + 1)));
            assert!(hints.is_actionable(start));
            assert!(hints.is_actionable(other));
            assert!(hints.is_reachable(start));
            assert!(hints.is_reachable(goal));
            assert!(!hints.is_reachable(enemy));
            assert!(hints.can_attack(enemy));
        }
        assert_eq!(visited.get(), actions.len());

        hints.update(Some(other), &actions);
        assert!(hints.is_reachable(start));
        assert!(!hints.is_reachable(goal));
        assert!(!hints.can_attack(enemy));

        hints.invalidate();
        hints.update(Some(other), &actions[..1]);
        assert!(!hints.is_actionable(other));
        assert!(!hints.is_reachable(start));
        assert!(hints.is_reachable(other));
    }
}
