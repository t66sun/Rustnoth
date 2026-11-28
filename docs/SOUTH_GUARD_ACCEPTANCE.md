# The South Guard 前两关验收

使用 Windows 版 Wesnoth 1.18.8、Civilian（`EASY`）与战役初始种子 5489。顺序为 Born to the Banner、Proven by the Sword。命令在仓库根目录的 PowerShell 7 中运行。`$stock` 指向含 `wesnoth.exe` 和 `data` 的正版安装目录，输出目录需使用新名称。

## 准备原版素材和程序

```powershell
$stock = 'D:/SteamLibrary/steamapps/common/wesnoth'
$wesnoth = "$stock/wesnoth.exe"
$campaign = "$stock/data/campaigns/The_South_Guard"
$out = (New-Item -ItemType Directory "verification/sg-$(Get-Date -Format yyyyMMdd-HHmmss)").FullName
$profile = '--wnoconsole','--no-log-to-file','--data-dir',$stock,'--userdata-dir',"$out/user",'--userconfig-dir',"$out/config",'--usercache-dir',"$out/cache"
& $wesnoth @profile --preprocess-defines=CAMPAIGN_THE_SOUTH_GUARD,EASY,NO_TERRAIN_GFX --preprocess "$campaign/_main.cfg" "$out/campaign" | Out-Host
if ($LASTEXITCODE) { throw '原版战役预处理失败' }
& $wesnoth @profile --preprocess-defines=SKIP_CORE,NO_TERRAIN_GFX --preprocess "$stock/data/core/_main.cfg" "$out/core" | Out-Host
if ($LASTEXITCODE) { throw '原版核心预处理失败' }
$build = '--locked','--release','--manifest-path','engine_lite/Cargo.toml','--target-dir','runs/sg-engine'
cargo build @build --bin rustnoth-campaign --example south_guard
cargo build --locked --release --manifest-path rustnoth_gui/Cargo.toml --target-dir runs/sg-desktop
$ids = '01_Born_to_the_Banner','02_Proven_by_the_Sword'
foreach ($id in $ids) {
    & runs/sg-engine/release/rustnoth-campaign.exe "$out/campaign/_main.cfg" $id "$campaign/maps" > "$out/$id.cfg"
    if ($LASTEXITCODE) { throw "导出 $id 失败" }
}
```

## 原版联跑与固定动作复跑

```powershell
$driver = 'engine_lite/examples/south_guard/capture.py'
$check = (Resolve-Path 'runs/sg-engine/release/examples/south_guard.exe').Path
python $driver $wesnoth $check $out "$out/first" --deterministic
if ($LASTEXITCODE) { throw '原版首次采集失败' }
python $driver $wesnoth $check $out "$out/repeat" --deterministic --fixed-actions "$out/first/actions"
if ($LASTEXITCODE) { throw '固定动作复跑失败' }
& $check check $out "$out/repeat/$($ids[0])" "$out/repeat/$($ids[1])" "$out/repeat/incoming-native.cfg" "$out/offline-replay"
if ($LASTEXITCODE) { throw '原版记录的 Rust 回放失败' }
foreach ($id in $ids) {
    foreach ($relative in "actions/$id.tsv","$id/native.stdout","replay/$id.tsv") {
        if ((Get-FileHash "$out/first/$relative").Hash -ne (Get-FileHash "$out/repeat/$relative").Hash) {
            throw "两次运行不同 $relative"
        }
    }
}
```

采集器用同一进程的原版单人战役控制器依次运行两关。脚本代双方提交游戏命令，固定动作文件使用 WML 的一基坐标和零基武器序号。原版选择器只使用当前阵营可见信息。记录器在裁判中取得开局、战斗、征募和移动事件的随机抽样。Rust 从自己的第一关结算进入第二关，并与原版生成的第二关进场存档核对结转金币、名单顺序、兵种、性别、经验与特性。过关以原版场景自身的结局事件为准。测试运行会自动关闭故事对话和结算报告，在第二关结束后返回。

| 位置 | 核查内容 |
| --- | --- |
| `actions/<场景 ID>.tsv` | 可直接交给下一次运行的完整双方动作 |
| `<场景 ID>/native.stdout`、`replay/<场景 ID>.tsv` | 开局、每一步和终局的单位、金币、村庄、双方逐格视野及可见单位 |
| `<场景 ID>/native.stderr` | 原版 RNG 窗口、城堡目标切换、真实结局事件 |
| `incoming-native.cfg`、`replay/outgoing.cfg` | 原版第二关进场与 Rust 第一关结算 |
| `userdata/saves/` | 原版回放、回合自动存档和转关存档 |

该路线需要在河堡见到 Sir Gerrick、在村庄救出 Aleron、带 Deoran 到西城、获得五名援军并击败 Urza Nalmath。原版固定脚本中的敌我双方动作都可查阅。若输出首处分歧，保留整份运行目录，从相应步骤的动作、抽样和前后两帧定位。

## Windows 桌面路线

```powershell
$env:APPDATA = "$out/desktop-profile"
& runs/sg-desktop/release/rustnoth_gui.exe --title "$out/campaign/_main.cfg" "$campaign/maps" "$out/core/_main.cfg" 5489 --wesnoth-root $stock
```

从 Campaigns 选择 The South Guard 和 Civilian。第一回合将 Moreth 从 (14,5) 移到 (12,8)、Deoran 从 (15,5) 经 (10,7) 移到 (9,9)，再将 Moreth 移到 (11,9)。通过 Menu → Save Game 存档，退出进程，用同一命令启动，从标题 Load 读回。第二回合由 Moreth 进入 (9,10)，再由 Deoran 进入 (11,12)。第一关获胜后记下 Remaining gold 和 Retained gold，进入第二关检查顶部金币和四名驻军；在 7 回合期限前带 Deoran 进入 (22,13)，然后击败敌首领。从标题 Load 选择各关的胜利回放，点播放并核对终局。存档和回放位于 `$env:APPDATA/Rustnoth/saves`。桌面对手由 Rustnoth 自身的 AI 驱动，每局的实际动作以该局存档为准。

两关原版的战争迷雾和黑幕均关闭。逐格视野对拍照常执行。测实际揭雾耗时需使用相同地图、相同开局移动且仅给玩家阵营开启迷雾的单独样本。

窗口条件、帧时间、恢复交互和拖动延迟的复跑步骤见 [Windows 战役路线计时](SOUTH_GUARD_TIMINGS.md)。
