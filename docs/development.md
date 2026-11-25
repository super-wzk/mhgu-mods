# 开发指引

## 仓库入口

| 位置 | 职责 |
| --- | --- |
| [crates/mhgu-sdk](../crates/mhgu-sdk/README.md) | GU 1.4 引擎绑定，包括原生输入和转向、动作、碰撞、资源与注册接口。 |
| [crates/mhfz-sdk](../crates/mhfz-sdk/README.md) | 极型武器共用的 Frontier 动作程序、动作库预加载、技能帧数补正、弹刀、镜头震动和表现时序。 |
| [plugins/style-registry](../plugins/style-registry/README.md) | 扩展装备箱的原版风格页，按武器保存选择，转接 GU 动作、狩技、碰撞、粒子绘制与附加肩键相关的聊天入口。 |
| [极之型大剑](../plugins/great-sword-extreme/README.md)、[极之型双剑](../plugins/dual-blades-extreme/README.md)、[极之型太刀](../plugins/long-sword-extreme/README.md)、[极之型斩斧 F](../plugins/switch-axe-extreme/README.md)、[极之型单手剑](../plugins/sword-shield-extreme/README.md)、[极之型铳枪](../plugins/gunlance-extreme/README.md) | 各武器的动作编号、派生优先级、精准防御规则、槽、专属狩技及表现资源。 |
| [plugins/monster-registry](../plugins/monster-registry/README.md) / [plugins/monster-espinas](../plugins/monster-espinas/README.md) | 独立物种、任务和怪物行为的接入实例。 |
| [tools/mhfz2mhgu](../tools/mhfz2mhgu/README.md) | 宿主资源转换 CLI。`mhf-resource` 读 FZ 格式，`mhgu-resource` 读写 GU 格式，`src/hunter.rs` 等负责具体移植映射。 |

各武器当前的派生、槽、技能、特效和联机实现可在[极之型武器实现与回归](weapon-style-implementation.md)按功能查阅。

## 文档同步

1. 开工时读本页和受影响的插件 README。涉及玩家操作时同时检查对应的 `src/manual.txt`，涉及 SDK 或转换器时查看对应组件的 README。
2. 改动落实后，按影响范围同步说明。键位、派生和槽规则写在武器 README，玩家需要的操作同步到游戏内手册。共用接入和验收规则更新到[移植流程](weapon-style-porting.md)，跨武器行为及相关回放更新到[实现与回归](weapon-style-implementation.md)。构建、安装和选用验证的命令集中在本页。新出现或复发的故障在[常见故障与排查](weapon-style-troubleshooting.md)记录触发条件、检查点、证据和对应回放，修正已有的旧结论。
3. 交付前在同一改动中核对文档与最终实现。检查相关命令的参数、测试入口、相对链接和旧说法。手册改动按[注册器说明](../plugins/style-registry/README.md)检查格式，并用打包后的 NRO 走共用武器验收。提交或交付说明注明同步的文档、已验证的范围和待实机确认的事项。现有说明仍适用时，说明检查了哪些入口。

## 本地输入和构建

准备 Rust、插件目录声明的 nightly 和 `rust-src`、`cargo forge` 以及 Python 3。Forge 插件使用 ARM32 小端自定义目标。直接执行 `cargo forge build` 时从插件目录启动，让 Cargo 发现该目录的 `.cargo/config.toml`。棘龙及其注册器的输入与命令见[棘龙插件说明](../plugins/monster-espinas/README.md)。

| 输入 | 用途 |
| --- | --- |
| `MHFZ_DAT` | Frontier 的 `dat` 目录。转换器读取其中的猎人模型、动作、动作表、判定、音效和粒子资源。 |
| `MHGU_ROMFS` | 包含 `nativeNX` 的 GU RomFS 根目录。武器打包脚本统一通过第二个位置参数接收此路径。部分离线回放从环境变量读取 GU 武器 ARC。 |
| `mhfz/eboot.bin.elf`、`mhgu/main.elf` | 对应版本的本地 ELF。部分离线脚本使用这两个仓库相对位置。 |
| `mhfz/out/`、`mhgu/out/` | 可选的本地反编译导出。优先用 `function_list.txt` 和 `decompiled.c` 定位函数，再用资源表和调用者核对解释。 |

首次构建或修改资源后，从仓库根目录打包。只运行所需的武器打包命令即可。

```sh
export MHGU_ROMFS="${MHGU_ROMFS:?请指定包含 nativeNX 的 GU RomFS 根目录}"
: "${MHFZ_DAT:?请指定 Frontier 的 dat 目录}"
python3 plugins/style-registry/package.py
python3 plugins/great-sword-extreme/package.py "$MHFZ_DAT" "$MHGU_ROMFS"
python3 plugins/dual-blades-extreme/package.py "$MHFZ_DAT" "$MHGU_ROMFS"
python3 plugins/long-sword-extreme/package.py "$MHFZ_DAT" "$MHGU_ROMFS"
python3 plugins/switch-axe-extreme/package.py "$MHFZ_DAT" "$MHGU_ROMFS"
python3 plugins/sword-shield-extreme/package.py "$MHFZ_DAT" "$MHGU_ROMFS"
python3 plugins/gunlance-extreme/package.py "$MHFZ_DAT" "$MHGU_ROMFS"
```

武器打包脚本先把源数据转换到插件的 `target/assets/`，再构建 NRO 并组装 `dist/<插件名>/romfs/nativeNX`。动作程序和部分事件表由 `include_bytes!` 编入 NRO，LMT、命中、粒子、音效和贴图等原生资源随模组目录安装。

已有匹配的资源且本次仅改 Rust 时，可以增量构建。离线脚本通常读取 `dist` 中的 NRO，需同步本次产物再运行。以下以双剑为例。

```sh
(cd plugins/dual-blades-extreme && cargo forge build --release) &&
  cp plugins/dual-blades-extreme/dual_blades_extreme.nro \
     plugins/dual-blades-extreme/dist/dual-blades-extreme/romfs/nativeNX/plugins/
```

共用 SDK 改动后重建受影响的注册器和武器。转换器、源数据或资源格式发生变化时，重新运行相关打包脚本，让 NRO 内的事件表与外部资源一起更新。整理代码后也可对比重建前后的非 NRO 文件清单和哈希，确认生成资源一致。

## 调查原生入口

已有导出时可用 `rg -n 'func-address: 0x2b72d4' mhgu/out/decompiled.c` 定位攻击构造器，再跟到调用者、虚表或数据资源。记录游戏版本、函数相对地址、输入与返回语义。反编译推断不确定时才核对该入口的指令。带浮点数的原生调用还要确认参数走通用寄存器还是 VFP 寄存器，优先复用 SDK 现有的桥接函数。

运行时在所属游戏线程使用活对象及借用指针。注册描述符和提供给引擎长期读取的表使用常驻存储。共享派发入口可能收到集会所猎人，读取任务专用字段前通过 `hunter::is_quest` 确认类型。切换动作、场景或角色时，按实际生命周期清理插件状态，确认旧对象仍有效后再访问它。

当前武器风格使用 V1 注册服务。修改共用接口后，一起重建并安装注册器与使用该接口的插件。

## 安装和确认

启用 Forge 后，把各插件 `dist/<插件名>` 分别装入 MHGURecomp 的模组目录，保留完整的 `romfs/nativeNX` 层级。武器插件通过 `registry::when_ready` 等待风格注册器就绪。安装棘龙时须让 `monster-registry` 先于 `monster-espinas` 初始化。共用钩子或资源布局发生变化时，把关联插件及转换资源一起更新。仅修改代码且资源匹配时可以只更新对应的 NRO。

完整极之型合包包含配套的风格注册器与当前六个武器模组目录。归档前核对每个目录的 NRO 和配套资源。已有合包的命名格式为 `mhgu边境风格-YYYYMMDDHHMM.7z`。

重启游戏后，在原版装备箱按当前武器选风格。配置保存在 Forge 的 `savedata:/style_registry.ini`，`[Weapons]` 下用 GU 武器编号作键。查错误时依次确认 **安装包的 NRO 和资源、Forge 初始化和钩子日志、装备箱选择、任务内执行路径**。看到模组被列出或 NRO 被加载，只能确认到了加载阶段。

## 验证范围

| 修改内容 | 优先检查 |
| --- | --- |
| 单个速度、距离或计时参数 | 构建受影响的插件。需要安装时对照安装文件与本次产物。手感由任务内观察确认。 |
| 装备箱选择、肩键与 GU 命令优先级 | [装备箱与保存](../plugins/great-sword-extreme/tests/verify_menu.py)、[聊天快捷键与肩键交换](../plugins/style-registry/tests/verify_chat_shortcut.py)、[大剑输入及动作生命周期](../plugins/great-sword-extreme/tests/verify_style.py)。 |
| 攻击、防御、狩技和武器专属派生 | [大剑精防、蓄力与派生](../plugins/great-sword-extreme/tests/verify_style.py)、[大剑与双剑弹刀、刃打和双手刀光](../plugins/style-registry/tests/verify_deflection.py)、[专属狩技](../plugins/style-registry/tests/verify_weapon_arts.py)、[太刀](../plugins/long-sword-extreme/tests/verify_port.py)、[斩斧](../plugins/switch-axe-extreme/tests/verify_port.py)、[单手剑](../plugins/sword-shield-extreme/tests/verify_port.py)及[铳枪](../plugins/gunlance-extreme/tests/verify_port.py)的动作回放。 |
| 动作、音效、特效与材质 | 对照输出槽位、事件帧、挂点或材质字段。参考[资源检查](../plugins/great-sword-extreme/tests/verify_assets.py)、[原生粒子](../plugins/great-sword-extreme/tests/verify_native_fx.py)和[材质绘制](../plugins/great-sword-extreme/tests/verify_native_material.py)。 |
| 注册顺序、手册、预加载和角色交接 | [共用武器验收](../plugins/style-registry/tests/verify_weapon_styles.py)覆盖六种武器、注册器最后启动、两个手册入口、任务预读和成品粒子路径。[斩斧集会所回放](../plugins/switch-axe-extreme/tests/verify_lobby.py)核对任务角色与集会所角色交接。 |
| 联机动作与伤害归属 | [大剑与双剑](../plugins/style-registry/tests/verify_gs_db_multiplayer.py)、[太刀](../plugins/long-sword-extreme/tests/verify_multiplayer.py)、[片手与铳枪](../plugins/style-registry/tests/verify_sns_gl_multiplayer.py)和[斩斧](../plugins/switch-axe-extreme/tests/verify_port.py)覆盖 GU 原生动作包和队员命中。实机另查双方的动作、特效和结算。 |
| 独立怪物、任务与结算 | 按[棘龙插件说明](../plugins/monster-espinas/README.md)生成两个安装包，运行[任务注册与结算](../plugins/monster-registry/tests/verify_espinas_quest.py)及对应的出场、音效和碰撞检查。 |

从仓库根目录使用可导入 `unicorn` 的 Python 运行离线脚本。准备相应的本地 ELF，并保留打包生成的 `target/assets` 和 `dist`。涉及 GU 原版武器 ARC 的脚本还需要导出 `MHGU_ROMFS`。共用武器验收需要已构建的注册器和所有武器风格的 `dist`。

```sh
python3 plugins/style-registry/tests/verify_chat_shortcut.py
python3 plugins/style-registry/tests/verify_weapon_styles.py
python3 plugins/switch-axe-extreme/tests/verify_lobby.py
python3 plugins/switch-axe-extreme/tests/verify_port.py
```

分别记录构建、安装、离线回放和实机结果。离线回放可以核对事件、命令、资源路径与原生回调。光效外观、实际距离、操作手感及真实联机以任务内观察为准，复现记录应包含角色状态、输入顺序、区域、预期与实际表现。
