# style-registry

在 MHGU 1.4 的装备箱打开原版「切换风格」页面，当前武器已注册的风格会列在六个原版风格之后。用原版光标选择并确认；取消或退出不会改变已装备风格。选择原版风格会恢复 GU 的动作。超过八项时可左右翻页。

注册器启用后会屏蔽原版附加肩键的聊天快捷键（默认 ZR，随 GU 肩键设置交换）。仍可从原版菜单或触屏打开聊天，极型招式继续读取附加肩键。

从仓库根目录运行 `python3 plugins/style-registry/package.py`，把生成的 `dist/style-registry` 和需要的武器风格插件分别安装到 mods 目录。装备箱的选择立即生效并保存到 Forge 的 `savedata:/style_registry.ini`（MHGURecomp 通常为 `save/save_0/style_registry.ini`）。该设置在角色之间共享；自定义编号不会写入 GU 的装备数据。也可关闭游戏后直接编辑：

```ini
[Weapons]
0 = 1
11 = 2
7 = 3
8 = 4
```

| 配置值 | 实际效果 |
| --- | --- |
| `0`（默认） | 按 GU 装备的原版风格游玩。 |
| `1` | 极之型大剑，对应武器编号 `0`。 |
| `2` | 极之型双剑，对应武器编号 `11`。 |
| `3` | 极之型太刀，对应武器编号 `7`。 |
| `4` | 极之型斩斧 F，对应武器编号 `8`。 |
| 缺项、未注册、格式错误或武器不匹配 | 当前武器按 `0` 处理。 |

每种武器分别保存。只有旧 `[General]` 段的配置按空配置处理，确认一次风格选择后写入新格式。

通过 [SDK 风格注册表](../../crates/mhgu-sdk/src/style/registry.rs) 声明常驻 `static Style`，填入独占编号（1–65535）、GU 武器类型、动作及回调。注册器先初始化，内容插件像[极型大剑](../great-sword-extreme/src/lib.rs)一样，在 Forge 初始化入口使用 `registry.register(&STYLE, name, description)` 登记一次。编号冲突返回 `Conflict`，保留先注册的声明。用 `registry.selected(weapon)` 查询实际启用的编号。

V1 注册服务同时提供攻击提交与格挡结果。GU 从武器动作组派发已登记的 `0x8000..0x80ff` 槽位。风格根据 GU 已接受的拔刀或切招请求选择招式，并自行定义动作、派生窗口和格挡规则。注册器负责大剑切招、狩技接续、下一击强化与格挡收尾的原生对接。切出风格时用空输入通知控制器，actor 为 0 表示旧猎人已失效。

`art_slots` 指定狩技槽位数（0–3），装备、充能和发动由 GU 处理。`independent` 使 GU 的固定长度风格表使用索引 0，并停用勇气槽。仅替换动作资源的风格可以省略控制器。

四种极之型使用同一个[预读入口](../../crates/mhgu-sdk/src/style/preload.rs)。GU `29B7D4` 完成任务初始化后，注册器只调用本地猎人当前风格的预读回调。各武器准备当前模型的三组动作库、本地粒子表、音库和使用的私有命中形状与 HUD 贴图。预读沿用原生资源缓存，动作库在实际播放时绑定到猎人。

武器插件通过 [ParticleModelV1](../../crates/mhgu-sdk/src/style/particle.rs) 登记模型粒子绘制回调。注册器负责保存和恢复 GU 的绘制状态，并派发粒子释放回调。

构建注册器和[极型大剑](../great-sword-extreme/README.md)后，用装有 Unicorn 的 Python 运行 `plugins/great-sword-extreme/tests/verify_style.py` 和 `verify_menu.py`。后者执行 GU 的列表光标、确认框和原版装备写入，覆盖选择、取消、跨页、保存以及切回原版。使用内存文件和渲染替身，不接触玩家存档。

内容插件可通过 [StyleRulesV1](../../crates/mhgu-sdk/src/style/rules.rs) 登记格挡半角、公共动作结束条件、弹刀与镜头回调。注册器在 GU 近战解算后按猎人、动作槽和刀身的打击序号匹配 `Attack::DEFLECTABLE` 标记的命中，并用 GU 的斩味修正、心眼与相关豁免完成判定。动作控制器通过 `mhgu::BladeDeflectionV1` 取得结果。
