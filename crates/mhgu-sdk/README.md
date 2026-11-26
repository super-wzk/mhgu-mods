# mhgu-sdk

MHGU Switch 1.4 的 Rust 引擎绑定。插件通过路径依赖引用 `mhgu-sdk`，Rust 中使用 `mhgu_sdk`。包为 `no_std`；宿主工具可以使用 `MotionId` 等值类型，ARM32 小端目标还提供运行时绑定。

风格插件需要安装 [mhgu-sdk.nro](../../plugins/mhgu-sdk-runtime/README.md)。Cargo 依赖提供 Rust 类型和调用接口，进程内的版本化服务由该 NRO 提供。运行时可先于或晚于内容加载。

## 武器风格插件

[mhfz-sdk](../mhfz-sdk/README.md) 在本 SDK 的 GU 接口上提供极型武器共用的 Frontier 动作、输入及表现逻辑。

[StyleRegistry](src/style/registry.rs) 在 GU 游戏线程访问风格注册器。内容插件在 Forge 入口调用 `registry::when_ready`，在回调中执行一次 `registry.register(&STYLE, name, description)`。常驻的 `Style` 描述符提供动作编号、狩技槽位数及可选的请求、更新和防御规则。`Style.id` 是从 10 起分配的自定义风格编号，角色存档和联机共用。用 `style::selected(weapon)` 查询启用的自定义风格，返回 `None` 时沿用 GU 原版。实际用法见[极型大剑](../../plugins/great-sword-extreme/src/lib.rs)。

GU 从武器动作组派发已登记的招式编号（0–255）。`Input` 提供 GU 已解析的公共和武器指令，`request` 处理 GU 已接受的拔刀和武器切招请求。公共动作可返回 `PASS` 交给 GU。空输入通知插件结束，actor 为 0 时代表旧猎人已失效。

[input](src/style/input.rs) 读取 GU 的组合键状态、过滤后的 X／A 按下沿和当前附加肩键。通过 [movement](src/style/movement.rs) 查询摇杆方向、应用朝向和使用原生耐力入口。回避准入复用 `has_evade_stamina` 检查体术、料理和免耗效果。读取任务专用字段前，用 [hunter::is_quest](src/style/hunter.rs) 核对角色类型。

[preload](src/style/preload.rs) 登记本地风格在 GU 完成任务猎人初始化后执行的资源准备回调。[MotionBanks](src/style/action/motion_bank.rs) 缓存男女模型共用的 LMT。首次播放时才绑定运行时 bank，按装备模型选择基础片段或槽号加 128 的专用片段。私有 HTS 可通过 [CachedHitShape](src/combat/collision/shapes.rs) 复用已加载资源。

[remote](src/style/remote.rs) 提供 GU 名册查询和远端状态存储。风格注册成功后，用 `bind_playback` 一次提交常驻的 `Playback` 描述符，包含动作回放 `tick`、可选的逐帧观察 `observe` 和动作包状态 `action_state`。注册器校验当前武器的动作包是否有可用位，校验通过后才登记整份描述符。插件定义状态值及回放逻辑。读取远端装备前按当前名册核对猎人，角色离场、换武器或槽位更换时释放旧状态。

使用 GU 公共收刀的风格可通过 `bind_sheathe` 登记常驻 `Sheathe` 描述符，提供冲刺动作 ID 和两种片段的显式编码请求。注册器把收刀变体加到原生动作包，并将接收到的变体交给风格的动作编码回调。

[physical](src/style/physical.rs) 提供统一的物理攻击力回调登记。回调可限定在 GU 的左右刀身碰撞槽，也可覆盖同一猎人的独立武器碰撞。风格插件负责数值与生效条件。

[rules](src/style/rules.rs) 为已注册的风格提供可选的格挡半角、公共动作结束条件、弹刀和镜头回调。[particle](src/style/particle.rs) 提供模型粒子绘制与释放回调的登记接口。

`Style::commands` 可在 GU 装载命令表时追加一组有序的武器派生，由 GU 缓冲先接收的输入并在指定帧切换动作。`action::enter` 走 GU 的公共动作切换。已判定的单条派生可通过 `action::transition` 提交，狩技使用 `Transition::HUNTER_ART`。`action::play_motion` 使用猎人的原生动作编码。`Attack::submit` 把动作值和形状交给 GU 的攻击构造器及武器修正。注册器在大剑的首个有效攻击事件处理兽魂附身。`guard::Policy` 提供一次格挡结果，注册器负责把结果交给 GU 的受击、斩味和回正流程，内容插件可以用 `guard::take` 进入后续招式。
