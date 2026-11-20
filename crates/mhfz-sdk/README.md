# mhfz-sdk

极之型武器在 MHGU 1.4 中共用的 Frontier 动作与表现逻辑。武器插件提供各自的动作表、招式、阈值和资源路径。调用 GU 原生动作、地形、粒子、碰撞和声音时使用 [mhgu-sdk](../mhgu-sdk/README.md) 的引擎接口。

| 模块 | 职责 |
| --- | --- |
| [action](src/action.rs) | 按 Frontier 片段帧、混合与循环执行动作事件，缓冲派生请求。 |
| [combat](src/combat.rs) | 按 Frontier SDT 计算刀身判定的延迟与剩余有效帧。 |
| [motion](src/motion.rs) | 把 Frontier 1000、1200、1400 三组片段映射到 GU 的 LMT 库及槽，保留持刀回避的左右派生。 |
| [sprint](src/sprint.rs) | 按 `81ECC418` 识别同向双推及第二推方向修正。共用急停和经过 GU 攀爬、落地动作后的恢复规则。 |
| [effects](src/effects.rs) / [sound](src/sound.rs) | 读取 Frontier 粒子和声音时间线。粒子按片段的循环点分段扫描，分别计算发射帧与挂点。 |
| [trail](src/trail.rs) | 根据历史刀身姿态插值 Frontier 刀轨，供 style-registry 的 GU 粒子绘制钩子提交。 |

三把武器和 style-registry 在构建时链接各自需要的代码。安装仍使用现有四个 NRO。动作、音效和粒子资源由各插件的打包流程生成。
