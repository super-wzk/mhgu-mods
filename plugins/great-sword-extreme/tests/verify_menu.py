"""Run the packaged equipment-box hooks with GU's actual cursor and controller."""

import struct

from verify_style import CONFIG, MENU_OPEN, MENU_UPDATE, REPO, STATUS_STYLE, STATUS_STYLE_SMALL, boot, hunters, load_registered_style, service, style_descriptor

MANUAL_HINT = "详见猎人笔记的武器操作指南。"
GREAT_SWORD_DETAILS = (
    "精准防御成功不耗耐力与锋利度，\r\n"
    "并可短暂挡住怪物的正面推进。\r\n"
    "挡住怪物攻击三次可发动辉烈剑。\r\n"
    "不动可抵抗硬直，兽魂附身强化下一击。\r\n"
    + MANUAL_HINT
)


def verify_status(g):
    manager, other, ui = (g.alloc(n) for n in (40032, 1500, 128))
    local = manager + 124
    g.put(0x1885B58, manager)
    msg = g.alloc(16)
    g.cpu.mem_write(msg, b"GU text\0")
    g.stub(0x55BC4C, lambda *_: msg)
    labels = {}
    for addr in (0x56E808, 0x56EA08):
        g.stub(addr, lambda _, widget, part, p: labels.update({(widget, part): g.string(p)}))
    for addr in (0xE79C8, 0x560E90, 0x56E6FC, 0x56E530, 0x54533C, 0x54EDEC, 0x545AF8, 0x545AC0):
        g.stub(addr, lambda *_: 0)
    cursor, vt, noop = (g.alloc(64) for _ in range(3))
    g.put(ui + 80, cursor)
    g.put(cursor, vt)
    g.put(vt + 56, noop)
    g.put(vt + 60, noop)
    g.stub(noop, lambda *_: 0)
    for equipment, weapon, expected in ((local, 0, "极之型大剑"), (other, 0, "GU text"), (local, 1, "GU text")):
        labels.clear()
        g.cpu.mem_write(equipment + 1236, bytes([weapon]))
        g.cpu.mem_write(equipment + 1241, b"\x05")
        g.run(g.hooks[STATUS_STYLE], ui, equipment, 1)
        g.put(ui + 76, equipment)
        g.run(g.hooks[STATUS_STYLE_SMALL], ui)
        assert labels[0x5C, 1] == labels[0x14, 1] == labels[0x69, 2] == expected
        assert labels[0x14, 2] == (GREAT_SWORD_DETAILS if expected == "极之型大剑" else "GU text")


def verify():
    elf = (REPO / "mhgu/main.elf").read_bytes()
    g = boot(elf, b"[Weapons]\n0 = 0\n11 = 2\n")
    _, eq, _ = hunters(g)
    ui, panel, info, widgets = [g.alloc(256) for _ in range(4)]
    pad = g.alloc(28000)
    g.put(0x188C670, pad)
    noop = g.alloc(4)
    g.stub(noop, lambda *_: 0)
    vt = g.alloc(128)
    for i in (24, 28, 32, 96):
        g.put(vt + i, noop)
    for p in (widgets, info):
        g.put(p, vt)
    for offset in (12, 48, 84):
        g.run(0x54ED74, ui + offset)
    g.put(panel + 76, g.alloc(160))
    g.put(panel + 88, widgets)
    g.put(widgets + 92, 8)
    g.put(widgets + 112, 0x16398B4)
    g.put(widgets + 120, 2)
    labels, marks, visible = {}, {}, {}

    def label(obj, widget, part, p):
        labels[obj, widget, part] = g.string(p)

    for addr in (0x56EA08, 0x56E808):
        g.stub(addr, label)
    g.stub(0x54533C, lambda _, w, on, _2: visible.update({w: on}))
    g.stub(0x56E530, lambda p, w, part, on: marks.update({(p, w, part): on}))
    # GUI rendering, message catalog, sound, networking, art inventory and actor rebuild are boundaries.
    # The box init, native cursor navigation, yes/no dialog and stock equipment writes run unchanged.
    msg = g.alloc(16)
    g.cpu.mem_write(msg, b"GU text\0")
    for addr in (0x55BC4C, 0x55BC7C):
        g.stub(addr, lambda *_: msg)
    for addr in (
        0x54AAE4,
        0x56DE60,
        0x56DFAC,
        0x6398F4,
        0x56E7B8,
        0x56E528,
        0x570468,
        0x638D34,
        0x638390,
        0x639130,
        0x524040,
        0x4EEB78,
        0x4EEC94,
        0x638730,
        0x638424,
        0x56E538,
        0x56E5A8,
        0x549FB4,
        0x227BA8,
        g.get(0x1749A88 + 352),
    ):
        g.stub(addr, lambda *_: 0)

    def open_box():
        g.run(g.hooks[MENU_OPEN], ui, panel, info, 1)

    def press(mask):
        g.put(pad + 27644, mask)
        g.run(g.hooks[MENU_UPDATE], ui)
        return g.get(ui + 128)

    def row():
        return g.run(0x557D78, ui + 12)

    register, selected = service(g)
    open_box()
    assert (
        row() == 4 and g.get(widgets + 100) == 7 and visible[0x4E] and not visible[0x4F]
    )
    assert labels[widgets, 0x4E, 2] == "极之型大剑" and marks[panel, 0x46, 1] == 1
    for _ in range(2):
        press(0x40)
    assert row() == 6 and labels[info, 2, 2] == "极之型大剑"
    assert labels[info, 3, 2] == GREAT_SWORD_DETAILS
    assert press(0x2000) == 2 and g.run(selected, 0) == 0
    press(0x20)  # highlight No
    assert press(0x2000) == 1 and row() == 6 and g.run(selected, 0) == 0
    assert press(0x2000) == 2 and press(0x2000) == 5
    assert g.run(selected, 0) == 1 and g.files[CONFIG] == b"[Weapons]\n0 = 1\n11 = 2\n"
    assert g.cpu.mem_read(eq + 1241, 1) == b"\x05"  # custom selection preserves Valor
    g.cpu.mem_write(eq + 576, struct.pack("<3H", 4, 5, 0))
    g.run(0x557D90, ui + 12, 2)
    assert press(0x2000) == 6  # GU opens the third slot's art list
    g.cpu.mem_write(eq + 576, bytes(6))
    open_box()
    assert row() == 6 and marks[panel, 0x4E, 1] == 1 and not marks[panel, 0x46, 1]
    assert (
        press(0x4000) == 0 and g.run(selected, 0) == 1
    )  # leaving the page is not selecting Guild
    open_box()
    press(0x40)  # seventh wraps to Guild
    assert row() == 0 and press(0x2000) == 2
    assert press(0x2000) == 3 and press(0) == 4
    assert g.run(selected, 0) == 0 and g.cpu.mem_read(eq + 1241, 1) == b"\0"
    assert g.files[CONFIG] == b"[Weapons]\n11 = 2\n"
    # GU accepts reselecting its currently equipped native style without opening a dialog.
    open_box()
    press(0x10)  # up from Guild wraps to the custom row
    assert row() == 6 and press(0x2000) == 2 and press(0x2000) == 5
    saved = g.files[CONFIG]
    restarted = boot(elf, saved)
    assert restarted.run(service(restarted)[1], 0) == 1
    verify_status(restarted)
    open_box()
    press(0x40)
    assert row() == 0 and press(0x2000) == 5
    assert g.run(selected, 0) == 0 and g.files[CONFIG] == b"[Weapons]\n11 = 2\n"
    # More than two extra styles: the next native page uses the same eight widgets.
    for id_ in (40, 41):
        assert g.run(register, style_descriptor(g, id_, 0), 0, 0) == 0
    open_box()
    press(0x20)
    assert (
        row() == 8
        and g.get(widgets + 100) == 1
        and labels[widgets, 7, 2] == "极之型大剑"
    )
    press(0x80)
    assert row() == 0 and labels[panel, 7, 2] == "GU text" and g.get(widgets + 100) == 8
    # Other weapon: box keeps its native size, navigation and descriptions.
    g.cpu.mem_write(eq + 1236, b"\x01")
    open_box()
    assert g.half(ui + 32) == 6 and g.get(widgets + 100) == 6

    styles = (
        (11, "dual-blades", "双剑", ("回避斩计入刃鳞", "各回复1点体力")),
        (7, "long-sword", "太刀", ("425点斩槽", "不消耗练气")),
        (8, "switch-axe", "斩斧 F", ("回满光剑槽", "能量累积回复斩击槽")),
        (1, "sword-shield", "单手剑", ("各发一道真空波", "叠加16", "造成50%属性伤害与异常积蓄")),
        (9, "gunlance", "铳枪", ("近战不耗斩味", "蓝槽全满时可再次点火")),
    )
    for index, (weapon, name, label, effects) in enumerate(styles, start=1):
        folder = name + "-extreme"
        nro = f"{folder.replace('-', '_')}.nro"
        load_registered_style(
            g, REPO / "plugins" / folder / "dist" / folder / "romfs/nativeNX/plugins" / nro,
            base=0x9200000 + index * 0x200000,
        )
        g.cpu.mem_write(eq + 1236, bytes([weapon]))
        labels.clear()
        open_box()
        if row() != 6:
            g.run(0x557D90, ui + 12, 5)
            press(0x40)
        assert row() == 6 and labels[info, 2, 2] == f"极之型{label}"
        details = labels[info, 3, 2]
        lines = details.split("\r\n")
        assert len(lines) == 5 and lines[-1] == MANUAL_HINT
        assert all(effect in details for effect in effects)

    print(
        "GU equipment box: six style descriptions, third Hunter Art slot, native reselect, paging and weapon filtering passed"
    )


if __name__ == "__main__":
    verify()
