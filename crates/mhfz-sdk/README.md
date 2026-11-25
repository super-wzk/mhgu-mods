# mhfz-sdk

极之型武器在 MHGU 1.4 中共用的 Frontier 动作与表现逻辑。武器插件提供各自的动作表、招式、阈值和资源路径。调用 GU 原生动作、地形、粒子、碰撞和声音时使用 [mhgu-sdk](../mhgu-sdk/README.md) 的引擎接口。

| 模块 | 职责 |
| --- | --- |
| [action](src/action.rs) | 按 Frontier 片段帧、混合与循环执行动作事件，缓冲派生请求。可用按下时的动作采样判断延后确认的指令。 |
| [combat](src/combat.rs) | 按 Frontier SDT 计算命中延迟和剩余有效帧，提供弹刀阈值及 FZ 格挡、回避窗口的技能补正。 |
| [camera](src/camera.rs) | 按 Frontier 震动类型计算振幅、距离衰减和时序。 |
| [motion](src/motion.rs) | 把 Frontier 1000、1200、1400 三组片段映射到 GU 的 LMT 库及槽，支持任务初始化时统一预加载及持刀回避的左右派生。 |
| [sprint](src/sprint.rs) | 按 `81ECC418` 识别同向双推及第二推方向修正。共用急停和经过 GU 攀爬、落地动作后的恢复规则。 |
| [effects](src/effects.rs) / [sound](src/sound.rs) | 读取 Frontier 粒子和声音时间线。`MotionClock` 按 `[当前姿势帧, 下一播放帧)` 扫描特效事件，`world_pose_angles` 将取样姿势转换为 GU 的世界粒子旋转角。 |
| [effects::model](src/effects/model.rs) | 将 Frontier 模型粒子的发射镜像、RGB 增益和场景深度淡化接入 GU 绘制。 |
| [trail](src/trail.rs) | 根据历史刀身姿态插值 Frontier 刀轨，在模型粒子绘制回调中提交。 |

风格注册器发布 GU 粒子绘制服务后安装共用的 FZ 模型粒子适配。武器插件通过该服务登记各自的刀轨。动作、音效和粒子资源由各武器插件的打包流程生成。
