# 四种极之型的实现与回归

本文记录当前 MHGU 1.4 移植的行为、实现入口和回放用例。玩家操作表、完整招式窗口和源数据细节以各武器 README 为准。构建和选择离线用例见[开发指引](development.md)，新增风格见[移植流程](weapon-style-porting.md)。

## 共用运行时

| 主题 | 当前实现 | 修改时核对 |
| --- | --- | --- |
| 风格选择与原版隔离 | `[Weapons]` 按 GU 装备武器编号保存风格。`0` 回到 GU 原版。注册器只对本地任务猎人启用当前风格的武器控制、命令、预加载及状态发送。退出控制器时传空输入，旧猎人已失效时再将 actor 置零。 | 验证同武器切回原版的装备箱、狩技、HUD 与攻击，也验证任务 → 集会所 → 任务。集会所角色共享 `282DA8` 派发入口。见[注册器运行时](../plugins/style-registry/src/runtime.rs)。 |
| 肩键、踢键与组合键 | 附加肩键用 GU `65FA04` 解析，默认 ZR，交换布局后跟随 GU。奔跑时从原生过滤前的按键恢复附加肩键。聊天钩子只暂时屏蔽 UI 的快捷键按下位，手动菜单和触屏仍可打开聊天。收刀踢击走 GU，持刀时按各风格的特殊攻击键派生。 | 用默认和交换布局分别检查站定、普通奔跑、背对怪物逃跑冲刺及滞空。组合键要试同时按下、先按 X／A 再补另一键，以及 R 与 X+A 重叠。见[输入接口](../crates/mhgu-sdk/src/style/input.rs)和[聊天钩子](../plugins/style-registry/src/runtime/chat.rs)。 |
| 预输入与转向 | GU 命令表负责固定窗口，FZ 动作游标负责武器动态派生。预输入、可执行帧与关闭帧独立，接招后写入当前与目标朝向。斩斧在 GU 已判定摇杆有方向时使用实时镜头方位，允许缓冲转向的招式可在回中后沿用排队期间的最后有效方向。 | 核对 GU 先选中的表号、目标动作组与槽、风格排队动作及执行帧。方向同时记录当时镜头、摇杆和猎人朝向，连测推住、转相机、回中和重新推杆。见[命令表钩子](../plugins/style-registry/src/runtime/commands.rs)和[移动接口](../crates/mhgu-sdk/src/style/movement.rs)。 |
| 防御和回避技能 | `mhfz-sdk` 给 FZ 防御窗口应用 GU 防御性能档位，+1／+2 分别延长 2／4 个 GU 时钟单位。可受回避性能加成的动作事件加 6／10 个单位。注册器保留 GU 的攻击排除与防御强化条件，风格通过 `StyleRulesV1` 声明补判半角。 | 窗口、正背面、技能资格、成功后的无敌计时和防御斩味消耗分别测。FZ 专属装备效果与特殊攻击类型按当前适配范围处理，例如太刀事件 18 的 FZ 专属装备加成没有接入。见[技能帧数](../crates/mhfz-sdk/src/combat.rs)和[GU 格挡转接](../plugins/style-registry/src/runtime/guard.rs)。 |
| 弹刀与命中停顿 | 注册器按猎人、动作和刀身接触序号监听命中。`mhfz-sdk` 提供 FZ 肉质边界，GU 先应用斩味和相关原生豁免，风格再选反弹动作及特殊招式的豁免。停顿按招式选择 GU 模板或独立判定数据。 | 测首次硬部位接触、同招后续接触、左右刀身、豁免招式和被打断。检查全停与减速两个原生计时器。见[弹刀钩子](../plugins/style-registry/src/runtime/deflection.rs)和[刀身接口](../crates/mhgu-sdk/src/style/action/melee.rs)。 |
| 资源预加载 | 在 GU `29B7D4` 后调用本地当前风格的回调，缓存当前模型的三组 LMT，再按需缓存音库、PEL、私有 HTS 和 HUD。首次播放才绑定动作库。 | 记录准备与首次播放两阶段的加载路径，对照男女模型、已占用的原生 bank、重复初始化、未选风格和远端角色。见[共用武器验收](../plugins/style-registry/tests/verify_weapon_styles.py)。 |
| 联机动作与资源 | GU `netPlAction` 同步武器动作，接收端按发送者的装备和名册槽播放。大剑同步蓄力档位，双剑同步鬼人阶段和满火位，斩斧同步亮瓶位和放出档位。太刀根据接收的动作槽和片段回放，刀身、瞬斩、冲击波和追击分别保留命中归属。 | 房主装备其他武器、两个队员同时行动、换装、换人和换区都要覆盖。额外状态仅在注册器核对过的武器上使用原生包可用位。离线覆盖动作包、回放和伤害归属，双机任务另行验证。见[远端接口](../crates/mhgu-sdk/src/style/remote.rs)和[动作包钩子](../plugins/style-registry/src/runtime.rs)。 |

## 动作特效与材质

- 动作事件用 `MotionClock` 在混合结束后扫描 `[当前姿势帧, 下一播放帧)`，按源记录顺序发射。检查非零起播帧、半速播放、停顿、回绕及同一姿势重复取样。蓄力、耐力等逻辑计时另按其实际经过量更新。见[动作特效游标](../crates/mhfz-sdk/src/effects.rs)。
- 转换器按 FZ 局部 X、Y、Z 顺序生成可旋转的模型粒子。世界粒子用 `world_pose_angles` 将取样姿势拆成 GU 所需的角度，随武器移动的效果启用 PEL 的关节旋转。测试时至少对比两个猎人朝向，分别看发射位置、刀身是否移动、角度是否跟随和当前关节是否可用。见[斩斧挥斩和挂点](../plugins/switch-axe-extreme/src/fx.rs)和[双剑刀光](../plugins/dual-blades-extreme/src/fx.rs)。
- 亮度按粒子层核对颜色、源 Alpha、MRL 技法、混合及叠加次数。大剑蓄力和升段闪光的 m66 层使用 t3 图集的原始 Alpha 和源 Alpha 加法混合。`mhfz-sdk` 的场景深度淡化在支持的粒子层使用两次 GU 原生深度测试近似 FZ 淡化。检查淡化时也要逐层对比颜色和混合。注册器在模型粒子绘制结束时恢复 GU 绘制状态。见[大剑材质回放](../plugins/great-sword-extreme/tests/verify_native_material.py)、[FZ 模型适配](../crates/mhfz-sdk/src/effects/model.rs)和[GU 绘制转接](../plugins/style-registry/src/runtime/effect.rs)。
- 持续效果按动作和猎人槽跟踪，取样型世界效果保留发射时姿势。收刀、受击、换区和名册更换时分别核对退出回调。粒子由 GU 先释放时，通过释放回调移除保存的句柄。见[粒子接口](../crates/mhgu-sdk/src/style/particle.rs)。

## 大剑

| 改动 | 行为与回放 |
| --- | --- |
| 纵斩蓄力与四蓄上挑 | 纵斩从事件 30 起半速播放，事件本身计入第一刻。无技能／集中／杂念的第一段分别为 40／32／48 个 FZ 刻。持刀翻滚第 30 帧起收 X 或 X+A，第 44 帧起执行。GU 等待组合键时暂缓单键纵斩，侧滚后仍可接四蓄上挑。见[战斗控制](../plugins/great-sword-extreme/src/battle.rs)和[原生输入及蓄力回放](../plugins/great-sword-extreme/tests/verify_style.py)。 |
| 精准防御与辉烈剑 | 大剑精防在低耐力时仍可按 GU 可防性挡下攻击。防御动作第 56 帧起可滚，成功后 X 接下劈、A 接横扫。三次怪物格挡可发动辉烈剑，首波从挥剑片段第 84 帧产生，连续 17 波，各自固定发射位置，攻击者停顿为零。见[大剑说明](../plugins/great-sword-extreme/README.md)和[资源回放](../plugins/great-sword-extreme/tests/verify_assets.py)。 |
| 原生狩技与联机 | 下一击强化在 GU 首次有效大剑攻击构造时消费。队员蓄力按各自收到的动作及档位结算，独立光波继续检查所属猎人。见[武器原生接入](../plugins/style-registry/src/runtime/weapon.rs)和[联机回放](../plugins/style-registry/tests/verify_gs_db_multiplayer.py)。 |

## 双剑

| 改动 | 行为与回放 |
| --- | --- |
| 刃打层数 | 最多四层，每层增加 5% 刀身物理攻击。全停与减速命中停顿期间暂停更新。武器连段内计时到零时保留一刻，退出连段清层数，收刀同时清计时。站定刃打第 50 帧可接 A 对应招式。空舞连斩的冲刺入口为双推后按 A。见[刃打状态](../plugins/dual-blades-extreme/src/battle/sharpen.rs)和[弹刀与刃打回放](../plugins/style-registry/tests/verify_deflection.py)。 |
| 回避、空舞与骑乘 | 持刀翻滚第 10 帧起可预输入 X，前滚第 44 帧、侧滚第 42 帧执行。极鬼人滞空的 X+A 及 R 加 X+A 都可接空舞连斩。空舞连斩及命中后 A 终结关闭模板的骑乘值，普通滞空 X 的空舞终结保留骑乘值。见[双剑战斗控制](../plugins/dual-blades-extreme/src/battle.rs)和[输入回放](../plugins/style-registry/tests/verify_deflection.py)。 |
| 真鬼人、极鬼人与满火 | 片段 1448 分别触发解放爆光、刀光及干地／水面效果。左、右刀光路由使用各自旋转和关节，刀轨各自取装备刀模长度。真鬼人升极鬼人继承火焰槽，满火时切换颜色。见[双剑特效](../plugins/dual-blades-extreme/src/fx.rs)和[双手刀光回放](../plugins/style-registry/tests/verify_deflection.py)。 |
| 兽魂附身【饿狼】 | 通过 GU 生成左右手延迟追击。每只手按追击接触序号，在首次有效怪物命中时回复 1 点体力。原挥击及重复接触无回血，狩技到期后停止生成新追击。见[饿狼追击](../plugins/dual-blades-extreme/src/battle/maw.rs)和[狩技回放](../plugins/style-registry/tests/verify_weapon_arts.py)。 |

## 太刀

| 改动 | 行为与回放 |
| --- | --- |
| 气刃派生与跑蓄命中 | 短按 R 接气刃斩，长按 R 在练气与招式窗口符合条件时出放出斩，缺放出行时退回普通气刃派生。跑蓄后接气刃一段时，刀身判定等到实际片段的打击起始帧再提交，防止 GU 碰撞时钟先走完。持刀翻滚第 6 帧起预输入，第 36 帧执行先接受的派生。见[太刀战斗控制](../plugins/long-sword-extreme/src/battle.rs)和[动作回放](../plugins/long-sword-extreme/tests/verify_port.py)。 |
| 化解与瞬斩 | 化解支持收刀站定、走路和两类冲刺，防御性能延长窗口，防御强化按 GU 可防性生效。化解成功后 X 接瞬斩，A 接反击突刺。瞬斩的路径判定和刀身独立，刀身挥空时路径仍可命中。见[瞬斩判定](../plugins/long-sword-extreme/src/instant.rs)和[原版对照回放](../plugins/long-sword-extreme/tests/verify_fidelity.py)。 |
| 练气、斩槽与狩技 | 上方练气槽 0–100，下方斩槽 0–850。樱花气刃斩在原生升色事件改加 425 斩槽，上限 850，维持无色。练气解放圆月斩期间极型招式也不消耗练气。解放连击由斩槽分为三档，按首次有效命中记录大型怪物部位以生成追击。见[太刀说明](../plugins/long-sword-extreme/README.md)和[狩技回放](../plugins/style-registry/tests/verify_weapon_arts.py)。 |
| 拖刀、放出与解放特效 | 跑蓄的三处光跟随刀身握刀关节，等关节有效后开始；地面效果采样已计算的动作根节点，出斩、回避或打断时清理。放出各段蓄力光取动作根节点。解放连击从同片段的不同帧进入并按实际区间播放，各档的特效、刀身和怪物追击独立。见[太刀特效](../plugins/long-sword-extreme/src/fx.rs)和[原版对照回放](../plugins/long-sword-extreme/tests/verify_fidelity.py)。 |

## 斩斧 F

| 改动 | 行为与回放 |
| --- | --- |
| 双槽、自然回复与 HUD | 斩击槽使用 GU w08 原生字段，进任务 50／100，集中、杂念和能量累积走原生加值。光剑待机与持刀移动也推进斩击槽自然回复，通常累计约 5 秒回 5 点。光剑槽独立，进任务 150／300；当前加值入口是斧身命中、成功吸收怪物攻击与剑鬼生效。HUD 从实际两槽值计算填充，等待贴图及原生节点就绪，退出风格时恢复原生显示。见[斩击槽与狩技](../plugins/switch-axe-extreme/src/battle/arts.rs)、[光剑槽与动作](../plugins/switch-axe-extreme/src/battle.rs)和[双槽 HUD 回放](../plugins/switch-axe-extreme/tests/verify_hud.py)。 |
| 剑鬼与能量累积 | 在狩技生效帧撤销 GU 的剑鬼原生回满，保留已有斩击槽并按技能回复基础 25 点，光剑回满至 300 并直接展开。剑鬼每跳扣 3 点光剑槽，三档的重复间隔为 63／72／63 个 GU 时钟单位，挥剑免除同槽的招式扣除。能量累积 I／II／III 的基础回复量为 75／100／100，作用于斩击槽。光剑攻击仍使用 GU 瓶种和剑鬼等级的命中修正。见[狩技实现](../plugins/switch-axe-extreme/src/battle/arts.rs)和[狩技与瓶倍率回放](../plugins/style-registry/tests/verify_weapon_arts.py)。 |
| 形态、拔刀与蓄力 | 持刀特殊攻击键在光剑满槽时解锁，解锁后可变形。收刀 X+A 走光剑拔刀或满槽解锁。收刀站定且摇杆回中时新按 R 可拔斧蓄力，推杆按 R 保留奔跑。光剑槽耗尽后等待允许切招的地面动作再播放 1441 变斧，武器收刃轨道使用 64～76 帧。单独耗尽斩击槽时，仍有光剑槽的光剑可继续使用。见[武器轨道](../plugins/switch-axe-extreme/src/motion.rs)和[按键回放](../plugins/switch-axe-extreme/tests/verify_stance.py)。 |
| 袈裟斩、放出与吸收 | 光剑待机 X+A 在任意方向接放出突刺。允许袈裟斩的连段按左、右、后分支和剩余槽量判定，普通突刺第 70～113 帧也可取消接袈裟斩。袈裟斩接吸收从第 38 帧收新的 ZR 按下，之后按怪物接触类型、前置状态、槽量和有效方向派生。强化放出按 0–1、2–3、4 次及以上有效命中分三档，重新起手清计数。见[动作与派生](../plugins/switch-axe-extreme/src/battle.rs)和[原版派生回放](../plugins/switch-axe-extreme/tests/verify_branches.py)。 |
| 冲刺、连挥与回避 | 斧冲刺用斩击槽，光剑冲刺用光剑槽。斧冲刺可暂时展开武器模型，越阶后按原冲刺形态从循环段续跑，不重复起跑耗槽。两形态在冲刺或刹车可按 GU 收刀键。斧连挥扣耐力，光剑乱斩扣光剑槽，两者抑制 GU 自然回耐，收招后恢复。见[连挥与方向回放](../plugins/switch-axe-extreme/tests/verify_chains.py)和[地形与狩技路由回放](../plugins/switch-axe-extreme/tests/verify_routes.py)。 |
| 光剑弹刀、瓶光与收刀 | 已命中硬部位且斩味等级低于 2 时，普通光剑刀身可弹刀。放出等使用 FZ 豁免表。GU 六种装备瓶映射成四种 FZ 瓶光，持续光层跟随武器节点移动并旋转。收刀、受击或换区按各自的所属动作回收持续层。冲刺收刀使用片段 1499 的动画、音效和粒子。普通收刀片段 1003／1009 的 34 层闪光仍停用，保留收刀动画、音效和地面效果。见[瓶光跟随回放](../plugins/switch-axe-extreme/tests/verify_phial_follow.py)和[斩斧说明](../plugins/switch-axe-extreme/README.md)。 |

前滚第 30 帧起接受攻击预输入，第 50 帧起执行。光剑 X 接纵斩，A 接二连斩，再按 A 可接四连斩后两刀。两条连段的放出突刺和方向加 X+A 袈裟斩要在各自窗口验证。前滚第 10 帧起可预输入双推冲刺，第 50 帧起还可接吸收，斧形态可按住 R 接蓄力。见[地面输入回放](../plugins/switch-axe-extreme/tests/verify_stance.py)。

斩斧的六种袈裟斩使用 GU 通用斩斧模板，攻击者停顿系数按 FZ 袈裟斩与斧纵斩的比例缩为三分之一并四舍五入。用同一怪物部位分别测试袈裟斩和普通纵斩，区分 GU 的减速和全停。光剑回避斩、太刀回避斩及双剑鬼人回避的保护各以该招的实际事件为准，排查时记录无敌计时器与命中帧。见[斩斧战斗控制](../plugins/switch-axe-extreme/src/battle.rs)和[斩斧命中停顿回放](../plugins/switch-axe-extreme/tests/verify_branches.py)。

## 选择回归入口

| 修改范围 | 从仓库根目录运行 |
| --- | --- |
| 共用输入、装备箱保存和公共动作 | `python3 plugins/style-registry/tests/verify_chat_shortcut.py`、`python3 plugins/style-registry/tests/verify_kick.py`、`python3 plugins/style-registry/tests/verify_ground_recovery.py`、`python3 plugins/great-sword-extreme/tests/verify_menu.py` |
| 大剑、双剑的命中与刀光 | `python3 plugins/great-sword-extreme/tests/verify_style.py`、`python3 plugins/style-registry/tests/verify_deflection.py`。粒子转换另跑大剑的 `verify_native_fx.py` 和 `verify_native_material.py`。 |
| 狩技结算、槽量与技能 | `python3 plugins/style-registry/tests/verify_weapon_arts.py`，会加载斩斧、太刀与双剑并回放原生路径。 |
| 太刀动作、特效和 HUD | `python3 plugins/long-sword-extreme/tests/verify_port.py`，其中的 `verify_fidelity.py` 回放也执行 HUD 检查。 |
| 斩斧动作、瓶光、资源和场景 | `python3 plugins/switch-axe-extreme/tests/verify_port.py`，入口覆盖按键、连挥、瓶光、地形、HUD 与四武器预加载。集会所切换另跑 `python3 plugins/switch-axe-extreme/tests/verify_lobby.py`。 |
| 多人动作和伤害归属 | `python3 plugins/style-registry/tests/verify_gs_db_multiplayer.py`、`python3 plugins/long-sword-extreme/tests/verify_multiplayer.py`，以及斩斧 `verify_port.py` 内的队员回放。 |

卡顿、闪退和特效错位需要实机复现确认。记录模组版本及 NRO 哈希、武器模型、地图区域、是否联机、当前动作组和槽，以及触发前后的输入。对收刀闪退同时记录是否展开光剑、是否仍在冲刺，以及日志里的异常模块。离线模拟通过只说明所运行场景的原生函数和回调符合断言。
