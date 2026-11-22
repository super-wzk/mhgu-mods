# MHF-Z 极之型单手剑 → MHGU 1.4

装备单手剑，在装备箱的「切换风格」中选择「极之型单手剑」。三个狩技槽沿用 GU 的装备和充能规则。要使用真空回旋斩，装备任意等级的混沌刃药，发动狩技并完成涂油，再持刀推摇杆按 A。普通刃药仍按 GU 的规则使用。

## 操作

| 场景 | GU 输入 | 动作 |
| --- | --- | --- |
| 持刀站立 | X，再按 X | 起手斩接地面剑连段。部分剑招可按 R＋X 进入循环斩击。 |
| 持刀站立或兼容的剑招派生窗口 | A / 推摇杆＋A | 摇杆回中时出跳跃二连斩，推摇杆时出回旋斩。 |
| 持刀站立 | X＋A | 跃进斩，派生窗口再按 X＋A 可接回避攻击。 |
| 持刀站立或剑招派生窗口 | R＋A，继续按 A | 进入连续盾击。摇杆回中时原地连打，向前推可边打边向前移动。盾击积累眩晕。 |
| 剑招派生窗口 | R＋B，并向左、右或后推摇杆 | 对应方向的回避攻击。接 X、A 或 X＋A 可转入下一招。 |
| 持刀移动 | 同向快速推两次摇杆 | 拔刀冲刺。冲刺中按 X 出跳跃二连回旋攻击，按 A 出冲刺盾击，松开摇杆制动，按收刀键可直接收刀。 |
| 从台阶腾空后按 X（含空中拔刀） | 出持刀冲刺 X 的跳跃二连回旋斩。动作的水平位移与出招时继承的水平惯性各减半。第 16、52 动作帧各有一次独立斩击，动作值为 18、34。只有第二击使用 GU 单手剑原版跳劈的骑乘判定，提前着地时也能打完第二击。 |
| 持刀防御 | R，防御中按 X | GU 处理格挡，X 接防御后的斩击。 |

## 真空回旋斩

FZ 单手剑为武器 4（`motion/w04.mot`），在 GU 对应武器 1，注册器风格编号为 5。回旋斩是 FZ 状态 2、片段 1405。刀身动作值为 31，第 30 动作帧的事件 13 调用 `81E97DDC` 产生族 87、子型 1 的真空波。移植将此事件绑定在 GU 混沌刃药 I、II、III 的有效期内。涂油中断或波出现前效果结束时，只结算刀身。

三个等级使用同一组 FZ 真空波参数。波从角色上方 100 单位处沿出招方向前进，每个 FZ 逻辑刻移动 10 单位，持续 17 刻，球形判定半径 200，动作值 16。刀身和波分别命中，波生成后可以离开原地。GU 自身负责三个等级的持续时间、叠油和狩技效果。队友回放读取 GU 动作包同步的刃药标记。

片手模型粒子由注册器接入与斩斧相同的绘制处理。带有 FZ 深度参数的层按各自的 `0.03` 或 `0.01` 淡化。加法和透明混合层的 RGB 乘以 `0.7`，真空波也应用这项亮度校正。

## 构建与离线回放

从仓库根目录运行，`MHFZ_DAT` 指向 FZ 的 dat 目录，`MHGU_ROMFS` 指向包含 `nativeNX` 的 GU RomFS 根目录。

```sh
python3 plugins/style-registry/package.py
python3 plugins/sword-shield-extreme/package.py "$MHFZ_DAT" "$MHGU_ROMFS"
```

把注册器及 `plugins/sword-shield-extreme/dist/sword-shield-extreme` 安装到游戏的 mods 目录。已安装极之型太刀时，一并更新本版本太刀的 NRO，共用碰撞派发由注册器提供。

在装有 Unicorn 的 Python 环境中设置 `MHGU_ROMFS`，按[开发指引](../../docs/development.md)准备本地 ELF 后运行下列回放。脚本逐项核对 FZ 的 48 个非空起手，并回放涂油、换区和队员动作。`verify_lifecycle.py` 使用 GU 原生按键映射检查组合键、回避交接、冲刺落地和集会所切换。`verify_contacts.py` 用原版命中数据与打包的碰撞形状检查刀身、真空波的独立扣血和盾击的眩晕值。

```sh
python3 plugins/sword-shield-extreme/tests/verify_port.py
python3 plugins/sword-shield-extreme/tests/verify_lifecycle.py
python3 plugins/sword-shield-extreme/tests/verify_contacts.py
python3 plugins/sword-shield-extreme/tests/verify_particles.py
```
