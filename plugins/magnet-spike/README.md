# 极之型磁斩锤

试制磁斩锤是用于验证 MHGU 1.4 装备注册、制造和购买的测试装备。武器类型与极之型风格的编号均为 16，具体装备编号为 `0x1001`。依赖同次构建的 [MHGU SDK 运行时](../mhgu-sdk-runtime/README.md)。

- 武器商店的购买武器或工房的武器生产，在原版类别之后翻到下一页，或从最后一个原版武器类别向下选择「磁斩锤」，再选择「试制磁斩锤」。购买价 **100z**，制造费 **0z**，材料为 **回复药 ×1**（GU 道具 9）。
- 道具箱首页打开「扩展装备箱」，使用 GU 的「更换装备」界面翻页和确认装备。买入和制造分别增加一个实例。交易成功后会播放 GU 原版音效，并可在原版确认框直接装备试制磁斩锤。原版「切换风格」页面显示该类型的极之型。按 A 确认可进入原版狩技页，从通用狩技中装备最多三个。
- 自己的公会卡片会在「武器使用情况」图表的原版栏目后显示磁斩锤及其独立使用次数。猎人生活日记会在新产生的相应任务记录中标注磁斩锤。
- Lv.1 使用验证用数值，攻击 100、会心 0%、无属性、无孔，斩味表 10、斩味等级 0。当前版本提供单级装备和外观，极之型招式及专用动作特效待移植。

## 模型和图标来源

MHF-Z 的 `mhfdat.bin` 近战装备记录 16881 指向模型 6451，类别为 13。网格、骨骼和原色贴图来自 `dat/extend/wk000.abn` 的 `wk451.bin`。转换器使用 GU 材质模板生成 MRL，连同 MOD 和 TEX 写入 `arc/frontier/magnet_spike.arc`。GU 注册资源名为 `weapon\frontier\magnet_spike\wk451`。

打包时从本地 MHF-Z 的 `dat/mhf.bin` 读取第 54 号 PNG 条目（从零计数），原样取出左上角坐标为 `(192, 896)` 的 64×64 磁斩锤图标。生成的 `GUI/frontier_magnet_spike/icon.tex` 和 NRO、模型 ARC 一起安装。

## 构建与验证

从仓库根目录运行以下命令。

```sh
python3 plugins/mhgu-sdk-runtime/package.py
python3 plugins/magnet-spike/package.py "$MHFZ_DAT" "$MHGU_ROMFS"
python3 plugins/magnet-spike/tests/verify_magnet_spike.py
python3 tools/package_weapon_styles.py --include-magnet-spike
python3 tools/tests/verify_packed_release.py --include-magnet-spike
```

成品回放覆盖两种插件加载顺序，执行 GU 商店和工房的原版入口与交易控制器，并核对成功音效指令、立即装备、预览初始化、预览方向输入、装备数值和模型绑定。游戏内的实际音效和 3D 画面需要实机确认。安装时从整合包取出 `mhgu-sdk/` 和 `magnet-spike/`，保留各自的目录结构。
