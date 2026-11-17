# Rustnoth

[简体中文](docs/README_zh.md)

Rustnoth aims to train an agent for The Battle for Wesnoth 1.18.8. The planned components are a Rust rules core (`engine_lite/`), a Python coordinator (`learning/`), and shared Lua legality checks (`lua/`).

The referee owns the full game state. Policies receive only the information and legal actions available to their side.

- [Milestone plan](docs/PLAN.md)
- [Working rules](AGENTS.md)
- [License](LICENSE)
