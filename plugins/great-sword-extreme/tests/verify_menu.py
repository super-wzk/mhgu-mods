"""Run the packaged equipment-box hooks with GU's actual cursor and controller."""

import struct

from unicorn.arm_const import UC_ARM_REG_SP

from verify_style import CONFIG, MENU_OPEN, MENU_UPDATE, REPO, boot, hunters, load_registered_style, service, style_descriptor

MANUAL_HINT = "详见猎人笔记的武器操作指南。"
GREAT_SWORD_DETAILS = (
    "精准防御成功不耗耐力与锋利度，\r\n"
    "可短暂阻挡地面怪物的正面位移。\r\n"
    "挡住怪物攻击三次可发动辉烈剑。\r\n"
    "不动可抵抗硬直，兽魂附身强化下一击。\r\n"
    + MANUAL_HINT
)


def verify_status(g):
    manager, other, controller, ui, compact = (g.alloc(n) for n in (40032, 1500, 132, 128, 128))
    network, actor = g.alloc(0x16280), g.alloc(5064)
    local = manager + 124
    peer = manager + 7004 + 1376
    g.put(0x1885B58, manager)
    g.put(0x18843F0, network)
    g.put(manager + 28, actor)
    g.stub(0x3F8398, lambda *_: 0)
    g.stub(0x22D8B4, lambda _, i, *rest: i == 1)
    msg = g.alloc(16)
    g.cpu.mem_write(msg, b"GU text\0")
    g.stub(0x55BC4C, lambda *_: msg)
    labels = {}
    for addr in (0x56E808, 0x56EA08):
        g.stub(addr, lambda _, widget, part, p: labels.update({(widget, part): g.string(p)}))
    for addr in (0xE79C8, 0x560E90, 0x56E6FC, 0x56E530, 0x54533C, 0x54EDEC, 0x545AF8, 0x545AC0):
        g.stub(addr, lambda *_: 0)
    for addr in (0x4D0720, 0x62E258, 0x62E910, 0x56E528, 0x545BCC, 0x4CF2D0,
                 0x630268, 0x630DC4, 0x631430, 0x6314C8):
        g.stub(addr, lambda *_: 0)
    for offset in (104, 108, 120, 124, 128):
        g.put(controller + offset, g.alloc(128))
    g.put(controller + 112, compact)
    g.put(controller + 116, ui)
    cursor, vt, noop = (g.alloc(64) for _ in range(3))
    g.put(ui + 80, cursor)
    g.put(cursor, vt)
    g.put(vt + 56, noop)
    g.put(vt + 60, noop)
    g.stub(noop, lambda *_: 0)

    def check(equipment, expected, *, details="GU text", player=None):
        labels.clear()
        if player is None:
            g.put(ui + 76, equipment)
            g.put(compact + 76, equipment)
        else:
            g.cpu.mem_write(controller + 80, bytes([player]))
            g.run(0x632EE8, controller)
            assert g.get(ui + 76) == g.get(compact + 76) == equipment
        g.run(0x62FB44, ui)
        g.run(0x633438, compact)
        assert labels[0x5C, 1] == labels[0x14, 1] == labels[0x69, 2] == expected
        assert labels[0x14, 2] == details

    g.cpu.mem_write(local + 1241, b"\x05")
    check(local, "极之型大剑", details=GREAT_SWORD_DETAILS, player=0)
    g.cpu.mem_write(other + 1241, b"\x05")
    check(other, "GU text")
    g.put(actor + 5060, other)
    check(other, "极之型大剑", details=GREAT_SWORD_DETAILS)
    g.put(actor + 5060, 0)

    # Capture the native broadcast/unicast packet at the transport boundary.
    for addr in (0x9FBA6C, 0x26E354, 0x4D390C, 0x26FCA4, 0x4D399C, 0xE173C, 0x26E50C,
                 0x26E580, 0x4D31E8, 0x26FC24, 0xE174C, 0x561F44):
        g.stub(addr, lambda *_: 0)
    g.stub(0x26E600, lambda out, inp, *rest: g.cpu.mem_write(out, bytes(g.cpu.mem_read(inp, 1376))))
    g.stub(0x13EFE3C, lambda out, inp, n, _: g.cpu.mem_write(out, bytes(g.cpu.mem_read(inp, n))))
    g.stub(0x26FDE8, lambda obj, ready, *_: g.put(obj + 1288, ready))
    g.stub(0x26FDDC, lambda obj, *_: g.get(obj + 1288))
    process, packet = g.alloc(64), g.alloc(1472)
    sent = []

    def capture(_, __, p, kind):
        assert kind == 16
        sent.append(bytes(g.cpu.mem_read(p, 1472)))

    g.stub(0x9FC394, capture)
    g.stub(0x9FC3D8, capture)
    g.run(0x202640, process, local, 1)
    g.run(0x202764, process, local, 0, 1)
    g.run(0x202640, process, local, 0x101)
    assert [struct.unpack_from("<I", p, 1400)[0] for p in sent] == [0x4D7000A1, 0x4D7000A0, 0x101]
    assert all(p[1249] == 5 for p in sent)

    def receive(flags, weapon=0, index=1):
        g.cpu.mem_write(packet, sent[1])
        g.put(packet + 1400, flags)
        g.cpu.mem_write(packet + 1244, bytes([weapon]))
        g.run(0x202894, process, packet, index)

    receive(0x4D7000A0)
    check(peer, "极之型大剑", details=GREAT_SWORD_DETAILS, player=1)
    register, _ = service(g)
    assert g.run(register, style_descriptor(g, 0xCAFE, 0), 0, 0) == 0
    receive(0x4D7CAFE0)
    check(peer, "Style 51966", details="", player=1)
    check(local, "极之型大剑", details=GREAT_SWORD_DETAILS, player=0)
    receive(0x4D7000A0, weapon=1)
    check(peer, "GU text", player=1)
    receive(0x4D70FFF0)
    check(peer, "GU text", player=1)
    receive(0x4D7000A0)
    g.run(0x2776D4, manager, 1)
    g.put(peer + 1288, 1)  # The slot becomes visible again before another hunter packet.
    check(peer, "GU text", player=1)
    receive(0x4D7000A0)
    g.run(0x274400, manager, 1, other)  # Native record replacement, also used for Prowler.
    check(peer, "GU text", player=1)
    receive(0x4D7000A0)
    receive(0)
    check(peer, "GU text", player=1)
    g.cpu.mem_write(local + 1236, b"\x01")
    g.run(0x202640, process, local, 1)
    assert struct.unpack_from("<I", sent[-1], 1400)[0] == 1
    check(local, "GU text", player=0)


def verify_set_styles(g):
    preview, summary, detail, my_set = (g.alloc(n) for n in (5000, 128, 128, 160))
    equipment = preview + 3052
    g.cpu.mem_write(my_set + 130, b"\x05")
    g.cpu.mem_write(equipment + 1241, b"\x05")
    msg = g.alloc(16)
    g.cpu.mem_write(msg, b"GU text\0")
    g.stub(0x55BC4C, lambda *_: msg)
    labels = {}
    g.stub(0x56EA08, lambda _, widget, part, p: labels.update({(widget, part): g.string(p)}))
    def formatted(_, widget, part, fmt):
        assert fmt == msg
        stack = g.cpu.reg_read(UC_ARM_REG_SP)
        assert g.get(stack) == msg
        labels[widget, part] = g.string(g.get(stack + 4))

    g.stub(0x56FD7C, formatted)
    for addr in (0x550354, g.originals[0x2778AC], 0x4CF2D0, 0x56E530, 0xE79C8):
        g.stub(addr, lambda *_: 0)
    g.stub(0x146408, lambda *_: my_set)
    g.stub(0x146BA0, lambda _, __, slot, _unused: slot == 0)
    g.stub(0x65CBE8, lambda _, panel, *rest: {21: detail, 22: summary}.get(panel, 0))
    g.stub(0x65CC0C, lambda *_: 0)
    g.stub(0x72D0F8, lambda *_: 0)
    g.run(0x72D3D8, preview, 0)
    g.run(0x58A434, summary)
    assert labels[0, 3] == "极之型大剑"
    g.put(preview + 840, 1)
    g.run(0x72D3D8, preview, 0)
    assert labels[2, 3] == "极之型大剑"
    g.put(preview + 840, 0)
    g.run(0x72D3D8, preview, 1)
    g.run(0x58A434, summary)
    assert labels[0, 3] == "GU text"
    g.cpu.mem_write(equipment + 1236, b"\x01")
    g.run(0x5D96F8, detail, equipment)
    assert labels[2, 3] == "GU text"


def verify():
    elf = (REPO / "mhgu/main.elf").read_bytes()
    g = boot(elf, b"[Weapons]\n0 = native\n11 = 11\n")
    _, eq, _ = hunters(g)
    network = g.alloc(0x16280)
    g.put(0x18843F0, network)
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
    assert g.cpu.mem_read(network + 0x1625A, 1) == b"\x01"
    assert g.run(selected, 0) == 10 and g.files[CONFIG] == b"[Weapons]\n0 = 10\n11 = 11\n"
    assert g.cpu.mem_read(eq + 1241, 1) == b"\x05"  # custom selection preserves Valor
    g.cpu.mem_write(eq + 576, struct.pack("<3H", 4, 5, 0))
    g.run(0x557D90, ui + 12, 2)
    assert press(0x2000) == 6  # GU opens the third slot's art list
    g.cpu.mem_write(eq + 576, bytes(6))
    open_box()
    assert row() == 6 and marks[panel, 0x4E, 1] == 1 and not marks[panel, 0x46, 1]
    assert (
        press(0x4000) == 0 and g.run(selected, 0) == 10
    )  # leaving the page is not selecting Guild
    open_box()
    press(0x40)  # seventh wraps to Guild
    assert row() == 0 and press(0x2000) == 2
    assert press(0x2000) == 3 and press(0) == 4
    assert g.run(selected, 0) == 0 and g.cpu.mem_read(eq + 1241, 1) == b"\0"
    assert g.files[CONFIG] == b"[Weapons]\n0 = native\n11 = 11\n"
    # GU accepts reselecting its currently equipped native style without opening a dialog.
    open_box()
    press(0x10)  # up from Guild wraps to the custom row
    assert row() == 6 and press(0x2000) == 2 and press(0x2000) == 5
    saved = g.files[CONFIG]
    restarted = boot(elf, saved)
    assert restarted.run(service(restarted)[1], 0) == 10
    verify_status(restarted)
    verify_set_styles(restarted)
    open_box()
    press(0x40)
    g.cpu.mem_write(network + 0x1625A, b"\0")
    assert row() == 0 and press(0x2000) == 5
    assert g.cpu.mem_read(network + 0x1625A, 1) == b"\x01"
    assert g.run(selected, 0) == 0 and g.files[CONFIG] == b"[Weapons]\n0 = native\n11 = 11\n"
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
        (8, "switch-axe", "斩斧F", ("回满光剑槽", "能量累积回复斩击槽")),
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
        "GU equipment box, local and peer status, native equipment packets, and My Set style previews passed"
    )


if __name__ == "__main__":
    verify()
