# style-registry

在 MHGU 1.4 的装备箱打开原版「切换风格」页面，当前武器已注册的风格会列在六个原版风格之后。用原版光标选择并确认；取消或退出不会改变已装备风格。选择原版风格会恢复 GU 的动作。超过八项时可左右翻页。

从仓库根目录运行 `python3 plugins/style-registry/package.py`，把生成的 `dist/style-registry` 和需要的武器风格插件分别安装到 mods 目录。装备箱的选择立即生效并保存到 Forge 的 `savedata:/style_registry.ini`（MHGURecomp 通常为 `save/save_0/style_registry.ini`）。该设置在角色之间共享；自定义编号不会写入 GU 的装备数据。也可关闭游戏后直接编辑：

```ini
[General]
style = 1
```

| 配置值 | 实际效果 |
| --- | --- |
| `0`（默认） | 按 GU 装备的原版风格游玩。 |
| `1` | 安装极型大剑且装备大剑时启用它已实现的动作。 |
| 其他已注册编号 | 装备该插件声明的武器时启用。 |
| 未注册、格式错误或武器不匹配 | 本次按 `0` 处理；不覆盖配置，切回匹配的武器会重新生效。 |

**添加风格。** 通过 [SDK 风格注册表](../../crates/mhgu-sdk/src/style/registry.rs) 声明一个常驻的 `static Style`，填入独占编号（1–65535）、GU 武器类型和可选的动作回调。让注册器先初始化。内容插件像[极型大剑](../great-sword-extreme/src/lib.rs)一样，在 Forge 初始化入口取得 `StyleRegistry::current()`，调用一次 `registry.register_named(&STYLE, name, description)` 提供菜单名称和说明。旧插件的 `register` 仍可用，默认显示 `Style <编号>`。注册器缺失或初始化太晚时，本次启动不会补注册。编号冲突返回 `Conflict`，保留先注册的声明。用 `registry.selected(weapon)` 查询实际启用的编号。

极之型大剑在初始化时用 `register_controller` 一次登记菜单文字、动作映射、战斗回调及原生请求回调。注册器直接传递 GU 公共指令表、武器指令表和摇杆；键位映射与组合键识别由 GU 完成。原生请求回调在 GU 接受拔刀等武器请求后，指定本风格执行的招式，因此无需另行枚举站立、行走和奔跑状态。控制器用 `style::action::enter(actor, move_id)` 切换自己的派生；招式编号 0–255 对应武器动作组内的 `0x8000..0x80ff`。未接收的原版武器请求会被拒绝，公共移动、受击等动作可返回 `PASS` 交给 GU。切出风格时以空输入通知插件退出，旧猎人失效时 actor 为 0。接口未正式发布，保持 V1。

需要独立规则但不需要接管输入的插件仍可用 `register_independent`；以上两种方式都停用狩技和勇气槽。GU 内部固定长度的风格表继续使用合法索引 0，避免把插件编号传入六项原版表；控制器的招式选择和派生不使用该索引。不需要独立规则的 `register_named` 保留原版机制。

构建注册器和[极型大剑](../great-sword-extreme/README.md)后，用装有 Unicorn 的 Python 运行 `plugins/great-sword-extreme/tests/verify_style.py` 和 `verify_menu.py`。后者执行 GU 的列表光标、确认框和原版装备写入，覆盖选择、取消、跨页、保存以及切回原版。使用内存文件和渲染替身，不接触玩家存档。
