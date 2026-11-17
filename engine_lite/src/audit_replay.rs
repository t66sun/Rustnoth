//! Execute scripted actions and record referee snapshots for differential audits.

use std::{error::Error, fmt, io};

use crate::{
    audit_trace::write_snapshot,
    fixed_match::{Action, MatchError},
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
    mut out: impl io::Write,
) -> Result<(), Box<dyn Error>> {
    let mut game = bundle.start(seed, read_map_file)?;
    write_snapshot(&mut out, 0, &game.audit_snapshot())?;
    for (index, action) in actions.iter().enumerate() {
        let step = index + 1;
        game.step(action.clone())
            .map_err(|cause| RejectedStep { step, cause })?;
        write_snapshot(&mut out, step, &game.audit_snapshot())?;
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
}
