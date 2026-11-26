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
