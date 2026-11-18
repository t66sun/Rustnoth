# 逐步裁判对拍

同一个原版战役预处理文件可以包含多个场景。先列出场景 ID，再按 ID 导出场景包。地图目录使用对应战役的 `maps`。导出的包保留场景事件、阵营和战役自定义单位，可直接用于桌面、无界面和对拍命令。Rust 的 WML 事件覆盖范围取决于场景实际使用的标签。

```sh
cargo run --quiet --manifest-path engine_lite/Cargo.toml --bin rustnoth-campaign -- --list campaign.cfg
cargo run --quiet --manifest-path engine_lite/Cargo.toml --bin rustnoth-campaign -- campaign.cfg 01_Born_to_the_Banner ../wesnoth_src/data/campaigns/The_South_Guard/maps > scenario.cfg
```

使用原版 Wesnoth 1.18.8 可执行文件、固定场景 WML 和动作脚本。自带规则的场景使用与无界面命令相同的包格式。`map_file` 相对于包所在目录解析。对拍工具将原版裁判限制在本次运行目录的独立用户配置中。

```sh
cargo run --manifest-path engine_lite/Cargo.toml --bin rustnoth-parity -- <wesnoth-executable> <bundle.cfg> <actions.tsv> <seed> <new-run-dir>
```

使用原版单位和地形时，用对拍所用的同一份 Wesnoth 1.18.8 安装预处理核心配置。

```sh
wesnoth --preprocess-defines=SKIP_CORE,NO_TERRAIN_GFX --preprocess "<install>/data/core/_main.cfg" "<out>"
```

在对拍命令末尾加上 `--core-config "<out>/_main.cfg"`。场景包可只包含 `[scenario]`，也可附带自定义单位和地形。Rust 从预处理文件读取原版规则，原版裁判从自己的安装读取内置规则。

要在一张地图上自动探索，可让原版从行动方已知的地形、单位和招募名单中选择动作。给定场景、动作上限和种子即可运行，适用于建局工具输出的多人地图。

```sh
cargo run --manifest-path engine_lite/Cargo.toml --bin rustnoth-parity -- --explore <wesnoth-executable> <bundle.cfg> 60 5489 <new-run-dir> --core-config "<out>/_main.cfg"
```

原版到达动作上限或对局结束时写完最后一帧。运行目录中的 `actions.tsv` 记录实际选中的动作，可用于下述独立回放。指定动作上限为零可单独核对开局。

修正规则后，可用原版已经执行过的那一局再次对拍。提供原先的场景包、外部地图、动作脚本、种子和核心配置。命令会从旧运行目录读取 `native.stdout` 与 `native.stderr`，按原版当时的随机数重新执行攻击和招募，将本次的 `rust.tsv` 写到新目录。

```sh
cargo run --manifest-path engine_lite/Cargo.toml --bin rustnoth-parity -- --replay-capture <capture-dir> <bundle.cfg> <actions.tsv> <seed> <new-run-dir> [--core-config <preprocessed-core.cfg>]
```

独立回放也可加载同一个预处理文件。它用指定的种子运行 Rust 裁判，并将逐步记录写入标准输出。

```sh
cargo run --manifest-path engine_lite/Cargo.toml --bin rustnoth-step-trace -- <bundle.cfg> <actions.tsv> <seed> --core-config "<out>/_main.cfg"
```

动作文件为 UTF-8，每行字段用制表符分隔。坐标从 1 起算，武器索引从 0 起算。以下是动作格式示例。

```text
move	1	2	2	2
attack	2	2	3	2	0
recruit	4	2	Spearman
end_turn
```

第 0 帧是开局，以后每个动作产生一帧。比较覆盖回合、行动方、双方金币、单位状态（含战斗经验）和村庄归属。命令在首处差异退出，运行目录保留 `rust.tsv`、`native.stdout` 和 `native.stderr`。这些文件用于裁判审计。

启用战争迷雾或未探索黑幕的阵营会额外输出每行地图上的可见、雾中和未探索地块，以及当前能看见的单位坐标。原版记录每次攻击和招募特质抽样的种子、计数和随机数。Rust 从这些检查点回放动作，校验随机数序列和次数，然后比较每步状态。命令行种子提供给双方初始化，原版同步动作自行选择的种子以原版记录为准。原版启动或执行失败时，运行目录保留原版日志。
