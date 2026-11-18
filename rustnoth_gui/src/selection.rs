use engine_lite::{fixed_match::Action, hex::Hex, observation::Observation};

/// The active side's selected unit on the board.
#[derive(Debug, Default)]
pub struct Selection {
    unit: Option<Hex>,
}

impl Selection {
    pub fn unit(&self) -> Option<Hex> {
        self.unit
    }

    /// Selects a friendly unit without disturbing the selection on an empty or enemy hex.
    pub fn select_own(&mut self, position: Hex, observation: &Observation) -> bool {
        let is_own_unit = observation
            .units
            .iter()
            .any(|unit| unit.position == position && unit.side == observation.side);
        if is_own_unit {
            self.unit = Some(position);
        }
        is_own_unit
    }

    pub fn clear(&mut self) {
        self.unit = None;
    }

    /// Returns the referee's choices for this hex in their original order.
    pub fn actions_at<'a>(
        &self,
        position: Hex,
        actions: &'a [Action],
    ) -> impl Iterator<Item = &'a Action> + use<'a> {
        let selected = self.unit;
        actions.iter().filter(move |action| match action {
            Action::Move { from, to } => selected == Some(*from) && *to == position,
            Action::Attack(attack) => selected == Some(attack.from) && attack.to == position,
            Action::Recruit { to, .. } | Action::Recall { to, .. } => selected.is_none() && *to == position,
            Action::Disband { .. } | Action::EndTurn => false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_lite::{board::AttackChoice, observation::ObservedUnit};

    #[test]
    fn selects_only_own_units_and_keeps_all_legal_choices_for_the_hex() {
        let own = Hex { x: 0, y: 0 };
        let ally = Hex { x: 0, y: 1 };
        let enemy = Hex { x: 1, y: 0 };
        let destination = Hex { x: 1, y: 1 };
        let keep = Hex { x: 0, y: 2 };
        let unit = |position, side| ObservedUnit {
                facing: None,
                can_recruit: false, emits_zoc: true,
                max_hitpoints: 10, full_action_points: (side == 1).then_some(true),
            poisoned: false,
            slowed: false,
            petrified: false,
            position,
            side,
            type_id: "Fighter".to_owned(),
            hitpoints: 10,
            experience: 0,
            max_experience: 40,
            moves_left: (side == 1).then_some(1),
            attacks_left: (side == 1).then_some(1),
        };
        let observation = Observation {
            side: 1,
            allied_sides: vec![1],
            gold: Some(20),
            width: 2,
            height: 3,
            tiles: Vec::new(),
            units: vec![unit(own, 1), unit(ally, 1), unit(enemy, 2)],
        };
        let attack = |from, weapon| Action::Attack(AttackChoice { from, to: enemy, weapon });
        let recruit = |unit_type: &str| Action::Recruit { unit_type: unit_type.to_owned(), to: keep };
        let actions = [
            attack(own, 0),
            attack(ally, 0),
            attack(own, 1),
            Action::Move { from: own, to: destination },
            Action::Move { from: ally, to: destination },
            recruit("Bowman"),
            recruit("Spearman"),
            Action::EndTurn,
        ];
        let mut selection = Selection::default();

        assert!(!selection.select_own(enemy, &observation));
        assert_eq!(selection.unit(), None);
        assert!(selection.select_own(own, &observation));
        assert!(!selection.select_own(destination, &observation));
        assert!(!selection.select_own(enemy, &observation));
        assert_eq!(selection.unit(), Some(own));
        assert_eq!(selection.actions_at(enemy, &actions).collect::<Vec<_>>(), [&actions[0], &actions[2]]);
        assert_eq!(selection.actions_at(destination, &actions).collect::<Vec<_>>(), [&actions[3]]);
        assert!(selection.actions_at(keep, &actions).next().is_none());

        selection.clear();
        assert_eq!(selection.unit(), None);
        assert_eq!(selection.actions_at(keep, &actions).collect::<Vec<_>>(), [&actions[5], &actions[6]]);
        assert!(selection.actions_at(enemy, &actions).next().is_none());
    }
}
