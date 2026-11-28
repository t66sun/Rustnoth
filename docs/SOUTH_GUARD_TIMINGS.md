# Windows 战役路线计时

在 Windows 主显示器上按 [两关验收流程](SOUTH_GUARD_ACCEPTANCE.md) 准备 Civilian 战役、种子 5489 和独立存档目录。使用同一个发布版、存档、地图位置、缩放、窗口尺寸、显示器刷新率和同步设置。记录每次采样的版本。首次着色、进程重启与系统文件缓存是否预热也要注明。

```powershell
$env:RUSTNOTH_FRAME_TRACE = "$out/before-01.tsv"
$game = Start-Process -PassThru runs/sg-desktop/release/rustnoth_gui.exe -ArgumentList @('--title',"$out/campaign/_main.cfg","$campaign/maps","$out/core/_main.cfg",'5489','--wesnoth-root',$stock)
```

从标题 Load 选择第一关中途或第二关开局存档并确认。重复读档时，每个样本都重新启动进程并使用新的 TSV。预览包含实际读盘、解析和重放历史。点击 Load 到首次可操作单独计时。按流程移动、战斗与结束回合，然后正常退出。比较两个版本时，各自选用同一个存档与同一段动作。Windows 默认使用桌面合成、交换间隔 0 和按显示刷新率定时的帧调度。若要测解除限速，在两边的启动参数最前面加入 `--uncapped` 并单独汇总。

```powershell
$measure = 'rustnoth_gui/tests/measure_route_frames.py'
python $measure "$out/before-01.tsv" "$out/before-02.tsv" "$out/before-03.tsv" --out "$out/before.json"
```

`groups` 包含操作数、帧间隔数、每帧 P95、最长帧、长帧数、操作耗时 P95 和首次可用输入界面。长帧指超过两个刷新周期的帧间隔；P95 取排序后的第 `ceil(0.95*n)` 个样本。计时涵盖跨过操作边界的帧。样本不完整时保留记录，该组 P95 留空。少于 20 次操作时，操作 P95 等于已采样的最大值。

| 项目 | 起止 |
| --- | --- |
| `load_preview`、`load` | 选择存档至预览生成；确认加载至地图、目标、剧情或回放可操作 |
| `end_turn`、`ai` | 确认交接至首次可交互；电脑开始执行实际命令至首次可交互。途中需要点击对白时，后续电脑操作另起一个样本 |
| `move`、`fog_reveal` | 提交移动至首次可交互；玩家新看见格子时归入揭雾，记录格数 |
| `combat`、`combat_animation`、`animation_recovery` | 玩家的战斗命令至可操作；实际战斗动画时长；动画结束至首次可操作，双方战斗均覆盖 |

两关按原版设定关闭战争迷雾。测揭雾可从第一关的导出文件另建一份地图，在 Deoran 的直属 `[side]` 增加唯一改动 `fog="yes"`，然后走原路线前四步。用 `rustnoth-parity <wesnoth.exe> <探针.cfg> <四步.tsv> 5489 <新结果目录> --core-config <core/_main.cfg>` 先核对这四步，并在报告中将该变体单列。桌面可用 `rustnoth_gui <探针.cfg> <core/_main.cfg> 5489 --wesnoth-root <原版根目录>` 打开。

## 输入到首次地图位移

使用带 NumPy 的 Python。在第二关开局的正常棋盘上，点击小地图中心，确保视口上下都有移动空间。游戏窗口需要在主屏前台。运行工具时它会恢复原鼠标位置。使用相同地图、视口和输入位移复测。

```powershell
python rustnoth_gui/tests/measure_map_drag.py $game.Id "$out/drag-before.json" --trace $env:RUSTNOTH_FRAME_TRACE --samples 30
```

程序按住中键，向上下交替发送 24 个物理像素的鼠标移动。计时从发送鼠标移动开始，到屏幕采样首次检测到同方向的地形平移结束。采样区域避开光标；每个样本保留平移向量、相关度和最后一次轮询间隔。GDI 采样和桌面合成会影响检测时间。未检测到位移的样本保留且该组 P95 留空。相同 trace 校验进程、渲染尺寸、刷新率与帧调度设置。
