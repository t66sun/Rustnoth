# 桌面版

双击 `rustnoth_gui` 即可进入原版标题画面。程序会在 Steam 游戏库中寻找 Wesnoth 1.18.8。也可以使用 `rustnoth_gui --wesnoth-root <安装目录>` 或设置 `WESNOTH_ROOT` 指定安装位置。首次进入 Multiplayer 或 Campaigns 时，程序用原版可执行文件预处理规则并保存在当前用户的 Rustnoth 缓存中。安装了《南疆卫队》时，Campaigns 可打开该战役。

桌面版使用 Rust 裁判运行本地对局，支持自由混战和盟军共同获胜。各阵营依次使用同一窗口，棋盘和行动范围由当前阵营的裁判视图决定。启用战争迷雾后，曾探索但当前不可见的地形会变暗；启用黑幕后，未探索的格子会被遮住。地形、精灵、队色和字体从本地 Wesnoth 源码目录读取。

从仓库根目录运行。准备包含 `[scenario]` 的场景包，以及[原版核心规则的预处理文件](PARITY.md)。

```sh
cargo run --manifest-path rustnoth_gui/Cargo.toml --locked -- bundle.cfg core.cfg 5489
```

示例使用随机种子 `5489`。素材默认从 `../wesnoth_src` 读取，可以在命令末尾加上 `--wesnoth-root <目录>`。场景的 `map_file` 路径相对于场景包。

### 从原版标题画面开始

```sh
cargo run --manifest-path rustnoth_gui/Cargo.toml --locked -- --title core.cfg 5489
```

标题画面使用本机 Wesnoth 1.18.8 的地图、徽标、按钮和每日提示。选择 **Multiplayer** 可建立本地热座或人机遭遇战。如需使用 **Campaigns**，在核心规则前提供已预处理的战役配置和地图目录，例如 `--title campaign.cfg ../wesnoth_src/data/campaigns/The_South_Guard/maps core.cfg 5489`。方向键上下切换按钮，Enter 确认，左右键翻阅提示。在战役列表或遭遇战设置页按 Esc 可返回标题。

### 在窗口中选择战役场景

给出已预处理的战役配置、该战役的 `maps` 目录、核心规则和种子。例如：

```sh
cargo run --manifest-path rustnoth_gui/Cargo.toml --locked -- --campaign campaign.cfg ../wesnoth_src/data/campaigns/The_South_Guard/maps core.cfg 5489
```

列表按战役文件中的顺序显示场景名称和 ID。点击场景，或用方向键选择后按 Enter，即可开始本地热座对局。Esc 从棋盘返回列表。`--wesnoth-root` 可指定原版素材目录。

### 用原版地图建立遭遇战

在窗口内选择地图和默认时代阵营，可从仓库根目录运行：

```sh
cargo run --manifest-path rustnoth_gui/Cargo.toml --locked -- --skirmish core.cfg 5489
```

点击地图、阵营编号、阵营或领袖以打开列表。每个地图起点可独立选择阵营和领袖。Visibility 可选择全图可见、战争迷雾、黑幕或两者并用。上下键和 Tab 可切换表单项目，Enter 打开列表或开始游戏。列表支持方向键、Page Up、Page Down、滚轮和鼠标选择。Esc 返回表单。Rounds 默认 30，可选 1 至 99 回合或 Unlimited。素材目录可通过 `--wesnoth-root` 指定。

**Play hot seat** 让多位玩家轮流操作。**Play vs computer** 由玩家操作第 1 方，电脑操作其余各方。电脑可以招募、接敌和探索，棋盘始终显示第 1 方的视野。

从仓库根目录运行下例，用原版 Hamlets 地图创建 30 回合的遭遇战。将生成的文件用作桌面、无界面或逐步对拍命令的场景包，并提供同一版本的预处理核心配置。

```sh
mp=../wesnoth_src/data/multiplayer
mkdir -p data
cargo run --quiet --manifest-path engine_lite/Cargo.toml --bin rustnoth-skirmish -- \
  "$mp/maps/2p_Hamlets.map" 30 "$mp/factions/loyalists-default.cfg" --leader "White Mage" \
  "$mp/factions/rebels-default.cfg" > data/skirmish.cfg
```

按地图起点编号依次传入至少两个阵营文件，可使用七个默认时代阵营或重复选择同一阵营。每方默认使用文件中列出的第一个领袖，可在该文件后加上 `--leader` 指定其他已列出的领袖。场景内嵌地形，采用独立敌对阵营、全图可见和从黎明开始的六段昼夜循环。此玩法使用地图地形和出生点，原版场景的专属事件需另行提供。

| 操作 | 控制 |
| --- | --- |
| 移动 | 左键选中己方单位，再点标出的可达格 |
| 攻击 | 左键选中己方单位，再点敌方目标，从侧栏选择武器 |
| 选择晋级方向 | 己方攻击者有多个晋级方向时，点击武器列表上方的 **Advance to** 或按 P 循环选择。本次战斗中达到升级经验时生效，**Default** 使用单位定义中的默认方向 |
| 招募 | 取消单位选择后，点标出的城堡格并从侧栏选择单位 |
| 召回 | 点可召回的城堡格，在侧栏按兵种、老兵 ID、生命、经验和金币花费挑选召回对象 |
| 遣散 | 按 D 打开本方预备兵名单，点击条目或用 Tab、Enter 选择。核对兵种和 ID 后再次点击或按 Enter 确认，按 Esc 或右键取消。遣散不消耗金币 |
| 选择菜单项 | 左键点击，或按 Tab 切换、Enter 确认 |
| 取消选择 | 右键 |
| 结束回合 | 点击 **End turn**，或按空格 |
| 缩放棋盘 | 在棋盘上滚动滚轮，按 Home 恢复全图 |
| 平移棋盘 | 缩放后使用中键拖拽或方向键 |
| 关闭菜单或退出 | Esc |
