use engine_lite::{
    fixed_match::{Action, FixedMatch, MatchError, MatchState},
    observation::Observation,
};

/// The active side's decision for a local match.
pub struct Session<'rules> {
    game: FixedMatch<'rules>,
    decision: Option<(Observation, Vec<Action>)>,
}

impl<'rules> Session<'rules> {
    pub fn new(game: FixedMatch<'rules>) -> Self {
        let decision = game.decision();
        Self { game, decision }
    }

    pub fn state(&self) -> MatchState {
        self.game.state()
    }

    pub fn observation(&self) -> Option<&Observation> {
        self.decision.as_ref().map(|(observation, _)| observation)
    }

    pub fn legal_actions(&self) -> &[Action] {
        self.decision.as_ref().map_or(&[], |(_, actions)| actions.as_slice())
    }

    pub fn apply(&mut self, action: Action) -> Result<MatchState, MatchError> {
        let state = self.game.step(action)?;
        self.decision = self.game.decision();
        Ok(state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_lite::{board::AttackChoice, hex::Hex, scenario_bundle::Bundle};

    #[test]
    fn follows_the_active_side_and_clears_the_decision_at_the_end() {
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
            [/units]
            [scenario]
                turns=1
                map_data="Xv,Xv,Xv,Xv
Xv,1 Gg,2 Gg,Xv
Xv,Xv,Xv,Xv"
                [side]
                    side=1
                    type=Fighter
                [/side]
                [side]
                    side=2
                    type=Fighter
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
        assert_eq!(session.legal_actions(), [west_attack, Action::EndTurn]);

        session.apply(Action::EndTurn).unwrap();
        assert_eq!(session.observation().unwrap().side, 2);
        assert_eq!(session.legal_actions(), [east_attack, Action::EndTurn]);

        assert_eq!(session.apply(Action::EndTurn).unwrap(), MatchState::Draw);
        assert_eq!(session.state(), MatchState::Draw);
        assert!(session.observation().is_none());
        assert!(session.legal_actions().is_empty());
    }
}
