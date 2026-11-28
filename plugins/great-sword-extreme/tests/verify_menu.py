"""Run the packaged equipment-box hooks with GU's actual cursor and controller."""

import json
import math
import struct

from unicorn.arm_const import UC_ARM_REG_SP, UC_ARM_REG_S0

from gu_replay import Game
from verify_style import MANAGER, SAVE, api_for, character_save, save_character, select_character, MENU_OPEN, MENU_UPDATE, REPO, boot, hunters, load_registered_style, service, style_descriptor
from verify_style import CHANGE_ACTION, COMMAND_BIND, DISPATCH

MANUAL_HINT = "详见猎人笔记的武器操作指南。"
GREAT_SWORD_DETAILS = (
    "精准防御成功不耗耐力与锋利度，\r\n"
    "可短暂阻挡地面怪物的正面位移。\r\n"
    "挡住怪物攻击三次可发动辉烈剑。\r\n"
    "不动可抵抗硬直，兽魂附身强化下一击。\r\n"
    + MANUAL_HINT
)


def register_weapon(g, *, ident=16, ranged=False, limit=3, style=42, art_slots=0,
                    weapon_name="Custom weapon", icon=13, controller=0, request=0, actions=0, max_slots=0):
    api = api_for(g, b"mhgu::WeaponRegistryV7", b"WPN7")
    register = g.get(api)
    descriptor, name, type_name = g.alloc(8), g.alloc(16), g.alloc(48)
    g.cpu.mem_write(type_name, weapon_name.encode() + b"\0")
    g.cpu.mem_write(descriptor, struct.pack("<H?B", ident, ranged, icon))
    g.cpu.mem_write(name, b"Custom weapon\0")
    style = style_descriptor(g, style, 0x10000 | ident, art_slots=art_slots)
    g.put(style + 20, controller)
    g.put(style + 24, request)
    g.put(style + 28, actions)
    assert g.run(register, descriptor, style, type_name, name, 0) == 0
    assert g.run(register, descriptor, style, type_name, name, 0) == 0
    assert g.run(register, descriptor, style_descriptor(g, 43, 0x10000 | ident), type_name, name, 0) == 2
    other = g.alloc(8)
    g.cpu.mem_write(other, struct.pack("<H?B", ident, ranged, icon))
    assert g.run(register, other, style_descriptor(g, 43, 0x10000 | ident), type_name, name, 0) == 2
    model, levels = g.alloc(12), g.alloc(12 * (limit + 1))
    g.cpu.mem_write(levels + 12 * limit + 8, bytes([max_slots]))
    g.cpu.mem_write(model, struct.pack("<HH5BxH", ident, ident, 1, 0, 0, 0, 0, 0))
    assert g.run(g.get(api + 20), model, levels, limit + 1, name, name, 0, 0) == 0
    return tuple(g.get(api + i * 4) for i in range(1, 5))


def setup_decorations(g):
    # Two identical SDK weapons, the highlighted copy is Lv.4 with three slots.
    box, pouch = (g.alloc(n) for n in (30000,256))
    tables, gems, records = (g.alloc(n) for n in (56,112,24))
    g.put(0x1858438, box)
    g.put(0x18584B8, pouch)
    g.put(0x18583A8,tables)
    items, item_rows = g.alloc(112),g.alloc(56*2641)
    g.put(tables+28,items)
    g.put(items+100,2641)
    g.put(items+108,item_rows)
    for id in (2638,2639,2640):
        g.put(item_rows+56*id+8,id)
        g.cpu.mem_write(item_rows+56*id+14,bytes([99]))
    g.stub(0x1943a4,lambda *_:0)
    g.put(tables+44,gems)
    g.put(gems+100,3)
    g.put(gems+108,records)
    g.cpu.mem_write(records+4,bytes([1]))
    g.cpu.mem_write(records+16,bytes([2]))
    g.cpu.mem_write(records+28,bytes([3]))
    g.put(pouch+26,(1<<16)|2639)
    g.put(box+26,(1<<16)|2638)
    for addr in (0x1943d4,0x3f8634,0x3f8678):
        g.stub(addr,lambda *_:0)
    # The replay has no live skill allocator or graphical comparison panels.
    for address in (0x26e354,0x26e43c,0x26e600,0x26e580,0x26fc24,0x26e50c,0x552c8c,0x72cb4c):
        g.stub(address,lambda ptr,*args:ptr)
    g.stub(0x13c208,lambda *args:0)
    # Stock controller owns jewel-list construction and navigation. Supply the
    # selected item table and allow the real debit/credit and socket writes.
    g.stub(0xb84114,lambda *args:0)
    g.stub(0x7af3a0,lambda n,*args:g.alloc(n))
    g.stub(0x7af3e4,lambda *args:0)
    g.stub(0x13efe30,lambda d,s,n,*args:g.cpu.mem_write(d,bytes(g.cpu.mem_read(s,n))))
    g.stub(0x550110,lambda *args:2)
    g.stub(0x1951fc,lambda *_:0)
    g.stub(0x195374,lambda *_:2)
    g.stub(0x195400,lambda _,i,*args: (2638,2639)[i])
    return box,pouch


def verify_expansion_controls(g):
    manager, actor, scratch = (g.alloc(size) for size in (40032, 14000, 1376))
    equipment = manager + 124
    g.put(0x1885B58, manager)
    g.put(manager + 28, actor)
    g.put(actor, 0x1749A88)
    g.put(actor + 5060, equipment)
    g.put(actor + 5068, 1)
    g.stub(0x3F8398, lambda *_: 0)
    g.stub(0x65FA04, lambda *_: 0x800)

    received, controller, request = [], g.alloc(4), g.alloc(4)
    g.stub(controller, lambda _actor, inp, *_:
           received.append(g.get(inp + 16) if inp else None))
    # Even an explicit native opt-in must not reach the fallback weapon.
    g.stub(request, lambda *_: 0xfffffffe)
    register_weapon(g, controller=controller, request=request, actions=1, art_slots=3)
    saved = json.loads(character_save({}))
    saved["data"]["inventory"] = {
        "instances": {"1": {"equipment": 16, "upgrade": 0}}, "equipped": 1, "sets": {},
    }
    g.files[SAVE] = json.dumps(saved).encode()
    select_character(g, 0)
    g.put(actor + 5156, 4)
    native_tick = []
    g.stub(g.originals[DISPATCH], lambda _actor, *_: native_tick.append(True))
    g.run(DISPATCH, actor)
    assert received == [4] and not native_tick, (received, native_tick)

    changed = []
    g.stub(g.originals[CHANGE_ACTION], lambda _, group, slot, flags:
           changed.append((group, slot, flags)))
    g.run(CHANGE_ACTION, actor, 16, 3, 0x80)
    assert not changed
    g.run(CHANGE_ACTION, actor, 16, 0x8000, 0)
    g.run(CHANGE_ACTION, actor, 16, 241, 0x40)
    g.run(CHANGE_ACTION, actor, 1, 6, 0)
    assert changed == [(16, 0x8000, 0), (16, 241, 0x40), (1, 6, 0)]

    rows = [(16, 3, 8), (0x8010, 12, 8), (1, 6, 6), (16, 803, 6), (0x8010, 241, 0x13)]
    table, pointers = g.alloc(24), g.alloc(4 * len(rows))
    g.put(table + 8, len(rows))
    g.put(table + 12, len(rows))
    g.put(table + 20, pointers)
    for i, (group, slot, condition) in enumerate(rows):
        entry, resource = g.alloc(24), g.alloc(128)
        g.put(entry + 8, resource)
        g.put(pointers + 4 * i, entry)
        g.cpu.mem_write(resource + 100, struct.pack("<4H", group, slot, 0, condition))
    g.stub(g.originals[COMMAND_BIND], lambda *_: g.put(actor + 9384, table))
    g.run(COMMAND_BIND, actor, 1000, 0)
    filtered = g.get(actor + 9384)
    entries = g.get(filtered + 20)
    assert [g.get(entries + 4 * i) for i in range(g.get(filtered + 8))] == [
        g.get(pointers + 4 * i) for i in range(2, 5)
    ]

    # A fixed-loadout scratch record uses GU's original controls.
    g.put(actor + 5060, scratch)
    g.run(DISPATCH, actor)
    g.run(CHANGE_ACTION, actor, 16, 3, 0x80)
    g.run(COMMAND_BIND, actor, 1000, 0)
    assert received == [4, None] and native_tick == [True]
    assert changed[-1] == (16, 3, 0x80) and g.get(actor + 9384) == table


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
    for addr in (0x4D0720, g.originals[0x62E258], 0x62E910, 0x56E528, 0x545BCC, 0x4CF2D0,
                 g.originals[0x630268], 0x630DC4, 0x631430, 0x6314C8):
        g.stub(addr, lambda *_: 0)
    icons, numbers = {}, {}
    g.stub(g.originals[0x56E6A8], lambda _, group, part, atlas:
           icons.update({(group, part): (atlas, g.get(g.cpu.reg_read(UC_ARM_REG_SP)))}))
    g.stub(0x56FE88, lambda _, group, part, value, *_rest:
           numbers.update({(group, part): value}))
    g.stub(0x560EF0, lambda *_: 0)
    g.stub(0x560E5C, lambda *_: 0)
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
    g.stub(g.originals[0x9FC3D8], capture)
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

    # The packet identifies the specific equipment while preserving the native fallback.
    equipped, grant, upgrade, instance = register_weapon(g)
    data = json.loads(character_save({0: 10}))
    data["data"]["inventory"] = {"instances": {"1": {"equipment": 16, "upgrade": 2}}, "equipped": 1, "sets": {}, "decorations":{"1":[2638,0,0]}}
    g.files[SAVE] = json.dumps(data).encode()
    select_character(g, 0)
    assert g.cpu.mem_read(local + 1236, 1) == b"\x01"
    assert g.run(equipped) == 0x10010
    check(local, "Custom weapon", details="", player=0)
    assert labels[5, 3] == "Custom weapon"
    assert labels[0x27, 2] == "Custom weapon" and numbers[0x27, 8] == 3
    assert icons[5, 1][0] == 0 and all(icons[5, part][0] == 7 for part in (4, 5, 6))
    details, scratch = g.alloc(256), g.alloc(1376)
    g.stub(g.originals[0x57F468], lambda *_: 0)
    g.run(0x57F468, details, local + 24, 0, scratch, 0, 0x8000, 0, 0)
    assert labels[17, 3] == "Custom weapon" and numbers[17, 7] == 3
    del labels[17, 3]
    g.run(0x57F468, details, local + 68, 0, scratch, 0, 0x8000, 0, 0)
    assert (17, 3) not in labels
    g.run(0x202640, process, local, 0)
    assert struct.unpack_from("<3H",sent[-1],8+34)==(2638,0,0) and g.half(local+34)==0
    assert struct.unpack_from("<I", sent[-1], 1400)[0] == 0x98040020
    assert sent[-1][8 + 1236] == 1 and g.cpu.mem_read(local + 1236, 1) == b"\x01"
    g.cpu.mem_write(packet, sent[-1])
    g.run(0x202894, process, packet, 1)
    check(peer, "Custom weapon", details="", player=1)
    assert labels[5, 3] == "Custom weapon"
    assert labels[0x27, 2] == "Custom weapon" and numbers[0x27, 8] == 3
    receive(0)
    check(peer, "GU text", player=1)
    g.files[SAVE] = character_save({0: 10, 11: 11})
    select_character(g, 0)


def verify_guild(g):
    register_weapon(g)
    manager, guild, view, result = (g.alloc(n) for n in (40032, 30000, 1656, 920))
    card, event = guild + 32, result + 760
    g.put(0x1885B58, manager)
    g.put(0x1857668, guild)
    g.put(0x188AE38, g.alloc(64))
    g.put(view + 80, card)
    g.stub(0x166A30, lambda _, viewed, mine, *_rest: viewed == mine)
    data = json.loads(character_save({0: 10}))
    data["data"]["inventory"] = {"instances": {"1": {"equipment": 16, "upgrade": 2}}, "equipped": 1, "sets": {}}
    data["data"]["guild_card"] = {"equipment": None, "usage": {"16": [99, 0, 0]}, "diary": {}}
    g.files[SAVE] = json.dumps(data).encode()
    select_character(g, 0)
    labels = {}
    for addr in (0x56E808, 0x56EA08):
        g.stub(addr, lambda _, group, part, text: labels.update({(group, part): g.string(text)}))
    for addr in (0x56E6A8, 0x56E6FC, 0x560EF0, 0x56E528):
        g.stub(addr, lambda *_: 0)
    for addr in (0x1623B4, 0x5A41A0, 0x5A7B00, 0x59F560):
        g.stub(g.originals[addr], lambda *_: 0)
    g.run(0x1623B4, manager + 124, card, manager + 124)
    g.run(0x5A41A0, view)
    assert labels[9, 2] == "Custom weapon"
    g.put(view + 80, g.alloc(6328))
    labels.clear()
    g.run(0x5A41A0, view)
    assert (9, 2) not in labels
    packet = g.alloc(17 + 6328)
    g.stub(0x13EFE30, lambda out, src, size, *_: g.cpu.mem_write(out, bytes(g.cpu.mem_read(src, size))))
    g.run(0x2221E0, packet, card)
    g.put(view + 80, packet + 17)
    g.run(0x5A41A0, view)
    assert labels[9, 2] == "Custom weapon"
    g.put(view + 80, card)

    native_record = g.alloc(44)
    g.put(native_record + 4, 7)
    g.stub(0xDF814, lambda *_: native_record)
    g.stub(0x3B9870, lambda *_: 0)
    g.stub(0x3B5B28, lambda *_: 0)
    g.stub(0x3BCF68, lambda *_: 1)
    g.stub(0x227BA8, lambda *_: 0)
    g.put(0x188B458, g.alloc(14300))
    counter = card + 2234
    g.put(counter, 9999)
    g.run(0x38EAE4, result)
    assert g.half(counter) == 9999
    g.put(event, 0x07ea0b1a)
    g.put(event + 4, 7)
    g.stub(0x13EFDD0, lambda dest, source, size, *_:
           g.cpu.mem_write(dest, bytes(g.cpu.mem_read(source, size))))
    g.run(0x162758, guild, event)
    text, row = g.alloc(16), g.alloc(12)
    g.cpu.mem_write(text, b"Quest cleared\0")
    g.put(row, text)
    g.put(view + 196, row)
    g.cpu.mem_write(view + 632, b"\1")
    g.cpu.mem_write(view + 742, b"\1")
    g.run(0x5A7B00, view)
    assert labels[0xac, 3] == "Quest cleared  Custom weapon累计100次"

    verify_guild_usage_chart(g, view, card)

    save_character(g)
    saved = json.loads(g.files[SAVE])["data"]["guild_card"]
    assert saved["equipment"]["weapon"] == {"equipment": 16, "upgrade": 2}
    assert saved["usage"] == {"16": [100, 0, 0]}
    assert list(saved["diary"].values()) == [{"equipment": 16, "milestone": 100}]
    select_character(g, 0)
    labels.clear()
    g.run(0x5A41A0, view)
    g.run(0x5A7B00, view)
    assert labels[9, 2] == "Custom weapon" and "100次" in labels[0xac, 3]


def verify_guild_usage_chart(g, view, card):
    counts, clones, groups, visible = {}, [], {}, {}
    ids = [g.get(0x1631104 + i * 4) for i in range(15)]
    # Screen order from Null_bar in GU 1.4's guildcard_down.gui.
    stock = [37, 40, 39, 41, 38, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51]
    assert set(ids) == set(stock) and ids != stock
    stock_count = 203
    extra = [stock_count, stock_count + 1]
    layout = g.alloc(680)
    stock_groups, extra_groups = g.alloc(4 * stock_count), g.alloc(8)
    g.put(layout + 248, stock_groups)
    g.put(layout + 660, extra_groups)
    g.put(layout + 664, stock_count)
    g.put(view + 36, layout)
    g.put(view + 132, 3)

    def floats(address, *values):
        g.cpu.mem_write(address, struct.pack(f"<{len(values)}f", *values))

    def pair(address):
        return struct.unpack("<2f", g.cpu.mem_read(address, 8))

    parent = g.alloc(224)
    floats(parent + 144, 90, 116)
    g.put(stock_groups + 4 * 0x24, parent)
    floats(view + 160, 326.4, 307.2)
    vt, resize = g.alloc(116), g.alloc(4)
    g.put(vt + 112, resize)
    g.stub(resize, lambda node, size, *_: floats(node + 180, *pair(size)))
    for i, group in enumerate(stock + extra):
        node, kids, picture, bar, total = (g.alloc(n) for n in (240, 16, 300, 300, 240))
        groups[group] = (node, picture, bar, total)
        if group < stock_count:
            g.put(stock_groups + 4 * group, node)
            g.put(node + 104, parent)
        floats(node + 144, -4.8 + 43.2 * i, 0 if group < stock_count else -116)
        g.put(node + 216, kids)
        g.put(node + 208, 0xffffffff)
        g.put(node + 212, 29)
        g.put(kids + 4, picture)
        g.put(kids + 8, bar)
        g.put(kids + 12, total)
        for sprite in (picture, bar):
            g.put(sprite, vt)
        if group < stock_count:
            floats(picture + 128, 0, 355.2)
            floats(bar + 128, 0, 307.2)
            floats(picture + 180, 38.4, 38.4)
            floats(bar + 180, 19.2, 0)
        else:
            floats(picture + 128, 8.0, 0)
            floats(picture + 180, 32, 40)

    root, sequence = g.alloc(152), g.alloc(4)
    g.put(groups[stock[-1]][0] + 220, root)
    g.put(root + 148, sequence)
    g.put(sequence, 1000018)

    def clone(_layout, resource, template, parent_group):
        group = extra[len(clones)]
        clones.append((resource, template, parent_group))
        node = groups[group][0]
        g.put(node + 104, g.get(stock_groups + 4 * parent_group))
        g.put(extra_groups + 4 * (group - stock_count), node)
        return group

    selected_sequences = {}
    g.stub(0xA3896C, lambda node, sequence, *_: selected_sequences.update({node: sequence}))
    g.stub(0x54501C, clone)
    g.stub(0x54533C, lambda _layout, group, show, *_: visible.update({group: show}))
    g.stub(0x56FDE0, lambda _view, group, _part, n: counts.update({group: n}))
    g.stub(g.originals[0x5A6CB4], lambda *_:
           counts.update({ids[0]: g.half(g.get(view + 80) + 2234)}))

    def check_layout(added):
        active = stock + extra[:added]
        scale = len(stock) / len(active)
        step = 43.2 * scale
        centers = []
        for group in active:
            node, picture, bar, _ = groups[group]
            gx, gy = pair(node + 144)
            px, py = pair(picture + 128)
            centers.append(gx + px)
            assert g.get(node + 104) == parent and gy == 0
            assert math.isclose(gy + py, 355.2, abs_tol=0.001)
            assert math.isclose(pair(bar + 128)[1], 307.2, abs_tol=0.001)
            assert all(math.isclose(size, 38.4 * scale, abs_tol=0.001)
                       for size in pair(picture + 180))
            assert math.isclose(pair(bar + 180)[0], 19.2 * scale, abs_tol=0.001)
        assert all(math.isclose(b - a, step, abs_tol=0.001)
                   for a, b in zip(centers, centers[1:])), centers
        assert math.isclose((centers[0] + centers[-1]) / 2, 297.6, abs_tol=0.001)
        assert all(visible[group] == 1 for group in extra[:added])
        assert all(visible[group] == 0 for group in extra[added:len(clones)])

    g.run(0x5A6CB4, view)
    assert clones == [(0xffffffff, 29, 0x24)]
    assert selected_sequences == {groups[extra[0]][0]: 1000018}
    assert counts == {ids[0]: 9999, extra[0]: 100} and g.get(view + 80) == card
    check_layout(1)

    register_weapon(g, ident=17, style=44)
    g.run(0x5A6CB4, view)
    assert clones == [(0xffffffff, 29, 0x24)] * 2 and counts[extra[1]] == 0
    assert math.isclose(pair(groups[extra[1]][3] + 128)[1], 305.2, abs_tol=0.001)
    assert pair(groups[extra[1]][2] + 180)[1] == 0
    check_layout(2)

    g.put(view + 80, g.alloc(6328))
    g.run(0x5A6CB4, view)
    check_layout(0)
    g.put(view + 80, card)


def verify_set_styles(g):
    preview, summary, detail = (g.alloc(n) for n in (5000, 128, 128))
    save = 0x7110000
    g.put(0x1856FE8, save)
    my_set = save + 254028
    stock_set = my_set + 136
    g.cpu.mem_write(my_set + 42, struct.pack("<H", 7))
    g.cpu.mem_write(my_set + 130, b"\x05")
    g.cpu.mem_write(stock_set + 42, struct.pack("<H", 8))
    g.cpu.mem_write(stock_set + 130, b"\x04")
    _, worn, _ = hunters(g)
    g.stub(0x3F8398, lambda *_: 0)
    g.stub(0xE100C, lambda _, __, *rest: g.half(worn + 4))
    g.stub(g.originals[0x146CB8], lambda _, slot, *rest: int(slot < 40))

    def apply_native(_, slot, *rest):
        record = my_set + slot * 136
        g.cpu.mem_write(worn + 4, bytes(g.cpu.mem_read(record + 42, 2)))
        g.cpu.mem_write(worn + 1241, bytes(g.cpu.mem_read(record + 130, 1)))

    g.stub(g.originals[0x146560], apply_native)
    assert g.run(g.hooks[0x146CB8], save, 0) == 1
    # An old native set has no SDK metadata. It must disable the custom style.
    g.run(g.hooks[0x146560], save, 1)
    selected = service(g)[1]
    assert g.run(selected, 0) == 0 and g.half(worn + 4) == 8
    assert g.run(g.hooks[0x146CB8], save, 1) == 1
    g.run(g.hooks[0x146560], save, 0)
    assert g.run(selected, 0) == 10 and g.half(worn + 4) == 7
    equipment = preview + 3052
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
    g.stub(0x146408, lambda _, slot, *rest: my_set + slot * 136)
    g.stub(0x146BA0, lambda _, __, slot, _unused: slot < 2)
    g.stub(0x65CBE8, lambda _, panel, *rest: {21: detail, 22: summary}.get(panel, 0))
    g.stub(0x65CC0C, lambda *_: 0)
    g.stub(0x72D0F8, lambda *_: 0)
    save_character(g)
    assert json.loads(g.files[SAVE])["data"]["equipment_sets"]["0"]["id"] == 10
    select_character(g, 1)
    assert g.run(selected, 0) == 0
    select_character(g, 0)
    assert g.run(selected, 0) == 10
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

    # Run the original swap and delete with the game's string copy stubbed.
    def copy_name(dest, src, n, _):
        raw = bytes(g.cpu.mem_read(src, n))
        g.cpu.mem_write(dest, raw.split(b"\0", 1)[0].ljust(n, b"\0"))
        return dest

    g.stub(0x13EFE3C, copy_name)
    g.run(g.hooks[0x1480B4], save, 0, 0, 1)
    g.run(g.hooks[0x146560], save, 0)
    assert g.run(selected, 0) == 0
    g.run(g.hooks[0x146560], save, 1)
    assert g.run(selected, 0) == 10

    # Overwriting a custom combination with stock clears its record.
    g.run(g.hooks[0x146560], save, 0)
    assert g.run(g.hooks[0x146CB8], save, 1) == 1
    g.run(g.hooks[0x146560], save, 1)
    assert g.run(selected, 0) == 0
    g.run(g.hooks[0x147250], save, 1)
    assert g.half(stock_set + 42) == 0xFFFF
    save_character(g, success=False)
    select_character(g, 0)
    assert g.run(selected, 0) == 10
    g.run(g.hooks[0x147250], save, 0)
    assert g.half(my_set + 42) == 0xFFFF
    # A GU edit or mismatched restored record cannot supply the old custom style.
    g.cpu.mem_write(stock_set + 42, struct.pack("<H", 9))
    g.run(g.hooks[0x146560], save, 1)
    assert g.run(selected, 0) == 0
    save_character(g)
    select_character(g, 0)
    assert g.run(selected, 0) == 0 and set(g.files) == {SAVE}
    g.stub(g.originals[0x3DFAFC], lambda mgr, *rest: (
        g.put(mgr + 60, 9), g.cpu.mem_write(mgr + 108, b"\x01")
    ) and 1)
    g.put(MANAGER + 60, 6)
    g.run(g.hooks[0x3DFAFC], MANAGER)
    select_character(g, 0)
    assert g.run(selected, 0) == 0
    assert all(json.loads(data)["data"]["equipment_sets"] == {} for data in g.files.values())

    # Custom weapon registration survives My Set independently of the stock style.
    data = json.loads(character_save({}))
    data["data"]["inventory"] = {"instances": {"1": {"equipment": 16, "upgrade": 2}}, "equipped": 1, "sets": {}}
    g.files[SAVE] = json.dumps(data).encode()
    select_character(g, 0)
    equipped = g.get(api_for(g, b"mhgu::WeaponRegistryV7", b"WPN7") + 4)
    g.cpu.mem_write(worn + 1236, b"\x01")
    g.cpu.mem_write(worn + 28, struct.pack("<HH", 8, 9))
    g.cpu.mem_write(my_set + 42, struct.pack("<H", 9))
    g.cpu.mem_write(stock_set + 42, struct.pack("<H", 8))
    assert g.run(equipped) == 0x10010
    assert g.run(g.hooks[0x146CB8], save, 0) == 1
    g.run(g.hooks[0x146560], save, 1)
    assert g.run(equipped) == 1
    g.run(g.hooks[0x146560], save, 0)
    assert g.run(equipped) == 0x10010


def verify_stock_grid(g, elf):
    # Retail callbacks also run outside the extension controller's scoped reads.
    player = g.get(0x1897F78)
    manager, save = g.alloc(40032), g.alloc(270000)
    g.put(0x1885B58, manager)
    for village, guild in ((0, 0), (1, 2), (3, 7)):
        g.put(player + 2392, village << 21)
        g.put(player + 13844, guild)
        bonus = bin(village).count("1") * 200
        pages = bin(guild).count("1")
        assert g.run(0x1418E0, save, 0) == 1000 + bonus + pages * 200
        assert g.run(0x1418E0, save, 1) == 300 + bonus + pages * 100
        assert g.run(0x1418E0, save, 2) == 0
    g.cpu.mem_write(manager + 132, struct.pack("<7H", *range(40, 47)))
    for slot in (0, *range(40, 48)):
        assert g.run(0x144590, save, slot) == int(40 <= slot <= 46)
    # Recomp cannot resolve a guest indirect call into the middle of GU code.
    def reject_interior(address):
        def reject(*_):
            raise AssertionError(f"SDK jumped into a GU function at {address:#x}")
        g.stub(address, reject)

    for address in (0x148348, 0x148398, 0x144274):
        reject_interior(address)

    stock = Game(elf)
    stock_save = stock.alloc(270000)
    for game, data in ((g, save), (stock, stock_save)):
        game.cpu.mem_write(data + 254070, struct.pack("<7H", *range(20, 27)))
        game.cpu.mem_write(data + 254070 + 136 * 39, struct.pack("<8H", 50, 51, 52, 53, 54, 55, 23, 99))
        game.cpu.mem_write(data + 0x3F5B6, struct.pack("<3H", 50, 51, 52))
        game.cpu.mem_write(data + 0x3F5B6 + 68 * 23, struct.pack("<4H", 53, 0xffff, 50, 99))
    # The BL in the ordinary equipment upgrade check uses the hunter table.
    assert g.run(0x70AC90, save, 0, 23, until=0x70AC94) == 39
    for kind in (0, 1, 2):
        for slot in (0, *range(20, 27), *range(50, 56), 99, 0xffff, 0x10032):
            assert g.run(0x148340, save, kind, slot) == stock.run(0x148340, stock_save, kind, slot)

    stock_manager = stock.alloc(40032)
    stock.put(0x1885B58, stock_manager)
    stock.cpu.mem_write(stock_manager + 132, struct.pack("<7H", *range(40, 47)))
    for game, data in ((g, save), (stock, stock_save)):
        game.put(0x1856FE8, data)
        player = game.get(0x1897F78)
        game.put(player + 2392, 0)
        game.put(player + 13844, 0)
        for first in (28, 88028):
            game.cpu.mem_write(data + first + 44 * 22 + 4, struct.pack("<HHH", 7, 0, 0))
            game.cpu.mem_write(data + first + 44 * 23 + 4, struct.pack("<HHH", 7, 0, 1))
        # Supply the Prowler roster match, leaving the GU eligibility checks intact.
        game.stub(0x260440, lambda _roster, i, *_: i)
        game.stub(0x1462D0, lambda i, slot, *_: int(i == 83 and slot == 41))
    for kind, capacity in ((0, 1000), (1, 300), (2, 0)):
        for slot in (22, 23, 40, 41, 47, capacity - 1 if capacity else 0, capacity, 0xffff):
            assert g.run(0x14426C, save, kind, slot) == stock.run(0x14426C, stock_save, kind, slot)
    for kind, entry in enumerate((0x142268, 0x149374)):
        calls = []
        g.stub(entry, lambda arg, *_: calls.append(arg) or 13)
        assert g.run(0x14935C, save, kind) == 13 and calls == [save]
    assert g.run(0x14935C, save, 2) == save


def verify():
    elf = (REPO / "mhgu/main.elf").read_bytes()
    verify_stock_grid(boot(elf, character_save({})), elf)
    config = character_save({11: 11})
    g = boot(elf, config)
    actor, eq, _ = hunters(g)
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
    assert g.run(selected, 0) == 10 and g.files[SAVE] == config
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
    assert g.files[SAVE] == config
    # GU accepts reselecting its currently equipped native style without opening a dialog.
    open_box()
    press(0x10)  # up from Guild wraps to the custom row
    assert row() == 6 and press(0x2000) == 2 and press(0x2000) == 5
    save_character(g)
    saved = json.loads(g.files[SAVE])
    assert saved["data"]["weapons"][0] == 10
    last_saved = g.files[SAVE]
    restarted = boot(elf, character_save({0: 10, 11: 11}))
    assert restarted.run(service(restarted)[1], 0) == 10
    verify_status(restarted)
    verify_guild(boot(elf, character_save({0: 10})))
    verify_expansion_controls(boot(elf, character_save({})))
    verify_set_styles(restarted)
    open_box()
    press(0x40)
    g.cpu.mem_write(network + 0x1625A, b"\0")
    assert row() == 0 and press(0x2000) == 5
    assert g.cpu.mem_read(network + 0x1625A, 1) == b"\x01"
    assert g.run(selected, 0) == 0 and g.files[SAVE] == last_saved
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

    equipped, grant, upgrade, instance = register_weapon(g, art_slots=3, max_slots=3)
    g.cpu.mem_write(eq + 1236, b"\0")
    g.cpu.mem_write(eq + 28, struct.pack("<HH", 7, 8))
    native = bytes(g.cpu.mem_read(eq, 1376))
    open_box()
    assert g.half(ui + 32) == 9  # Only six retail and three regular styles.
    assert press(0x8000) == 1 and g.half(ui + 32) == 9  # Y stays with GU.
    assert press(0x1000) == 1 and g.half(ui + 32) == 9  # X leaves the native style list alone.

    box, home = g.alloc(4500), g.alloc(200)
    g.put(box + 52, g.alloc(112))
    native_save = g.alloc(0x3A000)
    g.put(0x1856FE8, native_save)
    g.put(native_save + 32, 7 | (1 << 16))
    g.stub(0x276278, lambda *_: actor)
    g.stub(g.get(g.get(actor) + 136), lambda *_: 1)
    g.stub(0x227BA8, lambda *_: 0)
    g.stub(0x65CC0C, lambda *_: 0)
    original_save = bytes(g.cpu.mem_read(native_save, 0x3A000))
    g.put(box + 64, 1)
    g.put(home + 112, 0x16328F0)
    updates = []
    home_vt, update = g.alloc(112), g.alloc(4)
    g.put(home, home_vt)
    g.put(home_vt + 104, update)
    g.put(home_vt + 108, noop)
    g.stub(update, lambda *_: updates.append(True))
    g.put(home + 84, box + 300)
    g.put(home + 92, 9)
    g.cpu.mem_write(home + 164, bytes([1] * 9))
    g.run(0x54ED74, box + 300)
    g.run(0x54EDEC, box + 300, 9, 1, 0)
    grid = g.alloc(160)
    g.put(grid+92,g.alloc(160))
    g.put(grid+96,g.alloc(160))
    g.run(0x553030, box + 372)
    for offset in (336, 408, 444, 516, 552):
        g.run(0x54ED74, box + offset)
    g.cpu.hook_del(g.traps[0x65CB3C])
    g.stub(0x65CB3C, lambda owner, index, *_: {0: home, 2: grid, 16: panel, 17: info}.get(index, 0)
           if owner == box else 0)
    g.stub(0x714E34, lambda *_: 0)
    g.stub(0x65CBA0, lambda _,idx,*args: home if idx == 0 else 0)
    cursor_positions = []
    g.put(home + 88, struct.unpack("<I", struct.pack("<f", 32.0))[0])
    def float_arg():
        return struct.unpack("<f", struct.pack("<I", g.cpu.reg_read(UC_ARM_REG_S0)))[0]
    g.stub(0x545AC0, lambda *_: cursor_positions.append(float_arg()))
    colors = {}
    g.stub(0x56E6FC, lambda obj, widget, part, color: colors.update({(obj, widget, part): color}))
    g.stub(0x550354, lambda enabled, marked, *_: enabled | (marked << 1))
    def localized(obj, widget, part, _):
        assert obj in (home, grid)
        sp = g.cpu.reg_read(UC_ARM_REG_SP)
        labels[obj, widget, part] = g.get(sp + 4)
    g.stub(0x56FC58, localized)
    # Run the GU root controller, its cursor, child initialization and close.
    # Only the external panels, sounds and lobby helpers are substituted here.
    for addr in (0x1B844, 0x72D75C, 0x65EFCC, 0x65CBC4, 0x65E794,
                 0x65E874, 0x65EA84, 0x718934, 0x718A0C, 0x259058, 0x259124,
                 0x54A9FC, 0x2772C4, 0x714DA0, 0x714BE4):
        g.stub(addr, lambda *_: 0)
    extra_stock_row = [1]
    g.stub(0x191688, lambda *_: extra_stock_row[0])
    inventory_ui = box + 2308
    for offset in (12, 48, 84):
        g.run(0x54ED74, inventory_ui + offset)

    def press_home(mask, repeat=0):
        g.put(pad + 27644, mask)
        g.put(pad + 27700, repeat)
        g.run(0x713DB8, box)
        return g.get(box + 64)

    prompts = []
    def inventory_prompt(flags, mode, group, message):
        choice = g.get(g.cpu.reg_read(UC_ARM_REG_SP))
        assert mode == 1 and group in (14, 22)
        prompts.append((flags, message, choice))
    g.stub(0x65E5BC, inventory_prompt)

    busy = [0]
    g.stub(0x65D9E8, lambda *_: busy[0])
    assert press_home(0x1000) == press_home(0x8000) == 1
    g.run(g.hooks[0x5C1FA4], home)
    assert labels[home, 10, 1] == 5  # GU's equipment-management row.
    assert labels[home, 11, 1] == "扩展装备箱", {k: v for k, v in labels.items() if k[0] == home}
    assert labels[home, 13, 1] == 7  # Equipment sets keep their native command.
    # +92 is nine real widgets even when the unlock leaves eight commands.
    g.put(home + 92, 9)
    extra_stock_row[0] = 0
    g.put(box + 64, 0)
    assert press_home(0) == 1
    g.run(g.hooks[0x5C1FA4], home)
    assert labels[home, 31, 1] == 6  # The ninth widget shows Change Style.
    for _ in range(6):
        press_home(0x40)
    assert press_home(0x2000) == 3
    assert g.get(box+88) == 1
    assert press_home(0x4000) == 1
    g.put(home + 92, 9)
    extra_stock_row[0] = 1
    g.put(box + 112, 0)
    g.put(box + 64, 0)
    assert press_home(0) == 1
    assert press_home(0x10) == 1  # Wrap to row 10 in a nine-widget window.
    assert g.get(box + 112) == 8
    g.run(g.hooks[0x5C1FA4], home)
    assert labels[home, 31, 1] == 225 and colors[home, 31, 1] == 3
    assert press_home(0x2000) == 3 and g.get(box + 116) == 8
    g.put(box + 64, 1)
    assert press_home(0x40) == 1 and g.get(box + 112) == 0
    for _ in range(7):
        press_home(0, repeat=0x40)
    assert press_home(0x2000) == 3 and g.get(box + 116) == 6
    g.put(box + 64, 1)
    assert press_home(0x10) == 1 and g.get(box + 112) == 5
    g.run(g.hooks[0x5C1FA4], home)
    assert colors[home, 11, 1] == 3 and colors[home, 10, 1] == 1
    assert cursor_positions[-1] == 6 * 32 and g.get(box + 112) == 5
    # GU's Manage Equipment child uses the same panel and renders over its home rows.
    assert press_home(0x10) == 1
    assert press_home(0x2000) == 3 and g.get(box + 116) == 5
    assert g.get(home + 156) == 3
    g.run(g.hooks[0x5C1FA4], home)
    assert updates and labels[home, 11, 1] == "扩展装备箱" and labels[home, 0x12, 1] == 17
    assert press_home(0x40) == 3 and g.get(box + 132) == 1
    g.run(g.hooks[0x5C1FA4], home)
    assert labels[home, 11, 1] == "扩展装备箱"
    assert press_home(0x4000) == 1
    g.run(g.hooks[0x5C1FA4], home)
    assert labels[home, 11, 1] == "扩展装备箱"
    assert press_home(0x40) == 1  # Move from Manage Equipment to the SDK row.
    g.run(g.hooks[0x5C1FA4], home)
    labels.clear()
    for _ in range(3):
        press_home(0)
        g.run(g.hooks[0x5C1FA4], home)
    assert not labels  # Stationary cursor leaves the localized text alone.
    # A native popup shortcut can take precedence over cursor navigation.
    g.stub(0x65ED60, lambda *_: 0)
    assert press_home(g.run(0x65F830), repeat=0x40) == 1
    g.run(g.hooks[0x5C1FA4], home)
    assert colors[home, 11, 1] == 3
    busy[0] = 1
    assert press_home(0x2000) == 1
    busy[0] = 0

    def open_inventory(kind=0):
        assert press_home(0x2000) == 3
        assert g.get(box+88) == 1
        for _ in range(kind):
            press_home(0x40)
        assert g.get(box+132) == kind
        assert press_home(0x2000) == 3 and g.get(box+88) == 2
        assert g.half(box+394) == 0x0a0a

    def press_inventory(mask):
        press_home(mask)
        if mask == 0x4000 and g.get(box+88) == 1:
            assert press_home(0x4000) == 1
        return int(g.get(box+64) == 3)

    open_inventory()
    assert g.half(box+400) == 0
    prompt_count = len(prompts)
    assert press_inventory(0x20) == press_inventory(0x2000) == 1
    assert g.get(box + 136) == 0 and all(flags != 20 for flags, _, _ in prompts[prompt_count:])
    assert press_inventory(0x4000) == 0  # The original child close returns to the box.

    assert g.run(grant, 999) == 0
    first, second = g.run(grant, 16), g.run(grant, 16)
    assert (first, second) == (1, 2)
    assert [g.run(upgrade, first) for _ in range(4)] == [1, 1, 1, 0]
    assert g.run(upgrade, 99) == 0

    open_inventory()
    assert g.half(box+400) == 2
    assert g.half(box+392) == 0
    jewel_box, pouch = setup_decorations(g)
    assert press_inventory(0x4000) == 0
    open_inventory(2)
    assert g.get(box+136) == 0
    press_inventory(0x2000)
    assert g.get(box+136) == 1
    press_inventory(0x2000)
    assert g.get(box+136) == 2
    press_inventory(0x40)
    press_inventory(0x2000)
    assert g.get(box+136) == 3
    press_inventory(0)
    assert g.half(jewel_box+28) == 0
    save_character(g)
    assert json.loads(g.files[SAVE])["data"]["inventory"]["decorations"] == {"1":[2638,0,0]}
    # Replace the first jewel using the second entry from the pouch.
    press_inventory(0x2000)
    press_inventory(0x40)
    press_inventory(0x40)
    press_inventory(0x2000)
    assert g.get(box+136) == 3
    press_inventory(0)
    save_character(g)
    assert json.loads(g.files[SAVE])["data"]["inventory"]["decorations"] == {"1":[2639,0,0]}
    assert g.half(jewel_box+28) == 1 and g.half(pouch+28) == 0
    # GU's Y bulk-removal flow returns the jewel to the real item box.
    for _ in range(3):
        if g.get(box+136) != 0: press_inventory(0x4000)
    assert g.get(box+136) == 0
    def remove_all():
        press_inventory(0x8000)
        assert g.get(box+136) == 8
        press_inventory(0x80)
        press_inventory(0x2000)
        assert g.get(box+136) == 9
        press_inventory(0)
        assert g.get(box+136) == 0

    prior_box = bytes(g.cpu.mem_read(jewel_box+26, 2300*4))
    g.cpu.mem_write(jewel_box+26,struct.pack("<HH",9,99)*2300)
    remove_all()
    save_character(g)
    assert json.loads(g.files[SAVE])["data"]["inventory"]["decorations"] == {"1":[2639,0,0]}
    g.cpu.mem_write(jewel_box+26,prior_box)
    remove_all()
    assert sorted([g.half(jewel_box+26),g.half(jewel_box+30)]) == [2638,2639]
    assert g.half(jewel_box+28) == g.half(jewel_box+32) == 1
    assert g.get(box+136) == 0
    assert press_inventory(0x4000) == 0
    open_inventory()
    g.cpu.mem_write(eq + 576, struct.pack("<4H", 11, 4, 23, 7))
    assert press_inventory(0x2000) == 1 and g.get(box + 136) == 1
    assert press_inventory(0) == 1 and prompts[-1] == (20, 119, 0)
    assert g.run(equipped) == 0
    assert press_inventory(0x2000) == 1 and g.get(box + 136) == 3
    assert press_inventory(0) == 1 and g.get(box + 136) == 4
    assert press_inventory(0) == 1 and g.get(box + 136) == 0
    assert g.run(equipped) == 0x10010 and g.run(selected, 0x10010) == 42
    arts = struct.unpack("<4H", g.cpu.mem_read(eq + 576, 8))
    assert arts == (0, 4, 0, 2), arts
    assert press_inventory(0x4000) == 0
    open_inventory(5)
    press_inventory(0x2000)
    assert g.get(box+136) == 0
    assert press_inventory(0x4000) == 0
    open_inventory()
    assert press_inventory(0x1000) == 1 and g.get(box + 136) == 5
    assert press_inventory(0) == 1 and prompts[-1] == (20, 126, 0)
    assert press_inventory(0x2000) == 1 and g.get(box + 136) == 6
    assert press_inventory(0) == 1 and g.get(box + 136) == 0
    assert press_inventory(0x4000) == 0
    assert bytes(g.cpu.mem_read(native_save, 0x3A000)) == original_save
    g.put(ui + 136, 1)
    g.cpu.hook_del(g.traps.pop(0x2772C4))
    g.stub(0x2772C4, lambda *_: actor)
    g.cpu.hook_del(g.traps.pop(0x524040))
    g.stub(0x524040, lambda _, art, *__: art in (4, 5, 6, 11, 23))
    open_box()
    assert g.half(ui + 68) == 3  # Only the unlocked general Arts.
    assert row() == 0 and g.half(ui + 32) == 1 and marks[panel, 7, 1] == 1
    assert labels[widgets, 7, 2] == labels[info, 2, 2] == "Custom weapon"
    dialog = g.alloc(128)
    g.stub(0x65EFD0,lambda *_:dialog)
    def show_guide(_,body,*args):
        labels[dialog,7,1] = g.string(body)
        g.put(dialog+76,1)
        return 1
    g.stub(0x65F190,show_guide)
    g.stub(0x64C600,lambda *_:g.put(dialog+76,0))
    guide = b"First page\nOne\n\nSecond page\nTwo"
    src = g.alloc(len(guide))
    g.cpu.mem_write(src,guide)
    binder = g.get(api_for(g,b"mhgu::StyleManualV1",b"SMN1"))
    assert g.run(binder,42,src,len(guide))
    open_box()
    assert "<ICON PAD_X>" in labels[info,3,2]
    assert press(0x1000)==1
    assert any("First page" in text for text in labels.values() if isinstance(text,str))
    assert press(0x20)==1
    assert any("Second page" in text for text in labels.values() if isinstance(text,str))
    assert press(0x4000)==1
    assert press(0x2000) == 5
    assert g.half(ui + 32) == 3  # GU creates its native three-slot UI.
    assert press(0x2000) == 6
    picked = [11]
    g.stub(0x6390E4, lambda *_: picked[0])
    g.stub(0x54A388, lambda *_: 1)
    assert press(0x2000) == 6 and g.half(eq + 576) == 0
    picked[0] = 6
    assert press(0x2000) == 5 and g.half(eq + 576) == 6
    g.run(0x557D90, ui + 12, 2)
    assert press(0x2000) == 6  # Native confirmation accepts an empty third slot.
    picked[0] = 5
    assert press(0x2000) == 5
    assert press(0x4000) == 1
    assert row() == 0 and marks[panel, 7, 1] == 1
    assert g.run(equipped) == 0x10010 and g.run(selected, 0x10010) == 42
    assert press(0x4000) == 0
    g.cpu.hook_del(g.traps.pop(0x2772C4))
    g.stub(0x2772C4, lambda *_: 0)
    g.cpu.mem_write(eq + 576, bytes(8))
    out = g.alloc(8)
    assert g.run(instance, 0, out) == 1
    assert bytes(g.cpu.mem_read(out, 8)) == struct.pack("<IHH", first, 16, 3)
    assert bytes(g.cpu.mem_read(eq, 1376)) == native
    save_character(g)
    inventory = json.loads(g.files[SAVE])["data"]["inventory"]
    assert inventory == {"instances": {"1": {"equipment": 16, "upgrade": 3},
                                       "2": {"equipment": 16, "upgrade": 0}},
                         "equipped": 1, "sets": {}, "slots": [2, 1]}
    select_character(g, 1)
    assert g.run(instance, first, out) == 0 and g.run(equipped) == 0
    select_character(g, 0)
    assert g.run(equipped) == 0x10010
    g.stub(g.originals[0x144768], lambda *_: 0)
    stock = g.alloc(128)
    g.cpu.mem_write(stock + 32, b"\x07")
    # Execute GU's actual BL in the grid renderer and in the sale eligibility check.
    # Weapon index zero remains GU's physical backing while only armor index one
    # should carry a visible E in the stock box.
    equipped_indices = g.get(0x1885B58) + 132
    previous_indices = bytes(g.cpu.mem_read(equipped_indices,14))
    g.cpu.mem_write(equipped_indices,struct.pack("<7H",0,1,2,3,4,5,6))
    def marked(slot):
        return g.run(0x5B932C,native_save,slot,until=0x5B9330)
    assert not marked(0) and marked(1)
    assert g.run(0x14426C,native_save,0,0) == 1
    assert g.run(0x144C04,native_save,0) == 1
    g.run(g.hooks[0x144768], stock, 0, 0)
    assert marked(0) and marked(1)
    assert g.run(0x144C04,native_save,0) == 0
    g.cpu.mem_write(equipped_indices,previous_indices)
    assert g.run(equipped) == 0 and g.run(instance, first, out) == 1
    g.cpu.mem_write(eq + 1236, b"\x01")
    open_inventory()
    g.cpu.mem_write(box+392,bytes([0,0]))  # Sorted Lv.1 copy remains first after reload.
    assert press_inventory(0x2000) == 1 and g.get(box + 136) == 1
    assert press_inventory(0) == 1 and prompts[-1] == (20, 119, 0)
    assert press_inventory(0x2000) == press_inventory(0) == press_inventory(0) == 1
    assert g.run(instance, 0, out) == 1 and g.get(out) == second
    assert press_inventory(0x4000) == 0
    open_box()
    assert g.half(ui + 32) == 1 and labels[widgets, 7, 2] == "Custom weapon"
    assert press(0x4000) == 0
    open_inventory()
    assert g.half(box + 392) == 0
    g.run(g.hooks[0x144768], stock, 0, 0)
    assert g.run(equipped) == 1

    _, grant_gunner, _, _ = register_weapon(g, ident=17, ranged=True, style=44)
    gunner = g.run(grant_gunner, 17)
    assert gunner == 3
    assert press_inventory(0x4000) == 0
    open_inventory()
    g.cpu.mem_write(box+392,bytes([2,0]))
    assert press_inventory(0x2000) == 1 and g.get(box + 136) == 1
    assert press_inventory(0) == 1 and prompts[-1] == (20, 138, 0)
    assert g.run(equipped) == 1
    assert press_inventory(0x20) == press_inventory(0) == 1
    assert prompts[-1] == (20, 138, 1)
    assert press_inventory(0x2000) == 1 and g.get(box + 136) == 0
    assert g.run(equipped) == 1
    assert press_inventory(0x2000) == 1 and g.get(box + 136) == 1
    assert press_inventory(0) == 1 and prompts[-1] == (20, 138, 0)
    assert press_inventory(0x2000) == press_inventory(0) == press_inventory(0) == 1
    assert g.run(equipped) == 0x10011
    g.run(g.hooks[0x144768], stock, 0, 0)
    assert g.run(equipped) == 1

    # Draw page two through the shipped grid hook with 102 distinct instances.
    assert press_inventory(0x4000) == 0
    for ident in range(4, 103):
        assert g.run(grant, 16) == ident
    open_inventory()
    assert g.half(box + 398) == 2
    assert press_inventory(g.run(0x65F914)) == 1
    assert g.half(box + 396) == 1
    cells = {}
    names = []
    xy = {}
    for addr, axis in ((0x545A40, "x"), (0x545AC0, "y")):
        if addr in g.traps:
            g.cpu.hook_del(g.traps[addr])
        g.stub(addr, lambda _, widget, *rest, axis=axis: xy.update({axis: float_arg()}) if widget == 7 else None)
    for addr in (0x5B8E54, 0x5B87FC, 0x56E6A8):
        g.stub(addr, lambda *_: 0)
    g.stub(0x5B8F38, lambda _, name, _kind, level: names.append((g.string(name), level)))
    if 0x56E530 in g.traps:
        g.cpu.hook_del(g.traps[0x56E530])
    g.stub(0x56E530, lambda _, group, part, visible: cells.update({(group, part): visible}))
    # Enter via GU's real mode dispatcher, including the page cursor child.
    child_vt, hide = g.alloc(32), g.alloc(4)
    g.put(g.get(grid + 96), child_vt)
    g.put(child_vt + 24, hide)
    g.put(child_vt + 28, hide)
    g.stub(hide, lambda *_: 0)
    assert g.get(grid + 80) == 6 and g.get(g.get(grid + 92) + 124) == box + 372
    g.run(0x5B68FC, grid)
    assert xy == {"x": 0, "y": 0}
    assert [part for (group, part), visible in cells.items() if group == 12 and visible] == [2, 3]
    cells.clear()
    assert names[-1] == ("Custom weapon", 101)
    assert press_inventory(0x20) == 1
    g.run(0x5B68FC, grid)
    assert xy == {"x": 36, "y": 0}
    xy.clear()
    g.run(0x5B68FC, grid)
    assert xy == {"x": 36, "y": 0}
    assert bytes(g.cpu.mem_read(native_save, 0x3A000)) == original_save

    # Move the final duplicate into an empty slot on page two, then reload.
    assert press_inventory(0x4000) == 0
    open_inventory(4)
    press_inventory(g.run(0x65F914))
    press_inventory(0x20)
    press_inventory(0x2000)
    press_inventory(0x20)
    press_inventory(0x2000)
    assert g.get(box+136) == 2
    press_inventory(0)
    save_character(g)
    saved = json.loads(g.files[SAVE])["data"]["inventory"]
    assert saved["slots"][100:103] == [101, 0, 102]
    data = json.loads(g.files[SAVE])
    saved = data["data"]["inventory"]
    saved["instances"]["102"]["upgrade"] = 3
    saved["decorations"] = {"102": [2638, 0, 0]}
    saved["sets"]["2"] = {"instance": 102, "fingerprint": 17}
    saved["sets"]["3"] = {"instance": 101, "fingerprint": 18}
    g.files[SAVE] = json.dumps(data).encode()
    item_rows = g.get(g.get(g.get(0x18583A8) + 28) + 108)
    g.put(item_rows + 56 * 2638 + 28, 37)
    press_inventory(0x4000)
    select_character(g, 0)
    open_inventory(5)
    press_inventory(g.run(0x65F914))
    press_inventory(0x20)
    press_inventory(0x20)
    # Quote and cancel, then sell the same duplicate and verify wallet and reload.
    sale = api_for(g,b"mhgu::EquipmentSale",b"SALE")
    # GU 1434E4 resolves the source series and calls its level-specific price.
    catalog, vt, kind_fn, price_fn = (g.alloc(n) for n in (4,352,4,4))
    g.put(catalog,vt)
    g.put(vt+24,kind_fn)
    g.put(vt+348,price_fn)
    g.stub(kind_fn,lambda *args:14)
    g.stub(price_fn,lambda _,level,*args:level * 100)
    g.cpu.hook_del(g.traps.pop(0x13c208))
    g.stub(0x13c208,lambda _,kind,series,*args:catalog if (kind,series)==(14,1) else 0)
    assert g.run(g.get(sale+4),16,3,(1<<16) | (6<<5) | 14) == 0
    drawn = []
    g.stub(g.originals[0x5b90a8],lambda _,name,price,*args:drawn.append((g.string(name),price)))
    g.run(0x5B68FC,grid)
    assert drawn[-1] == ("Custom weapon",637)
    player = g.get(0x1897F78)
    balance = g.get(player + 36)
    for confirm in (False, True):
        press_inventory(0x2000)
        assert g.get(box+136) == 1, (confirm, g.get(box+824),g.half(box+396),prompts[-1],g.get(box+136))
        press_inventory(0)
        assert prompts[-1][:2] == (20,144)
        press_inventory(0x2000 if confirm else 0x4000)
        if confirm: press_inventory(0)
        else:
            press_inventory(0x4000)  # GU first moves the choice to No, then dismisses.
            assert g.get(box+136) == 0
            assert g.run(instance, 102, out) and g.get(player + 36) == balance
    assert not g.run(instance, 102, out) and g.run(instance, 101, out)
    assert g.get(player + 36) == balance + 637
    save_character(g)
    select_character(g, 0)
    assert not g.run(instance, 102, out) and g.run(instance, 101, out)
    remaining = json.loads(g.files[SAVE])["data"]["inventory"]
    assert remaining["sets"] == {"3": {"instance": 101, "fingerprint": 18}}
    assert not remaining.get("decorations")
    assert bytes(g.cpu.mem_read(native_save, 0x3A000)) == original_save

    without_mod = boot(elf, g.files[SAVE])
    hunters(without_mod)
    api = api_for(without_mod, b"mhgu::WeaponRegistryV7", b"WPN7")
    assert without_mod.run(without_mod.get(api + 4)) == 0

    old = json.loads(character_save({0: 10}))
    old["data"]["custom_weapons"] = [16]
    g.files[SAVE] = json.dumps(old).encode()
    select_character(g, 0)
    assert g.run(equipped) == 1 and g.run(instance, first, out) == 0

    print(
        "GU equipment-box entry, native style panel, expansion inventory, My Set, status and single-file reload passed"
    )


if __name__ == "__main__":
    verify()
