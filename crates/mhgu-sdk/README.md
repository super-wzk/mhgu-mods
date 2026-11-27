# mhgu-sdk

MHGU Switch 1.4 的 Rust 引擎绑定。插件通过路径依赖引用 `mhgu-sdk`，Rust 中使用 `mhgu_sdk`。包为 `no_std`；宿主工具可以使用 `MotionId` 等值类型，ARM32 小端目标还提供运行时绑定。

风格插件需要安装 [mhgu-sdk.nro](../../plugins/mhgu-sdk-runtime/README.md)。Cargo 依赖提供 Rust 类型和调用接口，进程内的版本化服务由该 NRO 提供。运行时可先于或晚于内容加载。

## 武器类型

用 `WeaponId` 登记武器类型，用 `EquipmentId` 登记一件可制造的武器。磁斩锤类型是 16，试制磁斩锤装备是 `0x1001`。`weapon::WeaponEquipment` 是武器专用数据结构，防具和护石继续使用 GU 原版装备系统。同一类型的多件武器共用风格，每件都有自己的名称、模型和等级表。武器插件通过 `weapon::registry::when_ready` 初始化，调用 `register(&WEAPON, &STYLE, weapon_name, style_name, description)`，风格的 `weapon` 填武器类型的 `style_key()`。购买和生产类别显示 `weapon_name` 和 `Weapon.icon` 指定的图标。

自定义图标通过 `register_icon(slot, texture)` 把 128–255 的全局槽位绑定到一张独立的 64×64 GU TEX。路径相对于 `nativeNX`，使用 GU 反斜杠且省略 `.tex`。同一类型的装备均继承 `Weapon.icon`，运行时按各自的稀有度应用 GU 颜色。原生图集槽位仍可直接使用。

`register_equipment(&EQUIPMENT, LEVELS, name, description, model)` 接收常驻数据。`WeaponEquipment` 对应 GU 基础表中的稀有度、两路属性或异常类型、固有技能号、音效请求号和打击音色。`EquipmentModel` 指定 ARC 和 MOD 的无扩展名资源路径。每个 `EquipmentLevel` 对应一个强化等级，依次声明攻击、防御、会心、属性值、孔数、斩味表编号和斩味等级。第 0 行为游戏里的 Lv.1，最后一行决定强化上限。

GU 原生固有技能由 `mSpecialSkill` 选择。属性、异常和固有技能可以由游戏的命中与装备逻辑消费。音效请求号、音色目前可通过装备定义查询。自定义招式的刀光、粒子和时间点由风格的动作事件提供。

在游戏线程用 `registry.equipment(id)`、`registry.level(id, upgrade)`、`registry.model(id)` 查询注册值。用 `grant(equipment_id)` 发放实例，`upgrade(instance_id)` 提升强化等级，`equipped_instance()` 查询当前实例。存档仅保存装备 ID、实例号和强化等级。[磁斩锤注册示例](../../plugins/magnet-spike/src/lib.rs)

在同一次初始化回调中，给已注册的装备登记获取途径。三类登记相互独立，每把武器可有一条购买价格、一条制造配方，以及每个起始等级一条强化配方。商店购买与工房制造会生成一个未强化实例。强化配方的起始等级从 0 计，对应游戏显示的 Lv.1。工房首页的「扩展装备强化」从装备格中读取所选实例，按其实有等级查询配方。

```rust
use mhgu_sdk::weapon::trade::{self, Material, Recipe};

trade::register_purchase(EQUIPMENT.id, 1800)?;
trade::register_recipe(EQUIPMENT.id, Recipe {
    zenny: 1200,
    materials: [
        Material { item: 100, quantity: 3 },
        Material::EMPTY,
        Material::EMPTY,
        Material::EMPTY,
    ],
})?;
trade::register_upgrade(EQUIPMENT.id, 0, Recipe {
    zenny: 2000,
    materials: [
        Material { item: 101, quantity: 2 },
        Material::EMPTY,
        Material::EMPTY,
        Material::EMPTY,
    ],
})?;
```

材料填写 GU 道具编号，每项数量为 1–255，同一配方内编号不重复。金额为 0–9,999,999z。运行时在确认页用游戏的道具表检查编号，同时检查角色的余额、持有量和装备箱容量。

## 武器风格插件

[mhfz-sdk](../mhfz-sdk/README.md) 在本 SDK 的 GU 接口上提供极型武器共用的 Frontier 动作、输入及表现逻辑。

[StyleRegistry](src/weapon/style/registry.rs) 在 GU 游戏线程访问风格注册器。内容插件在 Forge 入口调用 `registry::when_ready`，在回调中执行一次 `registry.register(&STYLE, name, description)`。常驻的 `Style` 描述符提供动作编号、狩技槽位数及可选的请求、更新和防御规则。`Style.id` 是从 10 起分配的自定义风格编号，角色存档和联机共用。用 `style::selected(weapon)` 查询启用的自定义风格，返回 `None` 时沿用 GU 原版。实际用法见[极型大剑](../../plugins/great-sword-extreme/src/lib.rs)。

注册风格可通过 `weapon::style::input::bind_filter` 修改本地任务猎人的按键副本。回调在 GU 屏蔽菜单输入后、解析狩技及公共指令前运行。修改 `Buttons` 可阻止 GU 提前消费组合中的单键。未注册此回调的风格使用 GU 的正常输入路径。

GU 从武器动作组派发已登记的招式编号（0–255）。`Input` 提供 GU 已解析的公共和武器指令，`request` 处理 GU 已接受的拔刀和武器切招请求。公共动作可返回 `PASS` 交给 GU。空输入通知插件结束，actor 为 0 时代表旧猎人已失效。

[input](src/weapon/style/input.rs) 读取 GU 的组合键状态、过滤后的 X／A 按下沿和当前附加肩键。通过 [movement](src/weapon/style/movement.rs) 查询摇杆方向、应用朝向和使用原生耐力入口。回避准入复用 `has_evade_stamina` 检查体术、料理和免耗效果。需要消耗强走时间的招式用 `spend_stamina` 先结算 GU 的单次耐力消耗，再以 60 Hz 帧数缩短强走药和强走旋律。读取任务专用字段前，用 [hunter::is_quest](src/weapon/style/hunter.rs) 核对角色类型。

风格招式可通过 `movement::grant_evade_invulnerability` 延长 GU 受击检查使用的回避计时。时长以 GU 帧为单位，由招式根据当前技能和动作事件计算。

私有空中招式通过 [action::start_airborne_weapon_state](src/weapon/style/action.rs) 进入 GU 空中移动。用 [movement::motion_frames](src/weapon/style/movement.rs) 读取包含移动倍率的时间增量，再将每 60 Hz 帧的速度和加速度交给 `advance_airborne`。该入口调用 GU 的位移积分，叠加本次更新的前向和竖直距离，再检查墙体和落地。传 `Some(0.)` 使用脚底落地检查，也可以提供其他接触距离或传 `None` 延迟检查。着地高度由 GU 的地形探测决定。返回更新后的速度、阻挡和着地结果，供风格选择后续动作。

私有招式进入地面动作时，`action::start_weapon_state` 按 GU 地面动作的规则清除空中标志。可在动作更新中通过 [movement](src/weapon/style/movement.rs) 叠加本帧的有符号世界距离。`displace_forward` 沿猎人当前朝向移动，`displace_vertical` 可在起手尚未切入空中模式时叠加竖直位移。GU 随后继续结算原有动作根轨和地形接触。

独立近战和世界判定可用 [bind_strike_contact](src/weapon/style/action/melee.rs) 订阅本地命中。GU 的伤害入口确认对应武器判定后，回调提供动作值、会心倍率和可选的怪物出生标识。SDK 检查怪物存活且和猎人在同一区域后，才提供怪物类型、配置 ID 和复活序号，可复制后用于按出生实例记录积蓄。用 [bind_monster_strikes](src/weapon/style/action/melee.rs) 可订阅当前客户端所控制怪物的本地与队友命中。按出招者的风格匹配回调，本地猎人可以装备任意武器。本地任务猎人初始化时收到清理回调，可重置上一场任务的累计数据。

[HurtScene](src/combat/collision/hurt.rs) 在碰撞回调中提供当前任务等级和按 GU 怪物表顺序迭代的同区活体怪物。借用的 `Monster` 可查询其出生标识、本地是否控制该怪物、当前启用的受击关节及关节的世界坐标，也可用 `overlaps` 查询世界坐标球是否接触其受击体积。用 `emit_joint_particle` 可在当前怪物关节上发射按世界方向定向的粒子，由 GU 原生模型父节点维持挂载。用 [observe_hurt](src/weapon/style/world_contact.rs) 可在当前猎人装备其他武器时继续观察碰撞通行，并查询本地或复制的受制动作。

可对借用的 `Monster` 调用 [try_ground_hold](src/combat/collision/hurt/ground_hold.rs)，为本机控制且符合条件的地面怪物请求一段 GU 原生受制动作。联机任务须有活动的 GU 怪物动作同步流程，时长以整数 GU 帧同步。返回的 `GroundHold` 在游戏线程上重新校验怪物出生身份和当前动作。可通过 `observe_ground_hold` 借用本地或复制的现有受制，查询是否仍在受制和怪物的位置。控制权变化后句柄随同一出生实例查询当前控制成员，只有当前控制端可提前结束计时。受制结束后的效果可用 `resolve` 在当前 `HurtScene` 查询同区存活的原目标。

[preload](src/weapon/style/preload.rs) 登记本地风格在 GU 完成任务猎人初始化后执行的资源准备回调。[MotionBanks](src/weapon/style/action/motion_bank.rs) 缓存男女模型共用的 LMT。首次播放时才绑定运行时 bank，按装备模型选择基础片段或槽号加 128 的专用片段。私有 HTS 可通过 [CachedHitShape](src/combat/collision/shapes.rs) 复用已加载资源。

扩展武器风格可用 [`hud::bind_meter`](src/weapon/style/hud.rs) 登记任务内分段槽。回调提供余量、每格容量、格数、可选距离提示及最多两条附加进度。SDK 在原生斩味指示旁绘制一至八格。提供 `MeterSprite` 时按武器图集绘制，可相对斩味指示或屏幕中心定位。可选的 `MeterOverlay` 可给槽首和各个已充满的格子提供贴图帧，用于消耗与回充提示。可选的 `MeterPanel` 用于显示武器临时状态，回调提供静态 UTF-8 标题和最多两条进度。武器插件管理动画时钟，SDK 通过 GU HUD 绘制帧。回调参数用于匹配当前状态的猎人，怪物位置及动作数据应在所属场景回调中采样。

[remote](src/weapon/style/remote.rs) 提供 GU 名册查询和远端状态存储。风格注册成功后，用 `bind_playback` 一次提交常驻的 `Playback` 描述符，包含动作回放 `tick`、可选的逐帧观察 `observe` 和动作包状态 `action_state`。注册器校验当前武器的动作包是否有可用位，校验通过后才登记整份描述符。插件定义状态值及回放逻辑。读取远端装备前按当前名册核对猎人，角色离场、换武器或槽位更换时释放旧状态。用 `send_monster_state` 可发布由本机控制的同区怪物状态及两个附加数据字，用 `bind_monster_state` 订阅同区控制端发来的出生实例、来源和快照。用 `send_monster_exchange` 和 `bind_monster_exchange` 在已注册武器的猎人与对应怪物控制端之间发送带序号的请求、确认和结束。SDK 核对 GU 房间成员、所在区域、当前装备、控制权与怪物出生实例。

使用 GU 公共收刀的风格可通过 `bind_sheathe` 登记常驻 `Sheathe` 描述符，提供冲刺动作 ID 和两种片段的显式编码请求。注册器把收刀变体加到原生动作包，并将接收到的变体交给风格的动作编码回调。

[physical](src/weapon/style/physical.rs) 提供统一的物理攻击力回调登记。回调可限定在 GU 的左右刀身碰撞槽，也可覆盖同一猎人的独立武器碰撞。风格插件负责数值与生效条件。

扩展武器可用 [melee::Strike](src/weapon/style/action/melee.rs) 提交斩击或打击，提供动作值、命中窗口、眩晕积蓄、命中停顿和一组已加载的 HTS。SDK 通过 GU 构造命中，在可用的两槽中保留重叠窗口。已创建的扩展近战窗口随场景时间到期，跨动作保留剩余时间。同一 HTS 组的多个形状共用一个命中编号。HTS 可以使用 `0x4000 + 关节编号` 锚定猎人的主武器部件，GU 在每次展开时读取对应骨骼矩阵。

设置 `Strike::deflectable` 后，已登记的 `Rules::deflection` 会处理匹配的近战接触。运行时检查 GU 的心眼等豁免，风格控制器用 `melee::take_deflection` 取得结果并切换动作。

与猎人动作独立的命中可在 [world_contact](src/weapon/style/world_contact.rs) 的碰撞回调中提交。用 [ContactPool](src/combat/collision/mod.rs) 保持接触对象地址稳定，由 `prepare_world_strike` 生成 GU 接触对象，再用 `submit_world_capsule` 提供其固定世界坐标。GU 按场景时钟更新命中窗口。风格在切换区域或失去有效的猎人时停用其接触对象。[强化磁冲击示例](../../plugins/magnet-spike/src/shock.rs)

[world_contact::bind_hurt](src/weapon/style/world_contact.rs) 可为当前本地任务猎人的风格登记受击检测回调。回调通过 [HurtScene](src/combat/collision/hurt.rs) 使用 GU 的地形、受击形状展开和相交函数，适合磁力标记一类只需要接触位置的效果。命中返回可跨帧保存的部位标记。在猎人动作回调中可用 `Target::position_for` 查询，SDK 会重新检查怪物身份、区域和部位，再解析当前骨骼位置。

[world_contact::bind_attack](src/weapon/style/world_contact.rs) 在 GU 展开攻击形状后、伤害结算前派发。回调可通过 [AttackScene](src/combat/collision/attack.rs) 用世界坐标球查询当前任务猎人收到的怪物来袭攻击。判定球的位置、半径和时限由风格提供，原版攻击继续进入原有伤害结算流程。

独立武器可用 [melee::bind_strike_contact](src/weapon/style/action/melee.rs) 监听本地命中。运行时在 GU 结算会心后按猎人、碰撞槽和原生命中编号匹配近战。场景命中可登记 `bind_world_strike_power`，在自己的接触池中核对当前有效的接触对象并返回其动作值。两者通过同一回调提供原始动作值、生成时的加成及该次命中的实际会心倍率。独立近战的 `Strike::reaction` 可声明 `MonsterReaction::Topple`，用于在 GU 结算伤害后请求可倒地的地面怪物进入 GU 地面倒地动作。控制端用 `MonsterStrikes::restrained_contact` 指定出招者当前束缚的目标部位，其他怪物沿用原生几何判定。

[rules](src/weapon/style/rules.rs) 为已注册的风格提供可选的格挡半角、公共动作结束条件、弹刀和镜头回调。[particle](src/weapon/style/particle.rs) 提供模型粒子绘制与释放回调的登记接口。`weapon_joint_pose` 从已绑定的主武器部件复制关节矩阵。`pose_world_unit` 通过 GU 原生入口更新仍在播放的世界粒子坐标和朝向。

需要跟随独立场景对象的特效可使用世界坐标发射。持有单位句柄时，通过 `on_release` 在原生释放前移除记录，在游戏线程用 `move_world_unit` 更新已提交单位的位置，保留其播放进度。对象结束时，先移除记录再调用 `stop_unit`。[磁力弹与部位标记示例](../../plugins/magnet-spike/src/fx.rs)

扩展武器可以在私有动作中使用 [`guard::Decision::InPlace`](src/weapon/style/guard.rs)。风格提供格挡角度与窗口，GU 结算后通过 `GuardResult::on_block` 通知一次，当前动作继续播放，格挡免除伤害、耐力和斩味消耗。

扩展武器通过 [`remote::bind_playback`](src/weapon/style/remote.rs) 登记队友动作回放。`ActionState` 可随动作包携带一个 0–7 的值，例如用于选择共有动作下的武器部件姿势。注册器根据收到的装备资料选择回放，内容插件按队友分别保存游标。[磁斩锤回放实现](../../plugins/magnet-spike/src/peer.rs)

扩展武器可调用 [`remote::bind_position_events`](src/weapon/style/remote.rs) 登记任务位置事件。发送时沿用 GU 的任务位置包，在保留位携带 27 位武器数据。SDK 验证发射者装备，并把发射者的原生坐标一起交给接收回调。内容插件负责载荷格式、重复事件和效果清理。[磁力弹回放](../../plugins/magnet-spike/src/magnet/network.rs)

扩展武器可通过 `remote::bind_target_events` 接收队友的怪物部位接触，再用 `remote::send_target_event` 发送本地扫掠取得的 `Target`。SDK 复用 GU 的怪物资料包，按原生发送者槽、装备、区域、怪物配置 ID、复活序号和受击形状行解析目标。风格使用事件序号处理先后顺序，可在后续动作中继续查询目标骨骼上的接触点。[怪物接触接口](src/weapon/style/remote.rs)

`remote::bind_position_state` 通过 `PositionState` 登记 GU 周期位置包的编解码回调。`PositionSample::action` 可携带同一私有动作内的 0–7 值，按动作 ID 与动作序号核对。`PositionSample::weapon` 可携带跨动作的 0–3 值，按该武器状态的更新序号核对，待机时也能使用。两个字段都用 `u8::MAX` 表示本次未提供。位置与朝向由 GU 接收。[位置包实现](../../plugins/mhgu-sdk-runtime/src/weapon/style/multiplayer/position.rs)

[rules::Rules](src/weapon/style/rules.rs) 可登记独立的任务镜头回调。`camera_yaw` 返回水平视角的 GU 角度单位，返回 -1 时由游戏决定视角。注册器在原版按键处理前应用该角度。`camera_lens` 在普通任务镜头的碰撞检查前按 GU 帧数调整视野角、水平半径和相对猎人高度。`camera_offset` 为视图更新提供临时的垂直震动。

`Style::commands` 可在 GU 装载命令表时追加一组有序的武器派生，由 GU 缓冲先接收的输入并在指定帧切换动作。`action::enter` 走 GU 的公共动作切换。已判定的单条派生可通过 `action::transition` 提交，狩技使用 `Transition::HUNTER_ART`。`action::play_motion` 使用猎人的原生动作编码。`Attack::submit` 把动作值和形状交给 GU 的攻击构造器及武器修正。注册器在大剑的首个有效攻击事件处理兽魂附身。`guard::Policy` 提供一次格挡结果，注册器负责把结果交给 GU 的受击、斩味和回正流程，内容插件可以用 `guard::take` 进入后续招式。
