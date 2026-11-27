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

`magnet-spike-battle` 还从同一 `mhf.bin` 读取组 4 的动画 0x41、0x42，将消耗和回充的关键帧烘焙为 GU 贴图，写入磁斩锤动作 ARC。

`magnet-spike-battle "$MHFZ_DAT" -o OUTPUT_DIR` 将 `motion/w13.mot` 的猎人动作、`motion/w13w.mot` 的武器部件动作与近战 HTS 写入 `motions.arc`。部件按 `wk451.bin` 中的 11 骨骼模型转换，保留源槽位、空槽和循环起点。近战 HTS 对应判定族 120 的选择器 1 至 8，保留胶囊的半径、端点和挂点。传入 `--material-template` 时，输出同时包含模型、材质和贴图，文件名为 `magnet_spike.arc`。`magnet_spike.fmsa` 包含 `mhfdat.bin` 中的 165 个动作状态及 `mhfsdt.bin` 中判定族 120、121 的记录与形状。第 4 版中，`MSHT` 段按选择器保留原始形状列表，`MSCT` 段保存各状态的磁力消耗格数，`MSPR` 段保存武器参数和状态形态分类。

磁斩锤的 `we13.snd` 与 `common01.snd` 按用到的请求生成 ADPCM、SBKR 和 SRQR，依次装入同一 ARC。输出的 `frontier_ms_sound.flse` 保存动作帧、音效库与加权请求选项，和动作表一起编入 NRO。

给 `magnet-spike-battle` 传入 `--gu-weapon "$MHGU_ROMFS/nativeNX/arc/player/quest/w00.arc"` 时，从 `effect.bin` 的 4037 号特效组转换第 11、14 项，分别供磁力弹和部位标记使用。原始网格和贴图以及 GU 的 EFL、PEL 随动作 ARC 打包，`effects/routes.bin` 随动作表编入 NRO。旋转曲线保持原版每 540 帧一圈，供长期显示的部位标记使用。

同一转换还输出 4037 组第 21、22、70、71 项及 4035／4036 组第 2 项。两种格挡的火花使用第 21 项，按 FZ `8102CDDC` 中的局部坐标及原生出生延迟写入 GU 粒子定义。
