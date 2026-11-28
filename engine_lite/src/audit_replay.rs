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
    write_steps(bundle.start(seed, read_map_file)?, actions, seed, None, false, out, |_, _| Ok(())).map(|_| ())
}

/// Replays the native RNG at each attack and any supplied recruit checkpoint.
pub fn run_with_native_draws(
    bundle: &Bundle,
    actions: &[Action],
    seed: u32,
    read_map_file: impl FnOnce(&str) -> io::Result<String>,
    out: impl io::Write,
    native_draws: &BTreeMap<usize, AttackDraws>,
) -> Result<(), Box<dyn Error>> {
    run_with_capture(bundle, actions, seed, read_map_file, out, native_draws, None)
}

pub fn run_with_capture(
    bundle: &Bundle, actions: &[Action], seed: u32, read_map_file: impl FnOnce(&str) -> io::Result<String>,
    out: impl io::Write, native_draws: &BTreeMap<usize, AttackDraws>, opening: Option<&crate::native_draws::Opening>,
) -> Result<(), Box<dyn Error>> {
    let opening_seed = opening.and_then(|o| o.draws.checkpoint).map_or(seed, |(seed, _)| seed);
    let game = bundle.start(opening_seed, read_map_file)?;
    replay_match(game, actions, seed, out, native_draws, opening, false, |_, _| Ok(())).map(|_| ())
}

/// Audit an already started match, including a campaign with its actual incoming veterans.
/// The resulting referee can settle the victory and pass its survivors to the next scenario.
pub fn replay_match<'rules>(
    game: FixedMatch<'rules>, actions: &[Action], seed: u32, out: impl io::Write,
    native_draws: &BTreeMap<usize, AttackDraws>, opening: Option<&crate::native_draws::Opening>, all_sides_visibility: bool,
    inspect: impl FnMut(usize, &FixedMatch<'_>) -> Result<(), Box<dyn Error>>,
) -> Result<FixedMatch<'rules>, Box<dyn Error>> {
    for &step in native_draws.keys() {
        if !step
            .checked_sub(1)
            .and_then(|index| actions.get(index))
            .is_some_and(|action| matches!(action, Action::Attack(_) | Action::Recruit { .. } | Action::Move { .. }))
        {
            return Err(format!("unexpected native RNG record at step {step}").into());
        }
    }
    if let Some(opening) = opening {
        let draws = &opening.draws;
        let (opening_seed, consumed) = game.audit_rng_checkpoint();
        validate_words(0, "opening", draws, opening_seed, 0)?;
        if draws.checkpoint.is_some_and(|(_, calls)| calls != 0) {
            return Err("native opening RNG did not start at call one".into());
        }
        if consumed as usize != draws.words.len() {
            return Err(format!("step 0: Rust used {consumed} opening draws; original logged {}", draws.words.len()).into());
        }
        if game.audit_unit_genders() != opening.genders { return Err("step 0: original and Rust unit genders differ".into()); }
    }
    write_steps(game, actions, seed, Some(native_draws), all_sides_visibility, out, inspect)
}

fn write_steps<'rules>(
    mut game: FixedMatch<'rules>,
    actions: &[Action],
    seed: u32,
    native_draws: Option<&BTreeMap<usize, AttackDraws>>,
    all_sides_visibility: bool,
    mut out: impl io::Write,
    mut inspect: impl FnMut(usize, &FixedMatch<'_>) -> Result<(), Box<dyn Error>>,
) -> Result<FixedMatch<'rules>, Box<dyn Error>> {
    write_snapshot(&mut out, 0, &game.audit_snapshot_with_visibility(all_sides_visibility))?;
    inspect(0, &game)?;
    for (index, action) in actions.iter().enumerate() {
        let step = index + 1;
        let recorded = native_draws.and_then(|draws| draws.get(&step));
        if native_draws.is_some() && matches!(action, Action::Attack(_)) && recorded.is_none() {
            return Err(format!("native attack RNG is missing at step {step}").into());
        }
        let consumption = if let Some(recorded) = recorded {
            let kind = match action {
                Action::Attack(_) => "combat",
                Action::Recruit { .. } => "recruit trait",
                Action::Move { .. } => "movement event",
                _ => unreachable!("checked the native RNG step before replay"),
            };
            let (recorded_seed, calls) = recorded.checkpoint.unwrap_or((seed, 0));
            validate_words(step, kind, recorded, recorded_seed, calls)?;
            let (_, consumed) = game
                .step_with_rng_checkpoint(action.clone(), recorded_seed, calls)
                .map_err(|cause| RejectedStep { step, cause })?;
            Some((kind, consumed as usize, recorded.words.len()))
        } else {
            let before = game.audit_rng_checkpoint();
            game.step(action.clone()).map_err(|cause| RejectedStep { step, cause })?;
            if native_draws.is_some() && matches!(action, Action::Move { .. }) && before != game.audit_rng_checkpoint() {
                return Err(format!("native movement RNG is missing at step {step}").into());
            }
            None
        };
        write_snapshot(&mut out, step, &game.audit_snapshot_with_visibility(all_sides_visibility))?;
        inspect(step, &game)?;
        if let Some((kind, consumed, expected)) = consumption
            && consumed != expected
        {
            return Err(format!(
                "step {step}: Rust used {consumed} {kind} draws; original used {expected}"
            )
            .into());
        }
    }
    Ok(game)
}

fn validate_words(
    step: usize,
    kind: &str,
    recorded: &AttackDraws,
    seed: u32,
    calls: u32,
) -> Result<(), String> {
    if recorded.words.is_empty() {
        return Ok(());
    }
    if recorded.checkpoint.is_none() {
        return Err(format!("step {step}: native draws have no RNG checkpoint"));
    }
    let mut rng = ReplayRng::restore(seed, calls);
    for (index, &native) in recorded.words.iter().enumerate() {
        let rust = rng.next_u32();
        if rust != native {
            return Err(format!(
                "step {step}, {kind} draw {}: Rust RNG produced {rust}; original logged {native}",
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
    fn a_move_replays_native_trait_draws_for_its_scripted_reinforcement() {
        let source = "[terrain_type]\nstring=Gg\nid=flat\n[/terrain_type]\n[units]\n\
            [movetype]\nname=foot\n[movement_costs]\nflat=1\n[/movement_costs]\n[/movetype]\n\
            [unit_type]\nid=Guard\nmovement_type=foot\nmovement=5\nhitpoints=30\nnum_traits=2\n[/unit_type]\n\
            [trait]\nid=quick\n[/trait]\n[trait]\nid=strong\n[/trait]\n[/units]\n\
            [scenario]\nmap_data=\"Xv,Xv,Xv,Xv,Xv,Xv\nXv,1 Gg,Gg,Gg,2 Gg,Xv\nXv,Xv,Xv,Xv,Xv,Xv\"\n\
            [side]\ntype=Guard\n[/side]\n[side]\ntype=Guard\n[/side]\n[event]\nname=moveto\n\
            [unit]\ntype=Guard\nid=Ufes\nside=1\nx,y=3,1\n[modifications]\n[trait]\nid=loyal\n[/trait]\n[/modifications]\n[/unit]\n[/event]\n[/scenario]";
        let bundle = Bundle::parse(source.as_bytes()).unwrap();
        let actions = [Action::Move { from: Hex::from_wml(1, 1).unwrap(), to: Hex::from_wml(2, 1).unwrap() }];
        let draws = BTreeMap::from([(1, AttackDraws { checkpoint: Some((5489, 0)), words: vec![3499211612] })]);
        let mut output = Vec::new();
        run_with_native_draws(&bundle, &actions, 7, |_| unreachable!(), &mut output, &draws).unwrap();
        assert!(String::from_utf8(output).unwrap().contains("unit\t3\t1\t1\tGuard\t30\t5\t1\t0\t0\tloyal\tquick"));
        let missing = run_with_native_draws(&bundle, &actions, 5489, |_| unreachable!(), Vec::new(), &BTreeMap::new()).unwrap_err();
        assert!(missing.to_string().contains("movement RNG is missing at step 1"));
    }

    #[test]
    fn opening_replays_the_recorded_seed_and_rejects_missing_or_changed_words() {
        let bundle = Bundle::parse(br#"[units]
[unit_type]
id=Guard
gender=male,female
[/unit_type]
[/units]
[scenario]
map_data="Xv,Xv,Xv,Xv
Xv,1 Gg,2 Gg,Xv
Xv,Xv,Xv,Xv"
[side]
type=Guard
random_gender=yes
[/side]
[side]
type=Guard
[/side]
[/scenario]"#).unwrap();
        let replay = |log: &str| {
            let opening = crate::native_draws::parse_opening(log).unwrap().unwrap();
            run_with_capture(&bundle, &[], 99, |_| unreachable!(), io::sink(), &BTreeMap::new(), Some(&opening))
        };
        let ending = "info wml: RUSTNOTH_AUDIT_OPENING_UNIT 1 1 male\ninfo wml: RUSTNOTH_AUDIT_OPENING_UNIT 2 1 male\ninfo wml: RUSTNOTH_AUDIT_OPENING_END";
        let log = format!("debug random: pulled user random 3499211612 for call 1 with seed 1571\n{ending}");
        replay(&log).unwrap();
        assert!(replay(ending).unwrap_err().to_string().contains("Rust used 1 opening draws; original logged 0"));
        assert!(replay(&log.replace("3499211612", "22")).unwrap_err().to_string().contains("opening draw 1"));
        assert!(replay(&log.replace("1 1 male", "1 1 female")).unwrap_err().to_string().contains("unit genders differ"));
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
