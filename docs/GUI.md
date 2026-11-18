# 桌面版

桌面版使用 Rust 裁判运行全图可见的本地混战。各阵营依次使用同一窗口，行动范围由当前阵营的裁判视图决定。地形、精灵、队色和字体从本地 Wesnoth 1.18.8 源码目录读取。

从仓库根目录运行。准备包含 `[scenario]` 的场景包，以及[原版核心规则的预处理文件](PARITY.md)。

```sh
cargo run --manifest-path rustnoth_gui/Cargo.toml --locked -- bundle.cfg core.cfg 5489
```

示例使用随机种子 `5489`。素材默认从 `../wesnoth_src` 读取，可以在命令末尾加上 `--wesnoth-root <目录>`。场景的 `map_file` 路径相对于场景包。

### 用原版地图建立遭遇战

在窗口内选择地图和默认时代阵营，可从仓库根目录运行：

```sh
cargo run --manifest-path rustnoth_gui/Cargo.toml --locked -- --skirmish core.cfg 5489
```

点击地图、阵营编号、阵营或领袖以打开列表。每个地图起点可独立选择阵营和领袖。上下键和 Tab 可切换表单项目，Enter 打开列表或开始游戏。列表支持方向键、Page Up、Page Down、滚轮和鼠标选择。Esc 返回表单。桌面遭遇战使用 30 回合上限。素材目录可通过 `--wesnoth-root` 指定。

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
| 招募 | 取消单位选择后，点标出的城堡格并从侧栏选择单位 |
| 选择菜单项 | 左键点击，或按 Tab 切换、Enter 确认 |
| 取消选择 | 右键 |
| 结束回合 | 点击 **End turn**，或按空格 |
| 缩放棋盘 | 在棋盘上滚动滚轮，按 Home 恢复全图 |
| 平移棋盘 | 缩放后使用中键拖拽或方向键 |
| 关闭菜单或退出 | Esc |
