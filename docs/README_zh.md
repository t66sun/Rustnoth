# Rustnoth

[English](../README.md)

Rustnoth 的目标是为《韦诺之战》1.18.8 训练对战智能体。Rust 规则核心在 `engine_lite/`。项目还规划了 Python 协调层（`learning/`）和共享的 Lua 合法性检查（`lua/`）。

裁判掌握完整的对局状态。策略只接收己方可见的信息和可用的合法动作。

- [阶段计划](PLAN.md)
- [项目规则](../AGENTS.md)
- [许可条款](../LICENSE)

## 运行固定场景

无界面命令支持全图可见、双方敌对、使用基础单位属性和普通武器的固定对局。输入为已预处理的 WML 文件，包含顶层 `[terrain_type]` 地形定义、一个含已展开单位类型和移动配置的 `[units]` 块，以及一个 `[scenario]` 块。`map_file` 路径以输入文件所在目录为基准。

```sh
cargo run --manifest-path engine_lite/Cargo.toml --bin rustnoth-headless -- bundle.cfg 5489 100
```

最后两个参数分别是随机种子和动作上限。命令输出胜方或平局。如果动作上限先用尽，命令以错误退出。
