# 逐步裁判对拍

使用原版 Wesnoth 1.18.8 可执行文件、固定场景 WML 和动作脚本。自带规则的场景使用与无界面命令相同的包格式。`map_file` 相对于包所在目录解析。对拍工具将原版裁判限制在本次运行目录的独立用户配置中。

```sh
cargo run --manifest-path engine_lite/Cargo.toml --bin rustnoth-parity -- <wesnoth-executable> <bundle.cfg> <actions.tsv> <seed> <new-run-dir>
```

使用原版单位和地形时，用对拍所用的同一份 Wesnoth 1.18.8 安装预处理核心配置。

```sh
wesnoth --preprocess-defines=SKIP_CORE,NO_TERRAIN_GFX --preprocess "<install>/data/core/_main.cfg" "<out>"
```

在对拍命令末尾加上 `--core-config "<out>/_main.cfg"`。场景包可只包含 `[scenario]`，也可附带自定义单位和地形。Rust 从预处理文件读取原版规则，原版裁判从自己的安装读取内置规则。

动作文件为 UTF-8，每行字段用制表符分隔。坐标从 1 起算，武器索引从 0 起算。以下是动作格式示例。

```text
move	1	2	2	2
attack	2	2	3	2	0
recruit	4	2	Spearman
end_turn
```

第 0 帧是开局，以后每个动作产生一帧。比较覆盖回合、行动方、双方金币、单位状态（含战斗经验）和村庄归属。命令在首处差异退出，运行目录保留 `rust.tsv`、`native.stdout` 和 `native.stderr`。这些文件用于裁判审计。

当前对拍覆盖 Rust 裁判支持的固定双阵营场景。原版先执行脚本并记录每次攻击的种子、计数和随机数。Rust 从这些检查点回放战斗，校验双方使用的随机数序列和次数，然后比较每步状态。命令行种子提供给双方初始化，原版同步战斗自行选择的种子以原版记录为准。原版启动或执行失败时，运行目录保留原版日志。
