# 普通棘龙（Frontier Z → MHGU 1.4）

将普通 FZ 棘龙（em080）放进 GU 雌火龙的任务槽。一个模组同时包含模型、动作、身体和攻击碰撞、原版战斗选招数据及运行时。

选招从 [mhgu-sdk](../../crates/mhgu-sdk/README.md) 查询猎人、巡逻点和怪物状态。插件决定 FZ 的招式、碰撞窗口、发射时机和轨迹；SDK 负责播放 GU 动作、编码受击程序、校验碰撞形状组、计算水平朝向和登记弹体。硬直用哪个动作、断尾后换哪个形状组、冲锋何时转向由插件决定。GU 1.4 雌火龙的类标识与主弹体槽偏移只在本插件的 [rathian.rs](src/rathian.rs) 中声明；每次发射前由插件从该槽解析资源句柄，SDK 不接收字段偏移。

可安装的目录是 `dist/monster-espinas/`；它包含 `romfs/nativeNX/arc/enemy/em001_00.arc` 和 `romfs/nativeNX/plugins/monster_espinas.nro`，两者应配套使用。历史单招包在 `target/fixtures/` 下，只用于离线对照；同时启用会争用同一只怪物的资源。

[tests/](tests/) 保存可重复运行的棘龙离线回归脚本，和插件源码一起维护。游戏文件由本机提供；生成的测试包、轨迹和缓存不纳入 Git。

将 `MHFZ_EM080_PAC` 设为 FZ 的 `em080.pac` 文件路径，`MHGU_ROMFS` 设为包含 `nativeNX` 的 GU RomFS 根目录。从仓库根目录运行：

```sh
pac="${MHFZ_EM080_PAC:?请指定 FZ 的 em080.pac 文件路径}"
enemy="${MHGU_ROMFS:?请指定 GU RomFS 根目录}/nativeNX/arc/enemy"
python3 plugins/monster-espinas/package.py --pac "$pac" --enemy "$enemy"
python3 plugins/monster-espinas/tests/verify_espinas_spawn.py
python3 plugins/monster-espinas/tests/verify_espinas_contacts.py plugins/monster-espinas/dist/monster-espinas
```

插件的 `package.py` 调用转换器生成棘龙 ARC（肉质和怒气参数来自同套 FZ 文件的 `dat/mhfemd.bin`），然后在插件目录执行 `cargo forge build --release`，将生成的 NRO 移入暂存包。两步都成功后才替换 `dist/monster-espinas/`。转换器仍会拒绝未绑定的选招。转换器的离线回归直接将正式 ARC 的选招表与 Vita 选择器对照，不写出额外的模组。Python 回归使用 Unicorn，不启动游戏，不改存档或已安装模组。

普通开场先卧下（39），再循环睡眠（34）。猎人在附近可以诱发睡姿变化，但靠近本身不会唤醒它。伤害超过开场生命的 2%（阈值最多 1000），或睡眠计时结束后，依次起身（37 → 62）。战斗入口 0:3 按怒态、姿态、猎人的相对位置和随机数选出 28 种请求、38 种序列。普通冲锋、折返连冲、两段甩尾、三连火球和腾空落地都保留后续阶段。GU 的发现猎人状态和飞行姿态是两个独立字段。

硬直按受击部位分派，头部斩击肉质在常态和怒态分别为 15 和 70。普通个体使用原始怒气阈值 700 和持续时间 5400 个 FZ 逻辑帧。GU 的讨伐流程保留，地面死亡槽使用 FZ 动作 68。断尾的身体尖端、切面与地面可剥取对象使用 FZ 网格；近身攻击还会去掉断尾后不存在的碰撞体。

**当前验证范围：** 原版 Vita 解释器的 2560 组战斗选招，原生睡眠与冲锋处理器，以及编译后 NRO 的单个体持续回归。回归覆盖普通和警戒地面、空中降落、折返、火球发射、固定坐标攻击、怒前怒后肉质和分部位硬直。它不模拟整个任务。跨区追踪、陷阱捕获、弹体命中后的复合异常、切尾和剥取到任务结算的完整过程，尚未逐项证明与 FZ 一致。
