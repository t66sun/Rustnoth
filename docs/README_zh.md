# Rustnoth

[English](../README.md)

Rustnoth 的目标是为《韦诺之战》1.18.8 训练对战智能体。Rust 规则核心在 `engine_lite/`。项目还规划了 Python 协调层（`learning/`）和共享的 Lua 合法性检查（`lua/`）。

裁判掌握完整的对局状态。策略只接收己方可见的信息和可用的合法动作。

- [阶段计划](PLAN.md)
- [原版裁判逐步对拍](PARITY.md)
- [桌面版](GUI.md)
- [项目规则](../AGENTS.md)
- [许可条款](../LICENSE)

## 运行固定场景

无界面命令支持两个及以上阵营的全图可见对局。共享 `team_name` 的阵营互为盟友。当存活阵营之间没有敌对关系时，它们共同获胜。输入为已预处理的 WML 文件，包含顶层 `[terrain_type]` 地形定义、一个含已展开单位类型和移动配置的 `[units]` 块，以及一个 `[scenario]` 块。`map_file` 路径以输入文件所在目录为基准。

```sh
cargo run --manifest-path engine_lite/Cargo.toml --bin rustnoth-headless -- bundle.cfg 5489 100
```

示例中的随机种子为 `5489`，动作上限为 `100`。命令输出胜方或平局。如果动作上限先用尽，命令以错误退出。

使用原版单位和地形时，在命令末尾加上 `--core-config path/to/preprocessed-core.cfg`。场景包可只包含 `[scenario]`，也可附带自定义单位和地形。预处理命令见[原版裁判逐步对拍](PARITY.md)。
