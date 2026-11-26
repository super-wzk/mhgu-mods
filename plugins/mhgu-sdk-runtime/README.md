# MHGU SDK 运行时

Forge 插件由 `mhgu-sdk-runtime` 构建，发布为 `mhgu-sdk.nro`。它提供武器风格注册和角色存档服务。Rust 模组通过 [mhgu-sdk](../../crates/mhgu-sdk/README.md) 注册风格。

从仓库根目录运行 `python3 plugins/mhgu-sdk-runtime/package.py`，生成开发安装目录 `plugins/mhgu-sdk-runtime/dist/mhgu-sdk/`。武器整合包由仓库级工具组装，包内的 SDK 目录随附 `arc/frontier/common.arc`。见[整合包构建流程](../../docs/development.md#本地输入和构建)。

游戏只需安装一份 SDK 模组。升级时更新 SDK 和武器模组。武器的固定位置碰撞继续使用 SDK 统一挂接的 GU 碰撞展开入口 `1696FC`。

## 武器风格

在 MHGU 1.4 的装备箱打开原版「切换风格」页面，当前武器已注册的风格会列在六个原版风格之后。用原版光标选择并确认；取消或退出不会改变已装备风格。选择原版风格会恢复 GU 的动作。超过八项时可左右翻页。

个人状态和队友状态的概要、详情使用实际装备对应的风格名称。队友名称随 GU 的装备资料同步，双方需要加载支持风格资料同步的 SDK，查看方需要安装对应风格。未收到可识别的风格编号时，界面使用 GU 原有名称。

在装备箱登记或覆盖装备组合时，SDK 记录当时的风格。概要和详情显示该组合保存的风格，穿上组合后恢复。原版组合或缺少已记录插件时沿用组合的 GU 原版风格。调换组合时风格随组合移动，删除或覆盖时清除旧记录。斗技场固定装备预览和公会名片历史沿用 GU 记录。

注册器启用后会屏蔽原版附加肩键的聊天快捷键（默认 ZR，随 GU 肩键设置交换）。仍可从原版菜单或触屏打开聊天，极型招式继续读取附加肩键。

猎人笔记的「武器操作指南」按武器列出已安装的极之型手册。在原有风格之后选中极之型，即可用原版翻页操作阅读。暂停菜单的「操作手册」中按确认键打开同一组说明，方向键左右翻页、上下切换武器，取消键返回。手册使用简体中文，可以在装备该风格之前查阅。

手册是风格插件的可选项。风格登记成功后可调用 `mhgu_sdk::style::manual::bind(id, include_str!("manual.txt"))`。UTF-8 文件用空行分页。操作页首行为标题，以下每行用一个制表符分隔操作与招式，最多九行。说明页支持 GU 的 `<ICON>`、`<SIZE>` 和 `<COL>` 标记。两种页面均可写 `{X}`、`{A}`、`{B}`、`{Y}`、`{R}`、`{摇杆}` 显示原版按键图标。

## 角色存档

装备箱的选择立即生效。GU 角色存档保存成功后，SDK 将各武器当前的风格和各装备组合的自定义风格写入该角色槽的 JSON。文件位于 Forge 的 `savedata:/mhgu-sdk-<角色槽>-<0或1>.json`。两份文件交替写入，加载时使用最新的有效世代。若两份都无法读取，本次沿用原版并保留文件。旧 `style_registry.ini` 已停用。

JSON 版本 1 包含 `slot`、`generation` 和 `data`。其中 `weapons` 按 GU 武器编号排列，`null` 沿用 GU 原版。 `equipment_sets` 按装备组合槽记录自定义风格 ID、武器编号和用于核对 GU 装备组合的指纹。缺少自定义记录的组合沿用原版。

| 风格 | GU 武器编号 | 自定义风格 ID |
| --- | --- | --- |
| 极之型大剑 | `0` | `10` |
| 极之型双剑 | `11` | `11` |
| 极之型太刀 | `7` | `12` |
| 极之型斩斧F | `8` | `13` |
| 极之型单手剑 | `1` | `14` |
| 极之型铳枪 | `9` | `15` |

通过 [SDK 风格注册表](../../crates/mhgu-sdk/src/style/registry.rs) 声明常驻 `static Style`，填入角色存档和联机共用的全局唯一编号（10–65535）、GU 武器类型、动作及回调。内容插件在 Forge 入口调用 `registry::when_ready`，在回调中使用 `registry.register(&STYLE, name, description)` 登记一次。编号冲突返回 `Conflict`，保留先注册的声明。用 `registry.selected(weapon)` 查询启用的自定义编号，沿用原版时返回 `None`。更新时将 SDK 和武器 NRO 一起替换。

风格注册服务同时提供攻击提交与格挡结果。GU 从武器动作组派发已登记的 `0x8000..0x80ff` 槽位。风格根据 GU 已接受的拔刀或切招请求选择招式，并自行定义动作、派生窗口和格挡规则。注册器负责大剑切招、狩技接续、下一击强化与格挡收尾的原生对接。切出风格时用空输入通知控制器，actor 为 0 表示旧猎人已失效。

联机由 [multiplayer 模块](src/style/multiplayer.rs) 转接 GU 的 `netPlAction` 收发、远端风格查找、私有动作与逐帧观察派发。风格注册成功后通过 [`remote::bind_playback`](../../crates/mhgu-sdk/src/style/remote.rs) 登记一次回放描述符。注册器按当前名册和远端猎人的装备武器选择回放。每把武器负责自己的动作、附加状态值与效果清理。动作包格式沿用现有 GU 布局。运行时与武器 NRO 使用同次构建的成品。

[风格资料同步](src/style/profile.rs)通过 `netPlData::mAttr` 携带完整的注册编号，保留 GU 的装备风格字节和回包请求位。注册器核对接收槽位、当前装备记录和武器类型后再显示名称。替换装备资料或离开房间会清除对应记录。

带冲刺收刀的风格通过 `remote::bind_sheathe` 登记冲刺动作与两种收刀片段。双方使用本版时，队友可直接从收刀包确定片段。未携带收刀标记的旧版动作包沿用接收端的动作历史。[冲刺收刀联机回放](tests/verify_sheathe_multiplayer.py)覆盖六种极之型、错过冲刺或刹停、两名队友以及收刀结束。

`art_slots` 指定狩技槽位数（0–3），装备、充能和发动由 GU 处理。`independent` 使 GU 的固定长度风格表使用索引 0，并停用勇气槽。仅替换动作资源的风格可以省略控制器。

发布版首次请求某把武器的私有路径时，注册器先通过 GU `BCAD18` 读取 `common.arc`，再读取对应武器 ARC。两者保留在资源缓存中。GU 的 `B39F58` 按归档顺序加载依赖，供当前猎人和队友后续的原生路径请求复用。详见[打包回归](../../tools/tests/verify_packed_release.py)。

极之型武器使用同一个[预读入口](../../crates/mhgu-sdk/src/style/preload.rs)。GU `29B7D4` 完成任务初始化后，注册器只调用本地猎人当前风格的预读回调。男女模型共用的三组动作库和私有命中形状同步准备，供入场后的首次攻击使用。PEL、SRQR 和 HUD TEX 交给 GU 资源加载线程，原生资源标记完成后才能使用。动作库在实际播放时绑定到猎人。

武器插件通过 [ParticleModelV1](../../crates/mhgu-sdk/src/style/particle.rs) 登记模型粒子绘制回调。注册器初始化共用的 FZ 模型粒子渲染器，负责保存和恢复 GU 的绘制状态，并派发粒子释放回调。

构建注册器与武器模组后，用装有 Unicorn 的 Python 运行 `plugins/great-sword-extreme/tests/verify_style.py` 和 `verify_menu.py`。后者回放装备箱选择、角色槽隔离、GU 保存失败、A/B 回退、装备组合保存和应用、概要及详情、个人与队友状态及装备资料收发。使用内存文件、渲染替身和传输替身。

按[开发指引](../../docs/development.md#验证范围)打包全部武器后，运行 `python3 plugins/mhgu-sdk-runtime/tests/verify_weapon_styles.py`。该入口覆盖 SDK 最后启动、猎人笔记与暂停菜单中的手册显示和返回，以及各武器的任务预读与粒子资源。手册文字随武器 NRO 更新。

内容插件可通过 [StyleRulesV1](../../crates/mhgu-sdk/src/style/rules.rs) 登记格挡半角、公共动作结束条件、弹刀与镜头回调。注册器在 GU 近战解算后按猎人、动作槽和刀身的打击序号匹配 `Attack::DEFLECTABLE` 标记的命中，并用 GU 的斩味修正、心眼与相关豁免完成判定。动作控制器通过 `mhgu::BladeDeflectionV1` 取得结果。
