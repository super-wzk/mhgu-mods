# 怪物物种注册

让同一场任务里的原版怪物和新增怪物各自计数。原版物种直接查询 GU 的完整编号（含变种），原版事件交回引擎处理；新增物种由内容插件声明。新增物种可声明一个原生模板：引擎通过模板索引找构造器及固定布局，实例仍保存新物种编号。资源路径和行为由内容插件提供，模板的原版狩猎和尺寸记录不会被新增物种改写。

物种服务由 [mhgu-sdk.nro](README.md) 提供。新增物种的内容插件在 Forge 入口调用 `monster::species::registry::when_ready`，在回调中取得 `MonsterRegistry::current()` 并注册一次。SDK 先加载时当场执行回调，后加载时在注册服务和钩子就绪后执行。原版物种仍可直接通过 SDK 查询。

每只新增怪物声明一个稳定键（例如 `frontier:espinas`）、完整的 GU 任务编号（例如 `0x006b`）及同类别的原生模板。编号不能占用原版或其他新增物种；重复声明必须与原声明的键、编号和模板一致。按已有编号替换模型或动作时，不注册新物种。SDK 入口见 [species](../../crates/mhgu-sdk/src/monster/species/registry.rs)。

讨伐和捕获先记入本场，普通结算后才合并进当前角色；没有结算的场次不累计。普通角色存档保存成功时写 `savedata:/monster-registry-<角色槽>-<0或1>.tsv`。两份文件交替写入，以能完整解析的最新一份为准。数值用十进制；实际列之间是制表符：

```text
generation	1
species	slain	captured
frontier:espinas	1	0
# end
```

在仓库根目录运行 `cargo test --manifest-path plugins/mhgu-sdk-runtime/Cargo.toml` 验证记录读写。构建 SDK 运行时和棘龙的安装包后，将 `MHGU_ROMFS` 设为包含 `nativeNX` 的 GU RomFS 根目录，再以装有 Unicorn 的 Python 运行 `python3 plugins/mhgu-sdk-runtime/tests/verify_espinas_quest.py --native "$MHGU_ROMFS/nativeNX"`。回放使用实际 NRO、原版任务组和怪物归档，验证 SDK 先后加载时的注册、塔顶单怪出场、讨伐和捕获结算，以及原版怪物的资源与记录隔离。角色选择、文件服务及渲染边界使用内存替身，不接触真实存档。新增物种的尺寸纪录、图鉴和联机同步尚未接入。
