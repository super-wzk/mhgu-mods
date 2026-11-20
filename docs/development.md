# 开发指引

## 仓库入口

| 位置 | 职责 |
| --- | --- |
| [crates/mhgu-sdk](../crates/mhgu-sdk/README.md) | GU 1.4 引擎绑定、动作与碰撞接口、风格和怪物注册接口。 |
| [crates/mhfz-sdk](../crates/mhfz-sdk/README.md) | 极型武器共用的 Frontier 动作程序、输入、音效和粒子适配，依赖 mhgu-sdk。 |
| [plugins/style-registry](../plugins/style-registry/README.md) | 扩展装备箱的原版风格页，按武器保存选择，转接公共动作、狩技、命中、防御和粒子绘制。 |
| [plugins/great-sword-extreme](../plugins/great-sword-extreme/README.md) / [plugins/dual-blades-extreme](../plugins/dual-blades-extreme/README.md) / [plugins/long-sword-extreme](../plugins/long-sword-extreme/README.md) | 动作程序、风格输入与派生、专属音效和特效。双剑还提供刃打和火焰槽 HUD。 |
| [plugins/monster-registry](../plugins/monster-registry/README.md) / [plugins/monster-espinas](../plugins/monster-espinas/README.md) | 独立物种、任务和怪物行为的接入实例。 |
| [tools](../tools/) | 宿主资源转换 CLI。`mhf-resource` 读 FZ 格式，`mhgu-resource` 读写 GU 格式，`src/hunter.rs` 等负责具体移植映射。 |

## 本地输入和构建

准备 Rust、插件目录声明的 nightly 和 `rust-src`、`cargo forge` 以及 Python 3。Forge 插件使用 ARM32 小端自定义目标。直接执行 `cargo forge build` 时从插件目录启动，让 Cargo 发现该目录的 `.cargo/config.toml`。如果 IDE 报 `.json target specs require -Zjson-target-spec`，先检查实际运行目录和 `[unstable] json-target-spec` 是否被读取。

| 输入 | 用途 |
| --- | --- |
| `MHFZ_DAT` | Frontier 的 `dat` 目录。转换器读取其中的猎人模型、动作、动作表、判定、音效和粒子资源。 |
| `MHGU_ROMFS` | 包含 `nativeNX` 的 GU RomFS 根目录。打包读取原版材质等参考资源，部分离线检查也从这里读取武器 ARC。 |
| `mhfz/eboot.bin.elf`、`mhgu/main.elf` | 对应版本的本地 ELF。部分离线脚本使用这两个仓库相对位置。 |
| `mhfz/out/`、`mhgu/out/` | 可选的本地反编译导出。优先用 `function_list.txt` 和 `decompiled.c` 定位函数，再用资源表和调用者核对解释。 |

首次构建或修改资源后，从仓库根目录打包。只运行所需的武器打包命令即可。

```sh
export MHGU_ROMFS="${MHGU_ROMFS:?请指定包含 nativeNX 的 GU RomFS 根目录}"
: "${MHFZ_DAT:?请指定 Frontier 的 dat 目录}"
python3 plugins/style-registry/package.py
python3 plugins/great-sword-extreme/package.py "$MHFZ_DAT"
python3 plugins/dual-blades-extreme/package.py "$MHFZ_DAT" "$MHGU_ROMFS"
python3 plugins/long-sword-extreme/package.py "$MHFZ_DAT" "$MHGU_ROMFS"
```

打包脚本先把源数据转换到 `target/assets/`，再构建 NRO 并组装 `dist/<插件名>/romfs/nativeNX`。动作程序由 `include_bytes!` 编入 NRO。资源已经生成且本次只改 Rust 时，可以用 `(cd plugins/dual-blades-extreme && cargo forge build --release)` 增量构建，产物在插件目录的 `dual_blades_extreme.nro`。安装时取这次生成的 NRO，或先把它同步到 `dist` 中的同名文件。

## 调查原生入口

已有导出时可用 `rg -n 'func-address: 0x2b72d4' mhgu/out/decompiled.c` 定位攻击构造器，再跟到调用者、虚表或数据资源。记录游戏版本、函数相对地址、输入与返回语义。反编译推断不确定时才核对该入口的指令。带浮点数的原生调用还要确认参数走通用寄存器还是 VFP 寄存器，优先复用 SDK 现有的桥接函数。

运行时在所属游戏线程使用活对象及借用指针。注册描述符和提供给引擎长期读取的表使用常驻存储。切换动作、场景或角色时，按实际生命周期清理插件状态，确认旧对象仍有效后再访问它。

## 安装和确认

把 `dist` 下所需的模组目录分别安装到自己的 MHGURecomp 模组目录，顺序为 Forge、相应注册器、内容插件。每个模组保留完整的 `romfs/nativeNX` 层级。共用钩子或资源布局发生变化时，把关联插件及转换资源一起更新。仅修改代码时更新对应的 NRO 即可。

重启游戏后，在原版装备箱按当前武器选风格。配置保存在 Forge 的 `savedata:/style_registry.ini`，`[Weapons]` 下用 GU 武器编号作键。查错误时依次确认 **安装包的 NRO 和资源、Forge 初始化和钩子日志、装备箱选择、任务内执行路径**。看到模组被列出或 NRO 被加载，只能确认到了加载阶段。

## 验证范围

| 修改内容 | 优先检查 |
| --- | --- |
| 单个速度、距离或计时参数 | 构建受影响的插件。需要安装时对照安装文件与本次产物。手感由任务内观察确认。 |
| 配置、菜单、派生或共用命中逻辑 | 按改动挑选 [verify_menu.py](../plugins/great-sword-extreme/tests/verify_menu.py) 或 [verify_style.py](../plugins/great-sword-extreme/tests/verify_style.py) 中对应的场景。双剑特有的动作还需用双剑的数据和入口确认。 |
| LMT、音效、特效资源 | 对照输出槽位、事件帧或材质字段。现有大剑参考有 [verify_assets.py](../plugins/great-sword-extreme/tests/verify_assets.py)、[verify_native_fx.py](../plugins/great-sword-extreme/tests/verify_native_fx.py) 与 [verify_native_material.py](../plugins/great-sword-extreme/tests/verify_native_material.py)。 |
| 挂点、方向、地形衔接或场景渲染 | 用一个能重现问题的任务、动作和地图区域确认。记录角色状态、输入顺序、预期和实际表现。 |

现有 Python 离线脚本使用 Unicorn、对应版本的本地 ELF 和所需资源。先构建脚本读取的产物，再运行相关用例。离线结果可以说明表、命令和回调是否按预期执行。光效外观、实际距离、操作手感及联机行为需要各自的观察证据。
