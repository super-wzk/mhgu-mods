# mhfz2mhgu

从 Frontier 怪物 PAC 生成 Switch 版 MHGU 的带骨架模型（MOD 0xD4）和动作列表（LMT v67）。行为子命令从 Vita ELF 自动扫描脚本分组，可输出支持的指令或单招样本。可输出单独资源、带材质贴图的外观 ARC，或借用普通雌火龙任务槽的棘龙动作回放包。

棘龙插件的生成物统一在 [monster-espinas](../../plugins/monster-espinas/README.md) 下。插件的 `package.py` 用 `hunt` 生成完整狩猎资源，再调用 Forge 编译插件并组装安装包。下列单招导出命令只用于离线分析。

将 `MHFZ_EM080_PAC` 设为 FZ 的 `em080.pac` 文件路径，`MHGU_ROMFS` 设为包含 `nativeNX` 的 GU RomFS 根目录。从本仓库根目录运行：

```sh
cargo build --release --manifest-path tools/mhfz2mhgu/Cargo.toml
tool=./tools/mhfz2mhgu/target/release/mhfz2mhgu
pac="${MHFZ_EM080_PAC:?请指定 FZ 的 em080.pac 文件路径}"
out=plugins/monster-espinas/target/fixtures
mkdir -p "$out/espinas" "$out/evidence"

"$tool" inspect "$pac"
"$tool" model "$pac" -o "$out/espinas/espinas.mod"
"$tool" motion "$pac" --group 0 -o "$out/espinas/espinas_group0.lmt"
"$tool" motion "$pac" --group 0 --slot 2 -o "$out/espinas/espinas_slot2.lmt"
```

`inspect` 列出根节点、动作组及有效槽位。`--slot 2` 的输出仍在 LMT 的槽位 2，其余槽位为空。同一组如有不同骨架的动作，可单独选择槽位。默认从 PAC 成员 `0/0` 读 FMOD、`0/1` 读 FSKL，从成员 `2` 读 MOT。若资产使用另一对 FMOD/FSKL，可用 `--model-member` 指定父成员；动作用 `--motion-member`。输入可以带 ECD、EXF 或 JKR 封装。输出路径的父目录应已存在，同名文件会被覆盖。

**模型。** 每个 FZ 对象写成一个 GU 绘制组，按三角带的实际材质拆分网格；保留绕序、法线和 UV，通过源骨骼调色板把权重映射到目标骨骼序号。MOD 内含父子关系、局部矩阵、逆绑定矩阵和骨骼包围体。使用 GU 的四权重顶点布局 `0x14D40022`；超过四个有效影响时会指出对应顶点。源材质编号对应 `fz_mat_000` 等材质名，单独导出的模型需要配套的 MRL 和 TEX；下述 `package` 会生成它们。顶点色、特殊着色和部位显隐规则未移植。

**动画。** 按源骨架的根和 motion tag 遍历节点并绑定轨道；计数相同的组有多个时，错误会列出候选，可用 `--root`（根表内的序号）和 `--tag` 消除歧义。支持普通棘龙使用的 `0x12`（整数）和 `0x22`（浮点）Hermite 曲线，按每个源整帧采样局部旋转、平移和缩放，再写成 GU 四元数或向量轨道。输出使用转换后的 MOD 骨架；不会将招式自动重定向到另一只 GU 怪物。默认不循环，游戏事件为空，命中帧、特效和 AI 动作号需要另行对应。

**外观包与冲锋试验。** 下面的命令从自己的 GU RomFS 读取材质模板和普通雌火龙资源。单独的外观包包含模型、一组动作、按 FMOD 图像 ID 去重的漫反射贴图和一张黑色高光贴图；支持每个材质绑定一张 PNG。使用原版 MRL 的不透明与裁剪模板写材质，TEX 为带 mipmap 的 Switch 块布局 RGBA8。`--name` 只指定资源路径，不注册新怪物。

```sh
enemy="${MHGU_ROMFS:?请指定 GU RomFS 根目录}/nativeNX/arc/enemy"
trial="$out/EspinasChargeTest/romfs/nativeNX/arc/enemy/em001_00.arc"
mkdir -p "$out/evidence" "$out/espinas" "$(dirname "$trial")"
"$tool" package "$pac" --name em150_00 --material-template "$enemy/em030_00.arc" -o "$out/espinas/em150_00.arc"
"$tool" charge-test "$pac" --donor "$enemy/em001_00.arc" --material-template "$enemy/em030_00.arc" -o "$trial"
python3 plugins/monster-espinas/tests/verify_espinas_package.py "$out/espinas/em150_00.arc" "$trial" "$enemy/em001_00.arc"
```

普通棘龙（FZ 80）的战斗入口是 `0:3`。`combat-choices` 读取 Vita ELF 的默认脚本和普通个体的距离配置；四个阈值为 450、1000、2700、5000，等于阈值仍属较近的一档。它保留常态/怒态、空中分支、32 个随机值，以及部分扩展脚本在夹角达到 20° 时发出的前置动作。输出是 FZ **动作请求序列**，不能把请求号当成 LMT 槽号。在同一战况下把每个动作立即返回给脚本，2,560 组输入的序列与原版 Vita 解释器一致（28 种请求、38 种序列）。此处没有模拟动作耗时、选新目标、进怒或换区。

```sh
"$tool" combat-choices mhfz/eboot.bin.elf -o "$out/evidence/combat-choices.csv"
"$tool" combat-motions "$pac" --donor "$enemy/em001_00.arc" -o "$out/espinas/espinas-combat.lmt"
# 常态、距离 451、面向目标、随机值 30 -> FZ (3,9)
trial_dir="$out/EspinasAction3_09/romfs/nativeNX"
mkdir -p "$trial_dir/arc/enemy" "$trial_dir/plugins"
"$tool" combat-replay "$pac" --donor "$enemy/em001_00.arc" --material-template "$enemy/em030_00.arc" \
  --distance 451 --random 30 -o "$trial_dir/arc/enemy/em001_00.arc"
(cd plugins/monster-espinas && cargo forge build --release)
cp plugins/monster-espinas/monster_espinas.nro "$trial_dir/plugins/"
```

`combat-motions` 将普通骨架的 100 个全身动作分别写在原 PAC 编号处，叠合各自的附属轨，骨号与移植后的 MOD 一致。槽 179、180、181、183、184、186 的主轨不是这套骨架的 29 轨，在输出中留空。这个 LMT 是供动作处理器按需调用的独立资源。

`combat-replay` 先执行 FZ 选招，再将**指定战况的一次结果**写进 GU 测试包。普通雌火龙的原版处理器只负责启动槽 4、主攻击 HDE 19 和结束回放。包不会读猎人的实时位置重新选招。未接好动作或有未处理前置动作时，生成器报出原序列，不抽第二次或替换成冲锋。

| FZ 请求 | 原版动作 | 近战登记（本动作第 0 帧起：帧 → FZ 效果号） |
| --- | --- | --- |
| 3:2 | 107 → 118 | 107：26 → 103、74；118：2 → 82、16 → 101、20 → 78 |
| 3:9 | 104 | 10 → 69 |
| 3:12 | 106 | 87、116 → 71，各自独立 |
| 3:15 | 117 | 36 → 81、66 → 80、70 → 79、78 → 83、92 → 81 |

`charge-test` 是 3:2 的独立试验。它按完整的 75 帧前冲和 159 帧收招播放；实际 FZ 会按目标及计时器提前转入收招。两段按原版动作 118 的默认混合参数衔接。其余动作各自使用原版整段姿势。判定几何、基础威力、状态值来自 Vita 表，不能据此认定 GU 伤害公式、远程弹体、风压或断尾已移植。

3:8 没有列入可生成的回放：动作 103 除第 2 帧的骨骼判定 53，还会在第 70 帧通过另一条位置参数入口生成效果 54（源关节 127，半径 400，零基础威力）。该位置在生成时取自怪物；不能直接绑到移动中的骨骼。转换器会指出这个缺口。

每个回放包都应带配套的 Forge 插件（GU 1.4），放在包内 `romfs/nativeNX/plugins/monster_espinas.nro`。插件从 HDE 末尾的标记读取本动作的判定数和断尾分组，为额外的每次登记分配独立对象；第一个登记沿用 GU 主攻击槽。原版 HDE 不带标记时不会创建额外判定。四个包都替换同一个雌火龙任务槽，应一次只启用一个。

身体判定另从 `--vita-elf`（默认 `mhfz/eboot.bin.elf`）读取。GU BDD 含 27 个棘龙受击体和 1 个雌火龙原有的角色辅助球；骨号按移植后的 MOD 重映射，形状、半径、端点、肉质索引和部位索引来自 FZ。尾尖记录保留状态位 1 的禁用条件，但断尾流程和掉落模型没有移植。接触标志借用 GU 同部位记录；实际肉质数值、耐久和伤害仍由雌火龙资源提供。

锁定点使用 `dttune` 的近、远目标记录。原供体四条记录都绑在 0 号骨并向下偏移，换骨架后会落在地面下。回放包改绑到源躯干 3 和颈部 30，并用移植后的身体体积生成镜头碰撞。待机和冲锋共 415 帧已离线检查。要看屏幕上的锁定或实际伤害，仍需在任务中观察。

独立验证（Unicorn 2.1.4；包内资源回读本身只依赖 Python 标准库）：

```sh
python plugins/monster-espinas/tests/verify_espinas_choices.py "$out/evidence/combat-choices.csv"
python plugins/monster-espinas/tests/verify_espinas_contacts.py "$out/EspinasChargeTest" "$out"/EspinasAction3_*
python plugins/monster-espinas/tests/verify_espinas_package.py "$out/espinas/em150_00.arc" "$trial" "$enemy/em001_00.arc" --trace "$out/evidence/charge-trace.csv"
```

`verify_espinas_contacts.py` 离线执行实际 NRO、GU 原生 HDE 读取、攻击登记、计时及队列；使用每步 0.5、1、2 个动作帧，先更新原生时钟，再调用 GU 的碰撞资格判断；检查活动区间、断尾切换、中断、重新登记和释放。插件在已登记实例耗尽持续时间时立即清除启用位，以免 GU 将它再保留一次碰撞遍历。它模拟资源引用和游戏外部服务，没有执行猎人受击。轨迹 CSV 中 `primary_attack_enabled` 只表示第一个攻击实例，`active_fz_effects` 列出当帧全部活动实例。

**行为扫描。** 从 Vita 原版怪物选择器的 Thumb 指令读取配置表地址，通过 ELF 的 PT_LOAD 段把虚拟地址换算成文件位置。默认使用 bank 0，从组 0 槽 0 出发。按指令边界跟随状态跳转（`07`）、子脚本（`81`）、表 9（`16`）和扩展表（`82`）引用；保留原组号和槽号。列表里的 `?` 有对应诊断。FZ 指针表没有槽数，这里列的是**从入口可达的组和槽**；未被引用的表尾、原生函数才选择的脚本、外部 EMD 的运行时替换不会被猜成静态脚本。可用 `--bank` 选择其他配置，或用 `--entry 组:槽` 从已知的原生入口开始。当前支持这版 Vita ELF 的静态怪物表（ID 0–131）。

```sh
# 列出棘龙从默认入口可达的分组，不写文件，也不需要清单
"$tool" emc mhfz/eboot.bin.elf --monster 80
# 从状态 3 自动读出子脚本调用与动作，生成可单独验证的样本
"$tool" emc mhfz/eboot.bin.elf --monster 80 --entry 0:3 --action-sample -o "$out/evidence/espinas.emc"
```

直接指定 `-o` 会尝试完整翻译所选入口及其引用，遇到未映射指令报组、槽和地址，不写部分结果。目前可翻译主脚本调用 `81 slot`、动作 `05 group id 00`，以及匹配执行层级的主脚本结束、子脚本返回。棘龙整套 AI 仍有未映射的条件。`--action-sample` 则明确只取一条已解码的动作请求，优先保留一次子脚本调用，输出从 GU 的 `0:0` 开始，不表示原来的条件或概率。

**代码位置。** Cargo workspace 包含两个可独立使用的库：[mhf-resource](crates/mhf-resource/src/lib.rs) 读取 FZ 的 PAC、FMOD、FSKL、MOT 和行为脚本，其中 [ai](crates/mhf-resource/src/ai.rs) 负责 ELF 扫描；[mhgu-resource](crates/mhgu-resource/src/lib.rs) 负责 Switch MOD、LMT、EMC、TEX、MRL 和 ARC 格式。两库互不依赖，CLI 负责材质命名、权重映射、曲线采样和行为指令翻译。

**验证。** 在普通棘龙 `em080.pac` 上生成并独立回读了全部 7,705 个顶点、48 根骨骼、22 个网格、第 0 组的 62 个非空动作，以及其中 4 个动作的逐帧采样。用 MHGURecomp 0.15.3 的隔离存档副本进入集会所二星「女王降临」后，在溪流 8 区确认了替换实例、HDE 19 的攻击窗口和动作帧重复回到开头。白色剪影已由使用者在游戏中确认修复。新版身体、镜头和攻击判定已离线回读；四个回放包的任务内命中、实际位移和伤害未验收。可用自己的同版 PAC 运行回归检查：

```sh
cargo test --workspace --manifest-path tools/mhfz2mhgu/Cargo.toml
MHFZ_EM080_PAC="$pac" MHFZ_VITA_ELF="$PWD/mhfz/eboot.bin.elf" MHGU_ENEMY_ARCS="$enemy" cargo test --manifest-path tools/mhfz2mhgu/Cargo.toml -p mhfz2mhgu -- --ignored
```

生成的棘龙 EMC 可用仓库的 [离线探针](../../plugins/monster-espinas/tests/verify_espinas_emc.py) 验证：在装有 Unicorn 的 Python 环境运行 `python plugins/monster-espinas/tests/verify_espinas_emc.py "$out/evidence/espinas.emc"`。探针只执行到原版动作入口，不会运行一场狩猎。

GU 布局对照了本机原版怪物资源、[MOD 读取器](https://github.com/Scrimas/MHGU-Definitive-Edition/blob/main/tools/mod.py) 和 [LMT 读取器](https://github.com/Scrimas/MHGU-Definitive-Edition/blob/main/tools/lmt.py)。动画编码可对照 [RevilLib](https://github.com/PredatorCZ/RevilLib/blob/master/src/mtf_lmt/codecs.hpp) 与 [MHGU Blender 导入器](https://github.com/DemonDrugd/MHGU-blenderImporter/blob/main/MHGU_blenderImporter_v1_3.py)。
