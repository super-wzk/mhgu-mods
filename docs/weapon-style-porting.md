# 武器风格移植流程

本文以 Frontier 武器作为 MHGU 1.4 新风格接入。构建和安装见[开发指引](development.md)，实际招式、键位和移植差异见下表的武器说明。

## 1. 确认源武器和招式表

从同一版本的 FZ 载入器和 `dat` 数据确认武器、风格、动作表、动作资源。GU 的装备武器编号、FZ 的武器编号、自定义风格编号分别记录。

| 移植实例 | FZ 武器与动作资源 | GU 装备武器编号 | 自定义风格 ID |
| --- | --- | --- | --- |
| [极之型大剑](../plugins/great-sword-extreme/README.md) | 0，`motion/w00.mot` | 0 | 10 |
| [极之型双剑](../plugins/dual-blades-extreme/README.md) | 6，`motion/w06.mot` | 11 | 11 |
| [极之型太刀](../plugins/long-sword-extreme/README.md) | 7，`motion/w07.mot` | 7 | 12 |
| [极之型斩斧F](../plugins/switch-axe-extreme/README.md) | 12，`motion/w12.mot` | 8 | 13 |
| [极之型单手剑](../plugins/sword-shield-extreme/README.md) | 4，`motion/w04.mot` | 1 | 14 |
| [极之型铳枪](../plugins/gunlance-extreme/README.md) | 9，`motion/w09.mot` | 9 | 15 |
| [试制磁斩锤](../plugins/magnet-spike/README.md) | 13，`motion/w13.mot` 和 `motion/w13w.mot` | SDK 类型 16 | 16 |

每招先记录输入及优先级、前置状态、源状态、片段组和槽、起始帧、混合及循环、出判定与音效帧、派生窗口、结束动作。命中、格挡、落地或该层播放到末帧才允许的转移，应注明触发条件。由原生函数补充的分支也要登记，动作表的记录数只说明有多少状态槽。

从 `mhfdat.bin` 取片段、派生和事件记录，从 `mhfsdt.bin` 取猎人近战判定与独立效果判定。转换后对照 `program.rs` 的读取方式。需要定位未接通的分支时，同时查看原版函数和实际表行，沿输入到状态、片段和命中逐段确认。

扩展武器在武器注册器登记类型与风格，再为每件装备分配独立编号，并登记名称、模型与逐级数值。购买、配方和实例均引用装备编号。GU 1.4 的基础表反射 `FAD5C`、等级表反射 `FC668` 和斩味颜色表读取 `F4724` 可用于对照字段。[武器注册接口](../crates/mhgu-sdk/README.md#武器类型)

## 2. 转换资源并校正时序

1. 在 `tools` 的转换器中复用 `mhf-resource` 解包和 `mhgu-resource` 写入。现有武器把上下身的 0/1、2/3、4/5 三组轨道分别合成全身 LMT，保留原组内槽位、空槽和循环点。按男女骨架分别烘焙后写入同一套 LMT。相同片段使用源槽，有差异的女性模型片段写在源槽加 128 的位置。运行时按装备模型选择，解码时恢复源槽号。
2. 核对根轨、坐标和武器挂点事件。先看站立、拔刀、收刀和防御回正，再看活动幅度大的攻击。换手或离背要对齐源动作帧，混合期间也要观察。双剑左右手分别有挂点和接触槽。
3. 用私有路径加载资源，例如 `player/mot/frontier_db`。通过 mhfz-sdk 的 [MotionBanks](../crates/mhfz-sdk/src/motion.rs) 或武器自己的编码器，将源片段映射到 GU 空闲的运行时 bank 和槽。GU 的资源载入和绑定由 mhgu-sdk 提供。播放通过 `action::play_motion` 进入 GU 的层播放函数。
4. 分别标记动作播放帧、逻辑刻和实际经过时间。一刻 FZ 30 Hz 逻辑时间对应两个 GU 60 Hz 时钟单位。表内片段通过 `mhfz_sdk::motion::play_step` 播放。FZ 的非零混合多计最后一个插值刻，混合期间猎人停在起播姿势，最后一刻开始推进。动作游标按 FZ 逻辑刻读取混合倒计时。半速动作要分别验证片段事件帧和独立计时，例如大剑纵斩的蓄力升段。表驱动的特效使用 `MotionClock`，按 FZ `817BD900` 在混合结束后扫描 `[当前姿势帧, 下一播放帧)`。跳入新片段时按起播帧重置游标，循环在取到回绕后的姿势时重新扫描，重复采样跳过已处理的区间。见[动作特效游标](../crates/mhfz-sdk/src/effects.rs)。

资源生成脚本负责组装开发用 `dist`。发布时用 `tools/package_weapon_styles.py` 生成 `common.arc` 及每把武器的私有 ARC。武器的逻辑资源名及 EFL、PEL 引用保留，重复的图集与音频样本由打包器改写到共用路径。源参数最好留在提取的数据或动作程序中，运行时以可辨识的名称取用。新增 GU 派生参数也应注明用途和单位，比如[双剑空中下劈的水平速度](../plugins/dual-blades-extreme/src/program.rs)。

## 3. 接入 GU 的风格和动作生命周期

- 在内容插件 `lib.rs` 定义常驻 `Style`，声明全局唯一的自定义风格编号（10–65535）、GU 武器类型、所需回调和已实现的动作 ID。在 Forge 入口用 `registry::when_ready` 登记初始化函数，由该函数调用 `registry.register(&STYLE, name, description)` 一次，并在成功后绑定预读、联机和手册等回调。见[大剑入口](../plugins/great-sword-extreme/src/lib.rs)和[双剑入口](../plugins/dual-blades-extreme/src/lib.rs)。
- 沿用 V1 注册接口和装备箱选择。配置由注册器按武器保存，未注册、不匹配或无配置时解析为 0。`independent` 让 GU 固定表使用内部索引 0，实际风格由独立描述符决定。切出风格或场景更换时，控制器会收到空输入。旧 actor 为 0 时只清理插件自己的状态。
- 风格动作 ID 为 0–255，编码为 GU 武器组 `16` 的 `0x8000..0x80ff` 槽。与 GU 切招交接用 `action::enter`、`action::transition_to` 等现有接口。启动新招式时可用 `accept_custom_start` 接受 GU 已排定且尚未启动的目标动作。等 GU 接受目标后再更新朝向、建立游标、播放动作并安装命中和派生窗口。
- 从 `Input::command`、`Input::weapon_command` 与原生 `ActionRequest` 取用 GU 已解析的输入。逐项核对按下、持续、松开和组合键优先级。附加肩键读取 `Input::auxiliary_pressed`，由 GU 原生键位查询处理默认 ZR 和交换布局。收刀冲刺的原生过滤可能只保留冲刺键，注册器从 GU 的过滤前按键中恢复附加肩键。`request` 返回已注册的动作 ID 可接手请求。普通武器切招中 `NATIVE_ACTION` 放行原生槽，`PASS` 拒绝本次请求。拔刀路由中的 `PASS` 保留 GU 选中的路由。见[请求解析](../plugins/mhgu-sdk-runtime/src/weapon/style/mod.rs)和[武器路由](../plugins/mhgu-sdk-runtime/src/weapon/style/routes.rs)。
- 固定的输入窗口优先用 `Style::commands` 绑定到 GU 命令表，分别声明接收起点、可执行帧和停止接收帧。GU 正等待 `X+A` 时，`WeaponAfterCombo` 可暂缓单键，`AttackChord` 可接收 GU 的肩键加 `X+A` 组合。发现原版命令抢占时，查清表号及冲突的目标槽，只调整冲突行并保留需透传的地形或狩技行。动态派生由风格选定后交给 GU，一个输入由对应的命令表或风格分支消费一次。见[命令表接口](../crates/mhgu-sdk/src/weapon/style/action/command.rs)。
- 扩展武器在本地角色装备记录上生效时，GU `282DA8` 派发其注册风格，`2E9B7C` 的绑定钩子过滤回退武器的专属行，`2AB670` 的切招钩子只放行已注册的私有动作及 GU 公共动作和狩技。用 [扩展武器控制回放](../plugins/great-sword-extreme/tests/verify_menu.py) 核对输入派发、命令表和斗技场固定配装。
- 转向通过 GU 的方向判定和 `movement::set_facing`、`movement::turn` 更新当前与目标朝向。前后左右派生用 `command_stick_yaw` 读取 GU 的「指定方向类型」和摇杆死区。攻击执行帧的转向用 `attack_turn_yaw`，类型 1 跟随实时相机，类型 2 以猎人自身为基准。持刀冲刺等世界移动继续用 `live_relative_stick_yaw`。若派生有输入缓冲，按该招的源规则保存方向或到执行帧再取方向。见[移动接口](../crates/mhgu-sdk/src/weapon/style/movement.rs)。
- 公共移动、收刀、落地、受击、骑乘和狩技继续走 GU。`art_slots` 配置装备页槽位数，允许出招的窗口提交 `Transition::HUNTER_ART`。武器专属狩技要从本武器原版派发表确认起手、后续动作、槽更新和收尾。通过 `guard::Policy` 交付防御结果，使用 `guard::take` 取得已发生的格挡。风格通过 `StyleRulesV2` 声明格挡半角及其他可选规则。注册器处理 GU 来袭角、防御强化与伤害结算，具体防御窗口、接招和资源收益由风格决定。见[防御接入](../plugins/mhgu-sdk-runtime/src/weapon/style/guard.rs)。

共享派发在读取任务专用字段前先确认角色类型。注册风格后从 `lib.rs` 用 `remote::bind_playback` 登记常驻的 `Playback`。`tick` 回放 GU 接受的私有动作，`observe` 可在公共动作期间推进或清理队友状态，`action_state` 可为原生 `netPlAction` 提供额外状态的编码和解码。注册器检查武器是否有可用位，并按当前 GU 名册、任务猎人类型和远端装备武器定位回放。离场、换武器或复用名册槽时回收该槽的旧状态。见[远端接口](../crates/mhgu-sdk/src/weapon/style/remote.rs)和[收发与派发入口](../plugins/mhgu-sdk-runtime/src/weapon/style/multiplayer.rs)。

## 4. 接入命中、特效、声音和 HUD

| 功能 | 接入方式 |
| --- | --- |
| 武器判定 | 用 [`Attack::submit`](../crates/mhgu-sdk/src/weapon/style/action/attack.rs) 传命中模板、动作值、延迟、持续时间和形状。动作值需在原生构造阶段可读，以便应用武器、装备和狩技修正。GU 负责碰撞及结算。双手同帧判定可用 `Attack::SECONDARY` 分开接触槽。 |
| 模板属性 | HDP 行提供命中属性和形状索引，HTS 提供几何。按招式选择模板与属性。需要沿用空中斩击形状且关闭骑乘时，提交 `Attack::NO_MOUNT` 让注册器在构造前处理本次 HDP 行。 |
| 独立弹波或光波 | 按原版生成帧对齐特效和独立判定。两者采用同一位置及朝向，分清扫过挂点的判定、取样后固定在世界坐标的判定，以及每波的命中编号和斩味处理。 |
| 粒子 | 转换到 GU 原生 EFL/PEL 及配套 MOD/MRL/TEX。统一使用 `effect/frontier_<tag>/` 下的模型路径，`tag` 使用小写字母和数字，共用渲染器自动匹配该目录。FZ 粒子模型的局部旋转按 X、Y、Z 依次应用。以世界坐标发射时，使用 `world_pose_angles` 将取样姿势分解为 GU 发射器的旋转角。随刀移动和旋转的挂点效果在 PEL 中启用关节旋转，左右手使用各自的关节和局部角。风格在动作退出、收刀、换区或远端槽回收时停止仍由该动作负责的持续效果。注册器统一转接 GU 绘制及释放回调。 |
| 音效 | 挥刀随当前动作时间线触发，蓄力、格挡等事件按动作程序触发。命中和弹刀用 GU 的碰撞结果播公共音效。逐个核对发射方向、阶段及同时和逐发模式的请求次数。沿成品 SRQR、SBKR 解码 ADPCM 验证有效信号，并检查循环重复触发和动作打断后的清理。 |
| HUD | 原版狩技继续使用 GU 的槽位和结算。扩展武器可用 [`hud::bind_meter`](../crates/mhgu-sdk/src/weapon/style/hud.rs) 为 GU 任务 HUD 提供分段槽和距离提示。原作有专属图稿时，使用 `MeterSprite` 提供图集帧、锚点和缩放。复用武器专用 HUD 时，新资源放在独立路径，从当前猎人的风格状态取得进度。可参考[双剑 HUD](../plugins/dual-blades-extreme/src/hud.rs)的火焰槽与刃打层数。 |

在注册后通过 `style::preload::bind` 登记预加载回调。GU `29B7D4` 完成任务猎人初始化后，注册器为本地当前风格调用回调。各极型武器可用 `MotionBanks::preload_for_hunt` 预加载男女模型共用的移动、地形和攻击动作库，首次播放时再绑定 GU 的运行时槽。各插件按需在同一回调准备音库、粒子、私有 HTS 和 HUD。私有 HTS 可使用 `CachedHitShape` 持有缓存引用，攻击时取得独立引用，未预加载时仍可按路径加载。见[预加载接口](../crates/mhgu-sdk/src/weapon/style/preload.rs)和[形状缓存](../crates/mhgu-sdk/src/combat/collision/shapes.rs)。

## 5. 按功能验收

新增武器先按下表对照 FZ 源行为确定验收用例，再用该武器打包后的 NRO 和资源跑 GU 对应入口。对按源行为或移植设计不适用的项目，记录依据。用例记录应能区分源表或指令核对、GU 离线回放和实机观察。局部修改按受影响的项目选择验证。

| 项目 | 用新武器确认的行为 |
| --- | --- |
| 注册与资源 | 装备箱可选、切回原版、男女模型、重复初始化及首次出招。追踪实际读取的动作库、音库、粒子表、命中形状和 HUD，检查排队或绑定是否发生在正确阶段。为新极之型编写 `src/manual.txt`，核对猎人笔记和暂停菜单中的页序、翻页和返回。见[手册格式](../plugins/mhgu-sdk-runtime/README.md)。 |
| 输入、位移与结束 | 从 GU 的键位映射走到实际动作，核对组合键优先级、预输入、起手帧、转向和根位移。覆盖拔收刀、翻滚、冲刺、台阶、空中、落地和受击打断，确认回到公共动作后能正常交互。 |
| 判定与状态 | 从出招走到原生命中解算，验证判定形状、帧、动作值、重复命中、斩味、硬直和骑乘。核对格挡或反击窗口、资源与 HUD 更新、狩技续段和中断。将刀身和独立弹波分别验证。 |
| 音效与绘制 | 回放至少一招在空挥、命中及中断时的实际音效和发射事件。按 FZ 源层核对挂点、颜色、Alpha、材质、混合和深度淡化，在 GU 绘制提交中确认参数已生效。检查干地与水面分支以及持续效果的清理。 |
| 切换与联机 | 覆盖切风格、换武器、换区和任务与集会所交接。回放发送、接收和远端角色换槽，核对动作、伤害归属及持续效果清理。 |

组合键回放从完整前置连段进入，保留 GU 的原生命令表。至少比较同次采样、各键错开一次采样、先按住一键再补按、两种松键顺序，以及松开后重新按下。分别在派生窗口开启前、接收与执行之间、关闭前检查动作槽。确认合法的单键或双键派生也能独立完成。有肩键时覆盖两种布局。

打包后运行 `python3 plugins/mhgu-sdk-runtime/tests/verify_weapon_styles.py`。它从 `plugins/*-extreme/Cargo.toml` 发现新武器，缺少验收用例时会报出插件名。共用回放检查风格注册顺序、手册、预读和首次动作库绑定、PEL 到 TEX 的资源引用，以及各模型路径实际经过的共用绘制入口。武器特有的输入和招式走各自的回放。已知故障和可复用用例见[常见故障与排查](weapon-style-troubleshooting.md)。交付前按[文档同步流程](development.md#文档同步)更新对应说明。
