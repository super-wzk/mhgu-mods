# MHGU 移植开发

本仓库为 MHGU Switch 1.4 提供 Rust 引擎绑定、Forge 注册插件和 Frontier 资源转换工具。

[查看更新日志](CHANGELOG.md)

## 开发文档

- [开发指引](docs/development.md)介绍仓库结构、本地输入、构建、安装和验证。
- [武器风格移植流程](docs/weapon-style-porting.md)从源招式调查走到 GU 动作、输入、命中和表现接入。
- [极之型武器实现与回归](docs/weapon-style-implementation.md)记录各武器的派生、槽、狩技、特效和联机行为，并链接对应实现及回放。
- [常见故障与排查](docs/weapon-style-troubleshooting.md)记录已遇到的问题、可观察的证据和修复位置。
- [AGENTS.md](AGENTS.md)提供 agent 开工时需要的项目约定和文档入口。

武器的当前操作见各插件 README，也可在游戏内从猎人笔记的「武器操作指南」或暂停菜单的「操作手册」查看。棘龙的独立任务、安装及验证见[棘龙插件说明](plugins/monster-espinas/README.md)。
