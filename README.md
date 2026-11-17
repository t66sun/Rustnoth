# Rustnoth

[简体中文](docs/README_zh.md)

Rustnoth aims to train an agent for The Battle for Wesnoth 1.18.8. The Rust rules core lives in `engine_lite/`. A Python coordinator (`learning/`) and shared Lua legality checks (`lua/`) are planned.

The referee owns the full game state. Policies receive only the information and legal actions available to their side.

- [Milestone plan](docs/PLAN.md)
- [Original referee comparison](docs/PARITY.md)
- [Working rules](AGENTS.md)
- [License](LICENSE)

## Run a fixed scenario

The headless command runs a fully visible match between two opposing sides using base unit stats and ordinary weapons. Give it a preprocessed WML bundle with top-level `[terrain_type]` definitions, one `[units]` block of flat unit types and movement profiles, and one `[scenario]` block. Paths in `map_file` are relative to the bundle.

```sh
cargo run --manifest-path engine_lite/Cargo.toml --bin rustnoth-headless -- bundle.cfg 5489 100
```

The last two arguments set the random seed and action limit. The command prints the winner or a draw and exits with an error if the action limit expires first.
