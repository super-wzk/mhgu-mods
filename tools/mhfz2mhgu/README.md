# mhfz2mhgu

转换 Frontier 武器的动作、粒子、音频和 HUD，供 GU 1.4 风格插件打包使用。CLI 还支持检查 PAC、导出通用模型和动作，以及构建包含外观资源的 ARC。

```sh
cargo run --release --manifest-path tools/mhfz2mhgu/Cargo.toml -- --help
cargo test --workspace --manifest-path tools/mhfz2mhgu/Cargo.toml
```

武器构建使用各插件的打包脚本，输入为 Frontier 的 `dat` 目录及 GU RomFS。见[开发指引](../../docs/development.md#本地输入和构建)和[武器风格移植流程](../../docs/weapon-style-porting.md#2-转换资源并校正时序)。

[mhf-resource](crates/mhf-resource) 读取 Frontier 的 PAC、模型、骨架和动作。[mhgu-resource](crates/mhgu-resource) 读写对应的 GU 资源。工具使用本地游戏输入，生成物由调用方指定输出路径。


`weapon-model` 从 MHA 武器包按成员名取出 PAC，将 FMOD 网格、FSKL 骨骼和原贴图转换为 GU 的 MOD、MRL 和 TEX。磁斩锤打包脚本提供资源路径与材质模板。[磁斩锤构建](../../plugins/magnet-spike/README.md)

`magnet-spike-icon "$MHFZ_DAT" -o OUTPUT.tex` 从本地 MHF-Z 的 `mhf.bin` 原版武器图集读取磁斩锤图标，保留 64×64 像素和透明度，编码为 GU TEX。

`magnet-spike-battle` 还从同一 `mhf.bin` 读取组 4 的 0x3e–0x47 序列、磁缚槽底和准星图页，将形态、回充、磁缚计时和瞄准图稿编码为 GU 贴图，写入磁斩锤动作 ARC。

`magnet-spike-battle "$MHFZ_DAT" -o OUTPUT_DIR` 将 `motion/w13.mot` 的猎人动作、`motion/w13w.mot` 的武器部件动作与 HTS 写入 `motions.arc`。部件按 `wk451.bin` 中的 11 骨骼模型转换，保留源槽位、空槽和循环起点。近战 HTS 对应判定族 120 的选择器 1 至 8，保留胶囊的半径、端点和挂点。强化磁冲击的 HTS 对应判定族 121 的选择器 12，运行时按固定世界坐标展开。磁缚终结的 HTS 对应同一族的选择器 8，保留武器节点 7 上的半径 700 球形判定。传入 `--material-template` 时，输出同时包含模型、材质和贴图，文件名为 `magnet_spike.arc`。`magnet_spike.fmsa` 包含 `mhfdat.bin` 中的 165 个动作状态及 `mhfsdt.bin` 中判定族 120、121 的记录与形状。`MSHT` 段按选择器保留原始形状列表，`MSCT` 段保存各状态的磁力消耗格数，`MSPR` 段保存武器参数和状态形态分类。第 7 版在 `MSCT` 前用 `MSTR` 保存各状态的转向上限和禁转标记。GU 运行时使用怪物麻痹参数判定磁缚资格，各任务共用同一条磁缚耐性曲线。

磁斩锤的 `we13.snd`、`common01.snd`、型号音库 `wk416.snd` 和男女猎人默认语音 按用到的请求生成 ADPCM、SBKR 和 SRQR，依次装入同一 ARC。输出的 `frontier_ms_sound.flse` 保存动作帧、音效库与加权请求选项，和动作表一起编入 NRO。

给 `magnet-spike-battle` 传入 `--gu-weapon "$MHGU_ROMFS/nativeNX/arc/player/quest/w00.arc"` 时，从 `effect.bin` 转换磁斩锤特效。4037:11、4037:14 分别供磁界弹和部位标记使用，4038:34 供回避成功的武器闪光使用。4038:45–48 与 4037:84 用于强化磁冲击。4038:50–52 与 4037:83 用于普通磁冲击。闪光的短定义 UV 模式 4 按 FZ `81F4BAA4` 转为 GU 的阶梯 UV 曲线。原始网格和贴图以及 GU 的 EFL、PEL 随动作 ARC 打包，`effects/routes.bin` 随动作表编入 NRO。旋转曲线保持原版每 540 帧一圈，供长期显示的部位标记使用。另从 `mhfdat.bin` 的 w13 动作特效表提取整张动作表中所有可执行的粒子行到 `effects/motion_fx.bin`，随对应粒子路由编入磁斩锤 NRO。4037:45、46、48 用于怪物受缚和解除。出招者专用的 4037:42、44 和 4038:30 也写入同一武器 ARC，we13 请求 18、19 写入磁斩锤的音效库。

同一转换还输出 4037 组第 21、22、70、71 项及 4035／4036 组第 2 项。两种格挡的火花使用第 21 项，按 FZ `8102CDDC` 中的局部坐标及原生出生延迟写入 GU 粒子定义。

磁力反弹需要 4037 组第 1、77、78 项。转换器分别输出武器闪光、定点闪光和脉冲的粒子定义，插件按请求 11／12 的时序提供挂点与世界坐标。

磁缠强化的粒子定义来自 4038 组第 16 项和 4037 组第 85–88 项，按启动闪光、形态脉冲和形态火花分别写入选择器表。
