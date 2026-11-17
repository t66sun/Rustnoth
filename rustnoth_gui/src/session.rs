use engine_lite::{
    fixed_match::{Action, FixedMatch, MatchError, MatchState},
    observation::Observation,
};

/// The active side's decision and the last viewer's final board for a local match.
pub struct Session<'rules> {
    game: FixedMatch<'rules>,
    observation: Option<Observation>,
    legal_actions: Vec<Action>,
}

impl<'rules> Session<'rules> {
    pub fn new(game: FixedMatch<'rules>) -> Self {
        let (observation, legal_actions) = match game.decision() {
            Some((observation, actions)) => (Some(observation), actions),
            None => (None, Vec::new()),
        };
        Self { game, observation, legal_actions }
    }

    pub fn state(&self) -> MatchState {
        self.game.state()
    }

    pub fn observation(&self) -> Option<&Observation> {
        self.observation.as_ref()
    }

    pub fn legal_actions(&self) -> &[Action] {
        &self.legal_actions
    }

    pub fn apply(&mut self, action: Action) -> Result<MatchState, MatchError> {
        let state = self.game.step(action)?;
        if let Some((observation, actions)) = self.game.decision() {
            self.observation = Some(observation);
            self.legal_actions = actions;
        } else {
            self.observation = self
                .observation
                .as_ref()
                .and_then(|view| self.game.observation_for(view.side));
            self.legal_actions.clear();
        }
        Ok(state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_lite::{board::AttackChoice, hex::Hex, scenario_bundle::Bundle};

    #[test]
    fn follows_the_active_side_and_keeps_its_updated_board_at_the_end() {
        let bundle = Bundle::parse(
            br#"
            [units]
                [unit_type]
                    id=Fighter
                    hitpoints=10
                    [attack]
                        range=melee
                        damage=1
                        number=1
                    [/attack]
                [/unit_type]
                [unit_type]
                    id=Guard
                    hitpoints=10
                    [attack]
                        range=melee
                        damage=10
                        number=1
                    [/attack]
                [/unit_type]
            [/units]
            [scenario]
                turns=1
                map_data="Xv,Xv,Xv,Xv
Xv,1 Gg,2 Gg,Xv
Xv,Xv,Xv,Xv"
                [side]
                    side=1
                    type=Fighter
                    gold=17
                [/side]
                [side]
                    side=2
                    type=Guard
                    gold=29
                [/side]
            [/scenario]
            "#,
        )
        .unwrap();
        let mut session = Session::new(bundle.start(5489, |_| unreachable!()).unwrap());
        let west = Hex { x: 0, y: 0 };
        let east = Hex { x: 1, y: 0 };
        let west_attack = Action::Attack(AttackChoice { from: west, to: east, weapon: 0 });
        let east_attack = Action::Attack(AttackChoice { from: east, to: west, weapon: 0 });

        assert_eq!(session.observation().unwrap().side, 1);
        assert_eq!(session.legal_actions(), [west_attack.clone(), Action::EndTurn]);
        assert!(session.apply(east_attack.clone()).is_err());
        assert_eq!(session.observation().unwrap().side, 1);
        assert_eq!(session.legal_actions(), [west_attack.clone(), Action::EndTurn]);

        session.apply(Action::EndTurn).unwrap();
        assert_eq!(session.observation().unwrap().side, 2);
        assert_eq!(session.legal_actions(), [east_attack, Action::EndTurn]);

        assert_eq!(session.apply(Action::EndTurn).unwrap(), MatchState::Draw);
        assert_eq!(session.state(), MatchState::Draw);
        let view = session.observation().unwrap();
        assert_eq!(view.side, 2);
        assert_eq!(view.units.len(), 2);
        assert!(session.legal_actions().is_empty());

        let mut session = Session::new(bundle.start(5489, |_| unreachable!()).unwrap());
        assert_eq!(session.observation().unwrap().units.len(), 2);
        assert_eq!(session.apply(west_attack).unwrap(), MatchState::Won(2));
        let view = session.observation().unwrap();
        assert_eq!((view.side, view.gold), (1, Some(17)));
        let [survivor] = view.units.as_slice() else {
            panic!("expected only the defending unit to survive");
        };
        assert_eq!((survivor.position, survivor.side, survivor.hitpoints), (east, 2, 9));
        assert_eq!((survivor.moves_left, survivor.attacks_left), (None, None));
        assert!(session.legal_actions().is_empty());
    }
}
