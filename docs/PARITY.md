# 逐步裁判对拍

Rust 裁判会按单位 `[modifications]` 中的顺序应用特质、永久物品和已支持的默认满级晋级。永久物品的 `duration` 可省略、留空或写为 `forever`。召回保留物品，升级时按新兵种重新计算已支持的属性和武器效果。

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

场景事件含有 `[endlevel]` 时，对拍保留原始控制者设置，从 Wesnoth 的本地胜败回调读取实际结局。开场结束记作第 0 帧，动作或回合中结束记作触发动作的帧。固定脚本需在该帧结束。此模式的 `test_result` 属于对拍驱动，请使用普通战役的 `[endlevel]` 配置。

动作文件为 UTF-8，每行字段用制表符分隔。坐标从 1 起算，武器索引从 0 起算。以下是动作格式示例。

```text
move	1	2	2	2
attack	2	2	3	2	0
recruit	4	2	Spearman
end_turn
```

第 0 帧是开局，以后每个动作产生一帧。比较覆盖回合、行动方、双方金币、单位状态（含战斗经验）和村庄归属。命令在首处差异退出，运行目录保留 `rust.tsv`、`native.stdout` 和 `native.stderr`。这些文件用于裁判审计。

启用战争迷雾或未探索黑幕的阵营会额外输出每行地图上的可见、雾中和未探索地块，以及当前能看见的单位坐标。原版记录每次攻击和招募特质抽样的种子、计数和随机数。Rust 从这些检查点回放动作，校验随机数序列和次数，然后比较每步状态。命令行种子提供给双方初始化，原版同步动作自行选择的种子以原版记录为准。原版启动或执行失败时，运行目录保留原版日志。

## The South Guard 第一关河堡事件

在仓库根目录用 PowerShell 7.4 或更高版本运行。把 `$install` 指向 Wesnoth 1.18.8 安装目录。以下命令使用 Civilian（`EASY`）难度，生成场景包和十步脚本，并保留原版招募时的随机数供复验。

```powershell
$install = 'D:\SteamLibrary\steamapps\common\wesnoth'
$out = (New-Item -ItemType Directory -Path "verification/river-fort-$(Get-Date -Format yyyyMMdd-HHmmss)").FullName
$campaign = "$install/data/campaigns/The_South_Guard"
$wesnoth = "$install/wesnoth.exe"
$profileArgs = '--wnoconsole', '--no-log-to-file', '--data-dir', $install, '--userdata-dir', "$out/userdata", '--userconfig-dir', "$out/config", '--usercache-dir', "$out/cache"
& $wesnoth @profileArgs --preprocess-defines=CAMPAIGN_THE_SOUTH_GUARD,EASY,NO_TERRAIN_GFX --preprocess "$campaign/_main.cfg" "$out/campaign" | Out-Host
& $wesnoth @profileArgs --preprocess-defines=SKIP_CORE,NO_TERRAIN_GFX --preprocess "$install/data/core/_main.cfg" "$out/core" | Out-Host

$engine = '--release', '--locked', '--manifest-path', 'engine_lite/Cargo.toml'
cargo run @engine --bin rustnoth-campaign -- "$out/campaign/_main.cfg" 01_Born_to_the_Banner "$campaign/maps" > "$out/scenario.cfg"
@(
    'move 14 5 12 8', 'move 15 5 10 7', 'move 10 7 9 9', 'move 12 8 11 9', 'end_turn',
    'recruit 14 18 Ruffian', 'recruit 14 19 Ruffian', 'move 15 19 10 18', 'end_turn', 'move 11 9 9 10'
) -replace ' ', "`t" | Set-Content -Encoding utf8NoBOM "$out/actions.tsv"

$route = "$out/scenario.cfg", "$out/actions.tsv", '5489'
cargo run @engine --bin rustnoth-parity -- $wesnoth @route "$out/native" --core-config "$out/core/_main.cfg"
cargo run @engine --bin rustnoth-parity -- --replay-capture "$out/native" @route "$out/replay" --core-config "$out/core/_main.cfg"
```

两次对拍都应输出 `Matched 11 step(s)`，包含开局和十步动作。第十步 Moreth 进入 `(9,10)`，Sir Gerrick（`Infantry Lieutenant`）出现在 `(10,9)`，第二方金币从 6 增至 36。原版逐帧记录在 `$out/native/native.stdout`，同局 Rust 回放在 `$out/replay/rust.tsv`。
