# Working rules

This repository trains a Wesnoth 1.18.8 agent with a Rust rules core (`engine_lite/`), a Python coordinator (`learning/`), and shared Lua legality checks (`lua/`). Rules below bind every agent and human change.

## Hard rules

1. **No source file exceeds 3000 lines.** Applies to tracked `.py`, `.rs`, `.lua` and `.sh`. `git ls-files '*.py' '*.rs' '*.lua' '*.sh' | xargs wc -l | awk '$1>3000 && $2!="total"'` must print nothing. Split approaching files along their existing seams into modules of the owning package; never raise the limit.

2. **One pull request changes one behavior.** About 3000 implementation lines, excluding tests, fixtures, documentation and byte-identical moves. It touches only the files that behavior owns, keeps restructuring separate from tuning, and states the changed behavior in the title and each commit message.

3. **No AI attribution.** Commits, pull requests, issues, review comments and generated artifacts carry the user's authorship only: no agent names, no co-author trailers, no "generated with" lines.

4. **Every behavior change carries an assertion that fails when reverted.** No silent skips, no lowered or bypassed gates, no retries that hide flakiness, no weakened evidence. A check that cannot run is reported as not run, never as passed.

5. **Never claim more acceptance than observed.** Minimal checks prove wiring only; Ubuntu is the acceptance environment and Windows the reference. Convergence, strength and full native semantics need formal training evidence. A plan or a TEST.md item never authorizes collection or training.

6. **Referee truth never reaches a policy.** Snapshots, unseen units, unexplored terrain, RNG draws and opponent weights stay inside the referee, replay audit, reward computation and differential tests; observations, candidates, teacher queries and predictions see only legal visible context.

## Data and artifacts

Keep all data and generated artifacts outside Git, including input fixtures, captures, dated verification evidence, run logs, model weights, and delivery output. Use the ignored local directories `data/`, `verification/`, `runs/`, and `delivery/` when storing these files in the checkout. Track the executable checks and instructions needed to reproduce them.

## Documentation

Keep project documentation in `docs/`, with `README.md` and `AGENTS.md` at the repository root. The README's Chinese mirror is `docs/README_zh.md`. Use the `_zh` suffix for mirrors. Read `docs/PLAN.md` when selecting milestone work.

## CodeGraph

When `.codegraph/` exists, use `codegraph_explore` or `codegraph explore` before grep/code reads to locate symbols and trace callers; otherwise skip.

## Git workflow

Make every change on a working branch and integrate it into `main` through a pull request. Each subagent works on its own branch and, when its task is done, opens a pull request with `gh pr create --repo t66sun/Rustnoth --body-file`.

Make sure that every change that actually matters is committed. Every commit needs to be carried along with specific description of what is changed.

At the end of each phase the main agent collects the open pull requests, reviews and verifies them one at a time, and merges each through GitHub into `main`.

## Issue tracker

Requests/specs: GitHub Issues, `t66sun/Rustnoth`. Use `gh ... --repo t66sun/Rustnoth` or `gh api repos/t66sun/Rustnoth/...`; fetch comments/labels and pass multiline bodies via `--body-file`. Publishing means an issue, not a PR.

Triage labels: `needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`.

Keep one `wayfinder:map` with Notes, Decisions-so-far, Fog. Link children natively or by task list plus `Part of #N`; label `wayfinder:{research,prototype,grilling,task}`. Record dependencies natively or `Blocked by: #N`. Assign the first open, unassigned, unblocked child in map order to the driving developer. On resolution, comment, close, and link the finding in the map's Decisions-so-far.
