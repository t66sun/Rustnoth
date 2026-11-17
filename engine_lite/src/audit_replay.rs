//! Execute scripted actions and record referee snapshots for differential audits.

use std::{collections::BTreeMap, error::Error, fmt, io};

use crate::{
    audit_trace::write_snapshot,
    fixed_match::{Action, FixedMatch, MatchError},
    native_draws::AttackDraws,
    rng::ReplayRng,
    scenario_bundle::Bundle,
};

/// Write step zero and a new frame after each accepted action.
///
/// If the referee rejects an action, the completed frames remain in the output.
pub fn run(
    bundle: &Bundle,
    actions: &[Action],
    seed: u32,
    read_map_file: impl FnOnce(&str) -> io::Result<String>,
    out: impl io::Write,
) -> Result<(), Box<dyn Error>> {
    write_steps(bundle.start(seed, read_map_file)?, actions, seed, None, out)
}

/// Replays an accepted native script using its observed RNG stream at each attack.
pub fn run_with_native_draws(
    bundle: &Bundle,
    actions: &[Action],
    seed: u32,
    read_map_file: impl FnOnce(&str) -> io::Result<String>,
    out: impl io::Write,
    native_draws: &BTreeMap<usize, AttackDraws>,
) -> Result<(), Box<dyn Error>> {
    for &step in native_draws.keys() {
        if !step
            .checked_sub(1)
            .and_then(|index| actions.get(index))
            .is_some_and(|action| matches!(action, Action::Attack(_)))
        {
            return Err(format!("native attack RNG recorded at non-attack step {step}").into());
        }
    }
    write_steps(bundle.start(seed, read_map_file)?, actions, seed, Some(native_draws), out)
}

fn write_steps(
    mut game: FixedMatch<'_>,
    actions: &[Action],
    seed: u32,
    native_draws: Option<&BTreeMap<usize, AttackDraws>>,
    mut out: impl io::Write,
) -> Result<(), Box<dyn Error>> {
    write_snapshot(&mut out, 0, &game.audit_snapshot())?;
    for (index, action) in actions.iter().enumerate() {
        let step = index + 1;
        let consumption = if let (Some(draws), Action::Attack(_)) = (native_draws, action) {
            let attack = draws
                .get(&step)
                .ok_or_else(|| format!("native attack RNG is missing at step {step}"))?;
            let (attack_seed, calls) = attack.checkpoint.unwrap_or((seed, 0));
            validate_words(step, attack, attack_seed, calls)?;
            let (_, consumed) = game
                .step_with_rng_checkpoint(action.clone(), attack_seed, calls)
                .map_err(|cause| RejectedStep { step, cause })?;
            Some((consumed as usize, attack.words.len()))
        } else {
            game.step(action.clone()).map_err(|cause| RejectedStep { step, cause })?;
            None
        };
        write_snapshot(&mut out, step, &game.audit_snapshot())?;
        if let Some((consumed, expected)) = consumption
            && consumed != expected
        {
            return Err(format!(
                "step {step}: Rust used {consumed} combat draws; original used {expected}"
            )
            .into());
        }
    }
    Ok(())
}

fn validate_words(step: usize, attack: &AttackDraws, seed: u32, calls: u32) -> Result<(), String> {
    if attack.words.is_empty() {
        return Ok(());
    }
    if attack.checkpoint.is_none() {
        return Err(format!("step {step}: native draws have no RNG checkpoint"));
    }
    let mut rng = ReplayRng::restore(seed, calls);
    for (index, &native) in attack.words.iter().enumerate() {
        let rust = rng.next_u32();
        if rust != native {
            return Err(format!(
                "step {step}, combat draw {}: Rust RNG produced {rust}; original logged {native}",
                index + 1
            ));
        }
    }
    Ok(())
}

#[derive(Debug)]
struct RejectedStep {
    step: usize,
    cause: MatchError,
}

impl fmt::Display for RejectedStep {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "script step {} rejected ({})", self.step, self.cause)
    }
}

impl Error for RejectedStep {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.cause)
    }
}

#[cfg(test)]
mod tests {
    use crate::{board::AttackChoice, hex::Hex};

    use super::*;

    fn bundle() -> Bundle {
        Bundle::parse(
            br#"
            [units]
                [unit_type]
                    id=Fighter
                    hitpoints=10
                [/unit_type]
            [/units]
            [scenario]
                turns=1
                map_data="Xv,Xv,Xv,Xv
Xv,1 Kh,2 Kh,Xv
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
        .unwrap()
    }

    #[test]
    fn writes_initial_state_and_the_state_after_end_turn() {
        let mut output = Vec::new();
        run(&bundle(), &[Action::EndTurn], 5489, |_| unreachable!(), &mut output).unwrap();

        let text = String::from_utf8(output).unwrap();
        let headers: Vec<_> = text.lines().filter(|line| line.starts_with("step\t")).collect();
        assert_eq!(headers, ["step\t0\tplaying\t1\t1", "step\t1\tplaying\t1\t2"]);
        assert!(text.ends_with("end\t1\n"));
    }

    #[test]
    fn a_rejected_action_reports_its_step_and_keeps_the_completed_frames() {
        let mut output = Vec::new();
        let error = run(
            &bundle(),
            &[Action::EndTurn, Action::EndTurn, Action::EndTurn],
            5489,
            |_| unreachable!(),
            &mut output,
        )
        .unwrap_err();

        assert!(error.to_string().starts_with("script step 3 rejected"));
        assert!(matches!(
            error.source().and_then(|cause| cause.downcast_ref::<MatchError>()),
            Some(MatchError::Finished)
        ));
        let text = String::from_utf8(output).unwrap();
        assert!(text.lines().any(|line| line == "step\t2\tdraw"));
        assert!(text.ends_with("end\t2\n"));
    }

    #[test]
    fn audited_combat_uses_native_words_and_checks_rng_consumption() {
        let bundle = Bundle::parse(
            br#"
            [terrain_type]
                string=Gg
                id=flat
            [/terrain_type]
            [units]
                [movetype]
                    name=foot
                    [defense]
                        flat=50
                    [/defense]
                [/movetype]
                [unit_type]
                    id=Fighter
                    hitpoints=10
                    movement_type=foot
                    [attack]
                        range=melee
                        damage=5
                        number=1
                    [/attack]
                [/unit_type]
            [/units]
            [scenario]
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
        let actions = [Action::Attack(AttackChoice {
            from: Hex { x: 0, y: 0 },
            to: Hex { x: 1, y: 0 },
            weapon: 0,
        })];
        let mut capture = BTreeMap::from([(
            1,
            AttackDraws { checkpoint: Some((5489, 1)), words: vec![581869302, 3890346734] },
        )]);
        let mut output = Vec::new();
        run_with_native_draws(&bundle, &actions, 5489, |_| unreachable!(), &mut output, &capture)
            .unwrap();
        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("unit\t1\t1\t1\tFighter\t5\t0\t0\t1"));
        assert!(output.contains("unit\t2\t1\t2\tFighter\t5\t1\t1\t1"));

        let words = &mut capture.get_mut(&1).unwrap().words;
        words.pop();
        let error = run_with_native_draws(
            &bundle,
            &actions,
            5489,
            |_| unreachable!(),
            io::sink(),
            &capture,
        )
        .unwrap_err();
        assert_eq!(error.to_string(), "step 1: Rust used 2 combat draws; original used 1");

        capture.get_mut(&1).unwrap().words[0] += 1;
        let error = run_with_native_draws(
            &bundle,
            &actions,
            5489,
            |_| unreachable!(),
            io::sink(),
            &capture,
        )
        .unwrap_err();
        assert!(error.to_string().contains("step 1, combat draw 1"));
    }
}
