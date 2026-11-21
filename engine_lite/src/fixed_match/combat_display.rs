//! Post-resolution strike playback for a side that could see both participants.
use super::{Action, Attack, AttackChoice, FixedMatch, Hex, MatchError, MatchState};
use crate::{combat::ResolvedBlow, observation::Observation};

pub struct VisibleCombat {
    pub opening: Observation,
    pub participants: [Hex; 2],
    pub weapons: [Option<DisplayWeapon>; 2],
    pub blows: Vec<ResolvedBlow>,
}

pub struct DisplayWeapon {
    pub id: String,
    pub range: String,
}

impl FixedMatch<'_> {
    /// Commit exactly the same action as attack_with_advancements. Return the
    /// observed exchange only after the referee has finished handling the command.
    pub fn attack_for_display(
        &mut self,
        attack: AttackChoice,
        advancement: Option<&str>,
        viewer: usize,
    ) -> Result<(MatchState, Option<VisibleCombat>), MatchError> {
        self.attack_with_action_seed(attack, advancement, Some(viewer), None)
    }

    /// A synchronized single-player attack can carry its own seed in the replay.
    /// Restore the scenario stream before returning, including on a rejected attack.
    pub fn attack_with_action_seed(
        &mut self,
        attack: AttackChoice,
        advancement: Option<&str>,
        viewer: Option<usize>,
        seed: Option<u32>,
    ) -> Result<(MatchState, Option<VisibleCombat>), MatchError> {
        let mut display = None;
        let previous = seed.map(|seed| std::mem::replace(&mut self.rng, crate::rng::ReplayRng::new(seed)));
        let state = self.step_with_choices(
            Action::Attack(attack),
            advancement,
            None,
            viewer.map(|side| (side, &mut display)),
            None,
        );
        if let Some(rng) = previous {
            self.rng = rng;
        }
        state.map(|state| (state, display))
    }

    pub(super) fn visible_combat(&self, viewer: usize, attack: &Attack) -> Option<VisibleCombat> {
        let opening = self.observation_for(viewer)?;
        let participants = [attack.from, attack.to];
        if !participants
            .iter()
            .all(|hex| opening.units.iter().any(|unit| unit.position == *hex))
        {
            return None;
        }
        let weapon = |at: Hex, index: usize| {
            let w = self
                .scenario
                .board
                .unit_at(at)?
                .unit_type
                .attacks
                .get(index)?;
            Some(DisplayWeapon {
                id: w.id.clone(),
                range: w.range.clone(),
            })
        };
        Some(VisibleCombat {
            opening,
            participants,
            weapons: [
                weapon(attack.from, attack.weapon),
                attack
                    .retaliation
                    .as_ref()
                    .and_then(|(index, _)| weapon(attack.to, *index)),
            ],
            blows: vec![],
        })
    }
}
