# mhgu-sdk

MHGU Switch 1.4 的 Rust 引擎绑定。插件通过路径依赖引用 `mhgu-sdk`，Rust 中使用 `mhgu_sdk`。包为 `no_std`；宿主工具可以使用 `MotionId` 等值类型，ARM32 小端目标还提供运行时绑定。

在 GU 回调中用 `unsafe { Monster::from_raw(actor) }` 借用引擎持有的怪物，再通过 `monster.state()` 取得 `MonsterState`。它借用引擎的实时状态块，可访问位置、生命值、动作阶段和碰撞对象；复制句柄不会复制这些数据。空的怪物指针或尚未挂载的状态块分别返回 `None`。句柄只能在对象和状态块有效时、在所属游戏线程使用，不能存入插件的跨帧状态。可存储 `MotionId` 等值类型。

在怪物动作更新回调（GU 1.4 的 `CF5F64`）入口，用 `MonsterUpdate::from_callback` 一次性确认线程和回调生命周期。取得句柄后，`play_motion` 会检查原生动作槽是否已加载；读取骨骼坐标、切换已知姿态、启用主碰撞和生成弹体都使用安全方法。普通 `Monster` 句柄不能转换成更新句柄。

插件在自己的布局适配层确认宿主类，读取本次更新的原生资源 ID，并封装为 `DirectedShellResource`。只有该适配层需要确认 ID 对应正确的原生弹体类。SDK 检查 ID 范围；句柄绑定当前怪物和回调生命周期，不能用于另一只怪物。发射代码只接收句柄和轨迹：

```rust
use mhgu_sdk::{combat::shell::ShellLaunch, monster::shell::DirectedShellResource};

fn spawn(resource: &DirectedShellResource<'_>, launch: ShellLaunch) -> bool {
    resource.spawn(launch)
}
```

SDK 用句柄中的 ID 登记弹体；引擎拒绝时返回 `false`。选择哪种招式、发射帧和速度由插件决定。碰撞池则必须一直存活到引擎不再持有地址；和原始回调入口一样，这个生命周期边界显式保留 `unsafe`。

模块首先区分操作对象。插件导入怪物行为编号用 `mhgu_sdk::monster::action::ActionId`，导入共享碰撞对象用 `mhgu_sdk::combat::collision::Contact`。宿主工具可以使用 `ActionId`、`MotionId`、`AngerMode`、`Posture` 和几何值；访问活对象、调用引擎的方法仅在 ARM32 小端目标编译。

| 模块 | 职责 |
| --- | --- |
| [monster](src/monster/mod.rs) | 借用怪物实例、状态块和更新回调。 [MonsterClass](src/monster/instance.rs) 按虚表匹配原生类，不代表任务物种 ID。 |
| [motion](src/motion/mod.rs) | `MotionSlot` 只表示 LMT 列表内的槽，`MotionList::contains` 检查该槽是否有片段。怪物的 `MotionId` 还编码了库号；经 `monster.motion_list(id.bank())` 取得对应列表。 |
| [monster::action](src/monster/action/mod.rs) / [monster::motion](src/monster/motion/mod.rs) | `monster.action()` 返回怪物行为的组与槽；`monster.motion()` 返回正在播放的动画编号。两者使用不同类型。 |
| [monster::anger](src/monster/anger/mod.rs) / [monster::damage](src/monster/damage/mod.rs) | 怒态、生命值、部位硬直及原生受击程序。更新受击程序须在动作入口，并在 GU 使用期间持有它。 |
| [quest](src/quest.rs) | 通过 `update.quest()` 查询指定区域的猎人、按原序遍历区域节点，或从游戏随机数源取样。句柄限定在本次任务回调和游戏线程。 |
| [monster::movement](src/monster/movement/mod.rs) | 怪物姿态、位置与转向。[巡逻](src/monster/scene.rs) 按怪物的掩码筛选区域节点；[骨骼绑定](src/monster/skeleton.rs) 从当前更新取关节坐标。 |
| [monster::attack](src/monster/attack/mod.rs) | 从怪物的 HDE / HTS 取得攻击行和形状组，启用主判定；在攻击设置回调中用 `row.configure_melee(contact)` 配置近战判定。 |
| [combat::shell](src/combat/shell.rs) / [monster::shell](src/monster/shell.rs) | 发射位置、角度与速度是独立值；用于创建弹体的资源句柄绑定本次怪物更新。 |
| [combat::collision](src/combat/collision/mod.rs) | 从碰撞对象或怪物攻击表取得 HTS 形状组。批量查询时 `row.shape_group_in(groups)` 核对行与资源，`contact.set_shape_group(group)` 核对接收方；相同编号不能跨资源使用。 |
| [geometry](src/geometry.rs) | GU 角度、水平距离和方位；不依赖怪物实例或游戏回调。 |

[memory](src/memory.rs) 仅供 SDK 内部读写原生字段和换算引擎入口地址。
