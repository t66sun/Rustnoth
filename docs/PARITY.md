# 逐步裁判对拍

使用原版 Wesnoth 1.18.8 可执行文件、固定场景 WML 和动作脚本。WML 与无界面命令使用同一种包格式。`map_file` 相对于包所在目录解析。对拍工具将原版裁判限制在本次运行目录的独立用户配置中。

```sh
cargo run --manifest-path engine_lite/Cargo.toml --bin rustnoth-parity -- <wesnoth-executable> <bundle.cfg> <actions.tsv> <seed> <new-run-dir>
```

动作文件为 UTF-8，每行字段用制表符分隔。坐标从 1 起算，武器索引从 0 起算。以下是动作格式示例。

```text
move	1	2	2	2
attack	2	2	3	2	0
recruit	4	2	Spearman
end_turn
```

第 0 帧是开局，以后每个动作产生一帧。比较覆盖回合、行动方、双方金币、单位状态和村庄归属。命令在首处差异退出，运行目录保留 `rust.tsv`、`native.stdout` 和 `native.stderr`。这些文件用于裁判审计。

当前对拍覆盖 Rust 裁判支持的固定双阵营场景。原版同步战斗可能独立生成种子，检验基础战斗规则时可使用命中结果确定的场景。
