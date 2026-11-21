# mhgu-sdk

MHGU Switch 1.4 的 Rust 引擎绑定。插件通过路径依赖引用 `mhgu-sdk`，Rust 中使用 `mhgu_sdk`。包为 `no_std`；宿主工具可以使用 `MotionId` 等值类型，ARM32 小端目标还提供运行时绑定。

`Monster::species()` 读取实例完整的任务编号，和 `MonsterClass` 的虚表判断独立。在游戏线程取 `MonsterRegistry::current()` 后，`resolve(id)` 与 `counts(species)` 可查询原版或已注册物种。SDK 自身不保存计数；新增物种的注册与持久化由 [monster-registry](../../plugins/monster-registry/README.md) 提供。

内容插件在 Forge 的 `#[forge::entry]` 中注册一次：

```rust
use mhgu_sdk::monster::species::{SpeciesId, registry::MonsterRegistry};

#[forge::entry]
fn main() {
    if let Some(registry) = MonsterRegistry::current()
        && let Err(err) = registry.register("frontier:espinas", 0x006b, SpeciesId::new(1).unwrap())
    {
        forge::log::prelude::warn!("Espinas species registration skipped: {err:?}");
    }
}
```

`current()` 在游戏线程返回句柄，从其他线程调用返回 `None`。`id.is_native()` 只查 GU 的只读物种表，不需要注册表句柄。模板只决定原版构造器和固定表的布局；物种编号、资源和猎人记录可以独立。键或编号无效、模板非原生或编号已被占用时，`register` 返回错误。如果注册服务尚未初始化，调用返回 `Unavailable`，不会排队或重试。替换已有物种无需注册。

在 GU 回调中用 `unsafe { Monster::from_raw(actor) }` 借用引擎持有的怪物，再通过 `monster.state()` 取得 `MonsterState`。它借用引擎的实时状态块，可访问位置、生命值、动作阶段和碰撞对象；复制句柄不会复制这些数据。空的怪物指针或尚未挂载的状态块分别返回 `None`。句柄只能在对象和状态块有效时、在所属游戏线程使用，不能存入插件的跨帧状态。可存储 `MotionId` 等值类型。

在怪物动作更新回调（GU 1.4 的 `CF5F64`）入口，用 `MonsterUpdate::from_callback` 一次性确认线程和回调生命周期。取得句柄后，`play_motion` 会检查原生动作槽是否已加载；读取骨骼坐标、切换已知姿态、启用主碰撞和生成弹体都使用安全方法。普通 `Monster` 句柄不能转换成更新句柄。

定向弹体可以来自插件自己的 ARC，也可以来自引擎已有槽位。前者按路径哈希解析并持有资源引用；后者由插件确认槽位所属的类和资源 ID，再封装为 `DirectedShellResource`。只有该适配层需要确认 ID 对应正确的原生弹体类。SDK 检查 ID 范围；句柄绑定当前怪物和回调生命周期，不能用于另一只怪物。发射代码只接收句柄和轨迹：

```rust
use mhgu_sdk::{combat::shell::ShellLaunch, monster::shell::DirectedShellResource};

fn spawn(resource: &DirectedShellResource<'_>, launch: ShellLaunch) -> bool {
    resource.spawn(launch)
}
```

独立资源直接绑定到 GU 的定向弹体实例，不占用或替换原版编号缓存；已有槽位沿用 GU 的编号登记流程。引擎拒绝时返回 `false`。选择哪种招式、发射帧和速度由插件决定。碰撞池则必须一直存活到引擎不再持有地址；和原始回调入口一样，这个生命周期边界显式保留 `unsafe`。

模块首先区分操作对象。插件导入怪物行为编号用 `mhgu_sdk::monster::action::ActionId`，导入共享碰撞对象用 `mhgu_sdk::combat::collision::Contact`。宿主工具可以使用 `ActionId`、`MotionId`、`AngerMode`、`Posture` 和几何值；访问活对象、调用引擎的方法仅在 ARM32 小端目标编译。

| 模块 | 职责 |
| --- | --- |
| [monster](src/monster/mod.rs) | 借用怪物实例、状态块和更新回调。 [MonsterClass](src/monster/instance.rs) 按虚表匹配原生类，不代表任务物种 ID。 |
| [motion](src/motion/mod.rs) | `MotionSlot` 只表示 LMT 列表内的槽，`MotionList::contains` 检查该槽是否有片段。怪物的 `MotionId` 还编码了库号；经 `monster.motion_list(id.bank())` 取得对应列表。 |
| [monster::action](src/monster/action/mod.rs) / [monster::motion](src/monster/motion/mod.rs) | `monster.action()` 返回怪物行为的组与槽；`monster.motion()` 返回正在播放的动画编号。两者使用不同类型。 |
| [monster::anger](src/monster/anger/mod.rs) / [monster::damage](src/monster/damage/mod.rs) | 怒态、生命值、部位硬直及原生受击程序。更新受击程序须在动作入口，并在 GU 使用期间持有它。 |
| [quest](src/quest.rs) | 通过 `update.quest()` 查询指定区域的猎人、按原序遍历区域节点，或从游戏随机数源取样。句柄限定在本次任务回调和游戏线程。 |
| [monster::movement](src/monster/movement/mod.rs) | 怪物姿态、位置与转向。[巡逻](src/monster/scene.rs) 按地图、区域和路线编号读取当前怪物的巡逻资源；[骨骼绑定](src/monster/skeleton.rs) 从当前更新取关节坐标。 |
| [monster::attack](src/monster/attack/mod.rs) | 从怪物的 HDE / HTS 取得攻击行和形状组，启用主判定；在攻击设置回调中用 `row.configure_melee(contact)` 配置近战判定。 |
| [combat::shell](src/combat/shell.rs) / [monster::shell](src/monster/shell.rs) | 发射位置、角度与速度是独立值；用于创建弹体的资源句柄绑定本次怪物更新。 |
| [combat::collision](src/combat/collision/mod.rs) | 从碰撞对象或怪物攻击表取得 HTS 形状组。批量查询时 `row.shape_group_in(groups)` 核对行与资源，`contact.set_shape_group(group)` 核对接收方；相同编号不能跨资源使用。 |
| [geometry](src/geometry.rs) | GU 角度、水平距离和方位；不依赖怪物实例或游戏回调。 |

[memory](src/memory.rs) 仅供 SDK 内部读写原生字段和换算引擎入口地址。

## 武器风格插件

[mhfz-sdk](../mhfz-sdk/README.md) 在本 SDK 的 GU 接口上提供极型武器共用的 Frontier 动作、输入及表现逻辑。

[StyleRegistry](src/style/registry.rs) 在 GU 游戏线程访问风格注册器。注册器先初始化，内容插件在 Forge 初始化入口调用一次 `registry.register(&STYLE, name, description)`。一个 V1 描述符提供动作编号、狩技槽位数及可选的请求、更新和防御规则。用 `style::selected(weapon)` 查询当前风格，返回 0 时使用 GU 原版风格。实际用法见[极型大剑](../../plugins/great-sword-extreme/src/lib.rs)。

GU 从武器动作组派发已登记的招式编号（0–255）。`Input` 提供 GU 已解析的公共和武器指令，`request` 处理 GU 已接受的拔刀和武器切招请求。公共动作可返回 `PASS` 交给 GU。空输入通知插件结束，actor 为 0 时代表旧猎人已失效。

[remote](src/style/remote.rs) 提供 GU 玩家槽查询和远端动作状态的存储。读取远端装备前按当前名册核对猎人，角色离场、换武器或槽位更换时释放旧状态。风格插件负责根据 GU 接收的动作回放各自的招式。插件可以分别登记动作回放（`bind`）、逐帧观察（`observe`）和可选的动作状态收发（`bind_action_state`）。注册器校验当前武器的动作包是否有可用位，插件定义状态值的含义。

[physical](src/style/physical.rs) 提供统一的物理攻击力回调登记。回调可限定在 GU 的左右刀身碰撞槽，也可覆盖同一猎人的独立武器碰撞。风格插件负责数值与生效条件。

[rules](src/style/rules.rs) 为已注册的风格提供可选的格挡半角、公共动作结束条件、弹刀和镜头回调。[particle](src/style/particle.rs) 提供模型粒子绘制与释放回调的登记接口。

`Style::commands` 可在 GU 装载命令表时追加一组有序的武器派生，由 GU 缓冲先接收的输入并在指定帧切换动作。`action::enter` 走 GU 的公共动作切换。已判定的单条派生可通过 `action::transition` 提交，狩技使用 `Transition::HUNTER_ART`。`action::play_motion` 使用猎人的原生动作编码。`Attack::submit` 把动作值和形状交给 GU 的攻击构造器及武器修正。注册器在大剑的首个有效攻击事件处理兽魂附身。`guard::Policy` 提供一次格挡结果，注册器负责把结果交给 GU 的受击、斩味和回正流程，内容插件可以用 `guard::take` 进入后续招式。
