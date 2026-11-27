# 开发指引

## 仓库入口

| 位置 | 职责 |
| --- | --- |
| [crates/mhgu-sdk](../crates/mhgu-sdk/README.md) | GU 1.4 引擎绑定，包括原生输入和转向、动作、碰撞、资源与注册接口。 |
| [crates/mhfz-sdk](../crates/mhfz-sdk/README.md) | 极型武器共用的 Frontier 动作程序、动作库预加载、技能帧数补正、弹刀、镜头震动和表现时序。 |
| [plugins/mhgu-sdk-runtime](../plugins/mhgu-sdk-runtime/README.md) | 构建共享 Forge 插件 `mhgu-sdk.nro`，提供武器风格注册和角色存档服务，管理 GU 共用钩子和持久化。 |
| [极之型大剑](../plugins/great-sword-extreme/README.md)、[极之型双剑](../plugins/dual-blades-extreme/README.md)、[极之型太刀](../plugins/long-sword-extreme/README.md)、[极之型斩斧F](../plugins/switch-axe-extreme/README.md)、[极之型单手剑](../plugins/sword-shield-extreme/README.md)、[极之型铳枪](../plugins/gunlance-extreme/README.md) | 各武器的动作编号、派生优先级、精准防御规则、槽、专属狩技及表现资源。 |
| [极之型磁斩锤交易测试](../plugins/magnet-spike/README.md) | 用 MHF-Z 磁斩锤验证独立装备、模型、购买、制造与存档。 |
| [tools/mhfz2mhgu](../tools/mhfz2mhgu/README.md) | 宿主资源转换 CLI。`mhf-resource` 读 FZ 格式，`mhgu-resource` 读写 GU 格式，`src/hunter.rs` 等负责具体移植映射。 |
| [tools/package_weapon_styles.py](../tools/package_weapon_styles.py) | 组装 SDK 和六个武器的安装产物，在仓库根目录生成极之型整合包。 |

各武器当前的派生、槽、技能、特效和联机实现可在[极之型武器实现与回归](weapon-style-implementation.md)按功能查阅。

## 文档同步

1. 开工时读本页和受影响的插件 README。涉及玩家操作时同时检查对应的 `src/manual.txt`，涉及 SDK 或转换器时查看对应组件的 README。
2. 改动落实后，按影响范围同步说明。更新日志按交付顺序追加到文件末尾。键位、派生和槽规则写在武器 README，玩家需要的操作同步到游戏内手册。共用接入和验收规则更新到[移植流程](weapon-style-porting.md)，跨武器行为及相关回放更新到[实现与回归](weapon-style-implementation.md)。构建、安装和选用验证的命令集中在本页。新出现或复发的故障在[常见故障与排查](weapon-style-troubleshooting.md)记录触发条件、检查点、证据和对应回放，修正已有的旧结论。
3. 交付前在同一改动中核对文档与最终实现。检查相关命令的参数、测试入口、相对链接和旧说法。手册改动按[注册器说明](../plugins/mhgu-sdk-runtime/README.md)检查格式，并用打包后的 NRO 走共用武器验收。提交或交付说明注明同步的文档、已验证的范围和待实机确认的事项。现有说明仍适用时，说明检查了哪些入口。

## 本地输入和构建

准备 Rust、插件目录声明的 nightly 和 `rust-src`、`cargo forge` 以及 Python 3。Forge 插件使用 ARM32 小端自定义目标。直接执行 `cargo forge build` 时从插件目录启动，让 Cargo 发现该目录的 `.cargo/config.toml`。

| 输入 | 用途 |
| --- | --- |
| `MHFZ_DAT` | Frontier 的 `dat` 目录。转换器读取其中的猎人模型、动作、动作表、判定、音效和粒子资源。 |
| `MHGU_ROMFS` | 包含 `nativeNX` 的 GU RomFS 根目录。武器打包脚本统一通过第二个位置参数接收此路径。部分离线回放从环境变量读取 GU 武器 ARC。 |
| `mhfz/eboot.bin.elf`、`mhgu/main.elf` | 对应版本的本地 ELF。部分离线脚本使用这两个仓库相对位置。 |
| `mhfz/out/`、`mhgu/out/` | 可选的本地反编译导出。优先用 `function_list.txt` 和 `decompiled.c` 定位函数，再用资源表和调用者核对解释。 |

首次构建或修改资源后，从仓库根目录打包。只运行所需的武器打包命令即可。

```sh
export MHGU_ROMFS="${MHGU_ROMFS:?请指定包含 nativeNX 的 GU RomFS 根目录}"
: "${MHFZ_DAT:?请指定 Frontier 的 dat 目录}"
python3 plugins/mhgu-sdk-runtime/package.py
python3 plugins/great-sword-extreme/package.py "$MHFZ_DAT" "$MHGU_ROMFS"
python3 plugins/dual-blades-extreme/package.py "$MHFZ_DAT" "$MHGU_ROMFS"
python3 plugins/long-sword-extreme/package.py "$MHFZ_DAT" "$MHGU_ROMFS"
python3 plugins/switch-axe-extreme/package.py "$MHFZ_DAT" "$MHGU_ROMFS"
python3 plugins/sword-shield-extreme/package.py "$MHFZ_DAT" "$MHGU_ROMFS"
python3 plugins/gunlance-extreme/package.py "$MHFZ_DAT" "$MHGU_ROMFS"
python3 tools/package_weapon_styles.py
```

武器打包脚本先把源数据转换到插件的 `target/assets/`，再构建 NRO 并组装 `dist/<插件名>/romfs/nativeNX`。动作程序和部分事件表由 `include_bytes!` 编入 NRO，LMT、命中、粒子、音效和贴图先以开发散文件生成，供单武器回放读取。每种武器的男女模型共用 `0.lmt`、`2.lmt`、`4.lmt` 三个动作库。

`tools/package_weapon_styles.py` 从这七个开发目录组装发布版 `dist/weapon-styles/`。每种武器包含独立 NRO 和一个 `arc/frontier/<武器>.arc`。`mhgu-sdk/` 包含 `mhgu-sdk.nro` 和 `arc/frontier/common.arc`。整合包共 14 个文件。当前共用贴图和音频样本按内容去重，打包器在 ARC 中重写 MRL 和 SBKR 引用。

已有匹配的资源且本次仅改 Rust 时，可以增量构建。离线脚本通常读取 `dist` 中的 NRO，需同步本次产物再运行。以下以双剑为例。

```sh
(cd plugins/dual-blades-extreme && cargo forge build --release) &&
  cp plugins/dual-blades-extreme/dual_blades_extreme.nro \
     plugins/dual-blades-extreme/dist/dual-blades-extreme/romfs/nativeNX/plugins/
```

共用 SDK 改动后重建运行时及受影响的内容插件，再运行 `tools/package_weapon_styles.py` 组装发布版。转换器、源数据或资源格式发生变化时，重新运行相关打包脚本，让 NRO 内的事件表与外部资源一起更新。整理代码后也可对比重建前后的非 NRO 文件清单和哈希，确认生成资源一致。

## 调查原生入口

已有导出时可用 `rg -n 'func-address: 0x2b72d4' mhgu/out/decompiled.c` 定位攻击构造器，再跟到调用者、虚表或数据资源。记录游戏版本、函数相对地址、输入与返回语义。反编译推断不确定时才核对该入口的指令。带浮点数的原生调用还要确认参数走通用寄存器还是 VFP 寄存器，优先复用 SDK 现有的桥接函数。

运行时在所属游戏线程使用活对象及借用指针。注册描述符和提供给引擎长期读取的表使用常驻存储。共享派发入口可能收到集会所猎人，读取任务专用字段前通过 `hunter::is_quest` 确认类型。切换动作、场景或角色时，按实际生命周期清理插件状态，确认旧对象仍有效后再访问它。

新增 SDK 接口优先接收值类型或经 SDK 验证的借用句柄，并在 SDK 内完成原生指针读写与调用。碰撞展开等时机敏感的接口可接收模组返回的形状参数，由 SDK 在原生回调内校验并提交。直接挂接原生回调时，原始地址到借用句柄的转换仍须确认对象、线程和生命周期。

运行时通过版本化函数表向内容 NRO 提供服务。模组在 Cargo 中依赖 `crates/mhgu-sdk`，可在各自的 Forge 入口用 `weapon::style::registry::when_ready` 登记风格初始化函数，自定义武器用 `weapon::registry::when_ready`。未发布的内部服务在同一开发周期沿用固定标识。已发布接口出现不兼容的函数签名或布局变更时，再为新接口分配一个版本。修改共用接口后，一起重建并安装运行时和使用该接口的插件。

## 安装和确认

启用 Forge 后，把 `dist/weapon-styles/` 下的 `mhgu-sdk/` 和所需武器目录分别装入 MHGURecomp 的模组目录，保留完整的 `romfs/nativeNX` 层级。使用开发散文件版时，可以使用 `plugins/mhgu-sdk-runtime/dist/mhgu-sdk/`。同一游戏只需一份 SDK，武器可在 SDK 前后加载。升级时移除旧的 `style-registry`，保留原有存档。共用钩子或资源布局变化时，整目录更新 SDK 和关联内容模组，保证 NRO、`common.arc` 和武器 ARC 来自同一包。仅修改代码且资源匹配时可以只更新对应的 NRO。

完整极之型合包包含 SDK 和当前六个武器模组目录。验证磁斩锤时，先用 `python3 plugins/magnet-spike/package.py "$MHFZ_DAT" "$MHGU_ROMFS"` 构建，再用 `python3 tools/package_weapon_styles.py --include-magnet-spike` 组装含磁斩锤 NRO、模型 ARC 和专用图标 TEX 的整合包（17 个文件）。打包回归 `tools/tests/verify_packed_release.py` 同样传入 `--include-magnet-spike`。归档前核对每个目录的 NRO 和配套资源。已有合包的命名格式为 `mhgu边境风格-YYYYMMDDHHMM.7z`。

重启游戏后，在原版装备箱按当前武器选风格。风格和装备组合随 GU 角色存档写入 [每个角色的 SDK JSON](../plugins/mhgu-sdk-runtime/README.md#角色存档)。升级后可在装备箱重新选择并保存游戏。查错误时依次确认 **安装包的 NRO 和资源、Forge 初始化和钩子日志、装备箱选择、任务内执行路径**。看到模组被列出或 NRO 被加载，只能确认到了加载阶段。

## 验证范围

| 修改内容 | 优先检查 |
| --- | --- |
| 单个速度、距离或计时参数 | 构建受影响的插件。需要安装时对照安装文件与本次产物。手感由任务内观察确认。 |
| 装备箱选择、快捷键与 GU 命令优先级 | [装备箱与保存](../plugins/great-sword-extreme/tests/verify_menu.py)、[聊天、目标切换与地图缩放](../plugins/mhgu-sdk-runtime/tests/verify_chat_shortcut.py)、[大剑输入及动作生命周期](../plugins/great-sword-extreme/tests/verify_style.py)。 |
| 个人和队友状态、装备组合风格名称、自定义武器 | [状态与装备资料回放](../plugins/great-sword-extreme/tests/verify_menu.py)覆盖 GU 状态页面、装备资料收发、队友槽位复用、扩展装备箱原版控制器的打开、翻页、排序确认、换装、原版装备数据完整性、组合应用、存档重载和缺少武器插件时的回退。实机用同版本 SDK 的两台客户端在集会所和任务中互查状态，切回原版后再检查一次。 |
| 扩展武器购买、制造和强化 | [交易与存档回放](../plugins/great-sword-extreme/tests/verify_trade.py)覆盖原版工房与商店入口、翻页、原版成功音效和立即装备分支、配方、余额、道具袋和道具箱扣除、实例强化、库存满、GU 保存失败和强化等级重载。实机用已登记交易的内容插件检查原版工房与商店的列表布局、确认、取消和重进角色。 |
| 冲刺收刀和击飞受身 | [公共动作回放](../plugins/mhgu-sdk-runtime/tests/verify_ground_recovery.py)核对冲刺、刹停和普通收刀选片、GU 的收刀退出，以及双剑、斩斧和铳枪的前后受身。[收刀联机回放](../plugins/mhgu-sdk-runtime/tests/verify_sheathe_multiplayer.py)使用 `MHGU_ROMFS` 验证队友跳过冲刺或刹停时的选片。 |
| 攻击、防御、狩技和武器专属派生 | [大剑精防、蓄力与派生](../plugins/great-sword-extreme/tests/verify_style.py)、[大剑与双剑弹刀、刃打和双手刀光](../plugins/mhgu-sdk-runtime/tests/verify_deflection.py)、[专属狩技](../plugins/mhgu-sdk-runtime/tests/verify_weapon_arts.py)、[太刀](../plugins/long-sword-extreme/tests/verify_port.py)、[斩斧](../plugins/switch-axe-extreme/tests/verify_port.py)、[单手剑](../plugins/sword-shield-extreme/tests/verify_port.py)及[铳枪](../plugins/gunlance-extreme/tests/verify_port.py)的动作回放。 |
| 磁斩锤的本地动作、输入隔离、近战与磁力 | [磁斩锤战斗回放](../plugins/magnet-spike/tests/verify_battle.py)检查 Vita 自然回充曲线、R 组合键、跨区标记、私有动作派发、跨片段预输入、形态切换、公共翻滚、回退武器命令与专用 HUD 隔离、部件采样、近战扣血、两种形态弹刀、磁界弹、部位标记、磁力槽和标记亮点，以及磁界接近与脱离的门槛、位移、回避时间和派生。磁缚覆盖起手、着地、落空收招、阈值、耐性、单机延长输入、FZ 原版磁缚槽游标、计时与达标提示、终结窗口、终结判定与倒地、怪物出生校验及原版麻痹优先级。联机回放检查本地使用原版武器时合并两名队友的贡献，并排除非控制端和过期命中编号。由怪物控制端着地触发磁缚后，再经原版动作包核对接收端的计时、释放、原版麻痹和对旧控制端或出生序号的计时过滤。怪物资料及特效回放另检查同区原版武器观察者、积蓄和耐性乱序过滤及控制权交接，并经 GU 原生数据包回放队友着地申请、确认与受制动作乱序、超时、中断、远端终结的 164／184 帧判定、去重及倒地。磁缚特效回放核对队友起手、动作混合、挂点、终结冲击帧与重复采样，也检查受缚粒子的周期、同时存在的粒子和解除清理。出招端回放同时检查确认闪光、音效、脉冲对怪物关节的实时朝向及观察者隔离。镜头回放使用原版 `255CC` 检查转向的动作门槛、手动横向输入、剧情镜头和控制权交接，经 `26E64` 核对半径和视野的进出时序、终结帧、暂停及 GU 接管地形遮挡。强化磁冲击还检查四段场景判定、音效、粒子、暂停和队友伤害归属。 |
| 磁斩锤挥动轨迹与判定生效帧 | [逐帧姿势对照](../tools/mhfz2mhgu/tests/magnet_trajectory.rs)比较原始 MOT 与男女猎人、武器的成品关节和判定端点。[动作时钟回放](../plugins/magnet-spike/tests/verify_swing_frames.py)对照 Vita 的挂点与事件分发，战斗回放另经两作的原生时钟检查混合、循环和接触寿命。命令见[磁斩锤构建与验证](../plugins/magnet-spike/README.md#构建与验证)。 |
| 动作、音效、特效与材质 | 对照输出槽位、事件帧、挂点或材质字段。参考[资源检查](../plugins/great-sword-extreme/tests/verify_assets.py)、[原生粒子](../plugins/great-sword-extreme/tests/verify_native_fx.py)、[磁斩锤粒子绘制](../plugins/magnet-spike/tests/verify_fx.py)和[材质绘制](../plugins/great-sword-extreme/tests/verify_native_material.py)。 |
| 注册顺序、手册、预加载和角色交接 | [共用武器验收](../plugins/mhgu-sdk-runtime/tests/verify_weapon_styles.py)覆盖六种武器、注册器最后启动、两个手册入口、任务预读和成品粒子路径。[斩斧集会所回放](../plugins/switch-axe-extreme/tests/verify_lobby.py)核对任务角色与集会所角色交接。 |
| 联机动作与伤害归属 | [大剑与双剑](../plugins/mhgu-sdk-runtime/tests/verify_gs_db_multiplayer.py)、[太刀](../plugins/long-sword-extreme/tests/verify_multiplayer.py)、[片手与铳枪](../plugins/mhgu-sdk-runtime/tests/verify_sns_gl_multiplayer.py)、[斩斧](../plugins/switch-axe-extreme/tests/verify_port.py)和[磁斩锤](../plugins/magnet-spike/tests/verify_battle.py)覆盖 GU 原生动作包和队员命中。实机另查双方的动作、特效和结算。 |

从仓库根目录使用可导入 `unicorn` 的 Python 运行离线脚本。准备相应的本地 ELF，并保留打包生成的 `target/assets` 和 `dist`。涉及 GU 原版武器 ARC 的脚本还需要导出 `MHGU_ROMFS`。共用武器验收需要已构建的注册器和所有武器风格的 `dist`。

```sh
python3 plugins/mhgu-sdk-runtime/tests/verify_chat_shortcut.py
python3 plugins/mhgu-sdk-runtime/tests/verify_weapon_styles.py
python3 tools/tests/verify_packed_release.py
python3 plugins/switch-axe-extreme/tests/verify_lobby.py
python3 plugins/switch-axe-extreme/tests/verify_port.py
```

分别记录构建、安装、离线回放和实机结果。离线回放可以核对事件、命令、资源路径与原生回调。光效外观、实际距离、操作手感及真实联机以任务内观察为准，复现记录应包含角色状态、输入顺序、区域、预期与实际表现。
