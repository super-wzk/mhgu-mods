# monster-registry

让同一场任务里的原版怪物和新增怪物各自计数。原版物种直接查询 GU 的完整编号（含变种），原版事件交回引擎处理；新增物种由内容插件声明。新增物种可声明一个原生模板：引擎通过模板索引找构造器及固定布局，实例仍保存新物种编号。资源路径和行为由内容插件提供，模板的原版狩猎和尺寸记录不会被新增物种改写。

在仓库根目录运行 `python3 plugins/monster-registry/package.py`，安装目录为 `plugins/monster-registry/dist/monster-registry/`。注册插件在自己的 Forge 入口发布接口。需要新增物种时，让注册插件先初始化，再由内容插件在入口注册一次；服务尚未就绪会返回 `Unavailable`，之后不补注册。原版物种查询不依赖这个插件。

每只新增怪物声明一个稳定键（例如 `frontier:espinas`）、完整的 GU 任务编号（例如 `0x006b`）及同类别的原生模板。编号不能占用原版或其他新增物种；重复声明必须与原声明的键、编号和模板一致。按已有编号替换模型或动作时，不注册新物种。SDK 入口见 [species](../../crates/mhgu-sdk/src/monster/species/registry.rs)。

讨伐和捕获先记入本场，普通结算后才合并进当前角色；没有结算的场次不累计。普通角色存档保存成功时写 `savedata:/monster-registry-<角色槽>-<0或1>.tsv`。两份文件交替写入，以能完整解析的最新一份为准。数值用十进制；实际列之间是制表符：

```text
generation	1
species	slain	captured
frontier:espinas	1	0
# end
```

离线验证：在仓库根目录运行 `cargo test --manifest-path plugins/monster-registry/Cargo.toml`。构建两个插件的安装包后，将 `MHGU_ROMFS` 设为包含 `nativeNX` 的 GU RomFS 根目录，再以装有 Unicorn 的 Python 运行 `python3 plugins/monster-registry/tests/verify_espinas_quest.py --native "$MHGU_ROMFS/nativeNX"`。它会读取两个实际安装包、原版任务组和怪物归档，执行 GU 和 NRO 的 ARM 代码来验证独立任务、塔顶单怪的出场、讨伐和捕获结算，以及与原版怪物的资源和记录隔离。角色选择、文件服务及渲染边界使用内存替身，不接触真实存档。新增物种的尺寸纪录、图鉴和联机同步不由本插件提供。
