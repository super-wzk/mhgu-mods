"""Buy and forge the actual packaged Magnet Spike through the GU facility hooks."""

import hashlib
import json
import os
import struct
import sys
from pathlib import Path

from unicorn import UC_HOOK_CODE
from unicorn.arm_const import UC_ARM_REG_S0, UC_ARM_REG_SP

ROOT = Path(__file__).resolve().parents[1]
REPO = ROOT.parents[1]
sys.path.insert(0, str(REPO / "plugins/great-sword-extreme/tests"))
from verify_style import MANAGER, PROVIDER, SAVE, api_for, boot, character_save, select_character
from verify_trade import A, B, DOWN, LEFT, Facility, setup_items, verify_save

PLUGIN = ROOT / "dist/magnet-spike/romfs/nativeNX/plugins/magnet_spike.nro"
ICON = 255
NAME = "试制磁斩锤"
POTION = 9


def attach_details(g, facility):
    # Supply the retail layout boundary; run GU's attack, affinity, defense
    # and element widget renderers against the active SDK equipment record.
    panel = g.alloc(260)
    g.put(g.get(facility.owner + 52) + 5 * 4, panel)
    g.put(0x1899220, g.alloc(4))
    label = g.alloc(8)
    g.cpu.mem_write(label, b"Label\0")
    g.cpu.hook_del(g.traps.pop(0x55BC4C))
    g.stub(0x55BC4C, lambda *_: label)
    for address in (0x57F468, 0x56EA08, 0x56FE88):
        g.stub(address, lambda *_: 0)
    values = {}
    g.stub(0x56FDE0, lambda _, group, part, value: values.update({(group, part): value}))
    return values


def observe_badge(g, facility):
    # Execute the w07 catalogue vtable and GU's DBD74 / level colour logic.
    # Raw row 1 starts after the header and the 33-byte empty row.
    base = g.alloc(40)
    g.put(base, 0x1739A4C)
    limits = bytes([8, 3])
    if os.environ.get("MHGU_ROMFS"):
        table = Path(os.environ["MHGU_ROMFS"]) / "nativeNX/table/weapon07BaseData.w07d"
        assert table.read_bytes()[50:52] == limits
    g.cpu.mem_write(base + 13, limits)
    g.put(0x1856B18, g.alloc(512))
    g.cpu.hook_del(g.traps.pop(0x13C208))
    g.stub(0x13C208, lambda _, tag, series, *args: base if (tag, series) == (14, 1) else 0)
    panel = g.get(g.get(facility.owner + 52) + 20)
    g.put(panel + 36, g.alloc(64))
    g.stub(0x546AFC, lambda *args: 0)
    badges = []
    if 0x56EDB0 in g.traps:
        g.cpu.hook_del(g.traps.pop(0x56EDB0))
    g.stub(0x56EDB0, lambda p, group, part, sprite:
           badges.append((sprite, g.get(g.cpu.reg_read(UC_ARM_REG_SP)))) if (p,group,part)==(panel,17,1) else None)
    return badges


def verify_manual(g):
    panel, dialog = g.alloc(1520), g.alloc(128)
    guide = []
    g.stub(0x65E580, lambda *_: 0)
    g.stub(0x65EFD0, lambda *_: dialog)

    def open_guide(_, body, *_args):
        guide.append(g.string(body))
        return 1

    g.stub(0x65F190, open_guide)
    pressed = g.get(0x188C670) + 27644
    g.put(pressed, 0x2000)
    g.run(g.hooks[0x5CB780], panel)
    assert len(guide) == 1 and guide[0].startswith("极之型磁斩锤")
    from verify_manual import expand
    pages = (ROOT / "src/manual.txt").read_text().strip().split("\n\n")
    g.stub(0x56E808, lambda _, group, part, body: guide.append(g.string(body)))
    g.put(pressed, 0x20)
    for index, page in enumerate(pages):
        if index:
            g.run(g.hooks[0x5CB780], panel)
        assert expand(page).replace("\t", "　").replace("\n", "\r\n") in guide[index]
    g.stub(0x64C600, lambda *_: 0)
    g.put(pressed, 0x4000)
    g.run(g.hooks[0x5CB780], panel)
    g.put(pressed, 0)


def verify_notes(g):
    panel, state, cursor, vt, detail, arrows = (g.alloc(n) for n in (100,4,100,80,100,140))
    g.put(panel+92,state)
    g.put(panel+76,cursor)
    g.put(cursor,vt)
    page, selected = [1], [15]
    for offset, value in [(36,page),(56,selected)]:
        callback = g.alloc(4)
        g.put(vt+offset,callback)
        g.stub(callback,lambda *_, value=value: value[0])
    g.put(detail+84,arrows)
    labels = {}
    def substitute(addr, fn):
        if addr in g.traps:
            g.cpu.hook_del(g.traps.pop(addr))
        g.stub(addr,fn)
    for addr in (0x54533C,0x56E538,0x56E6A8,0x65EB68,0x54EDEC,0x600DE4,
                 0x56E528,0x56E530,0x550354,0x56E6FC,0x65D424,0x65D464,
                 0x65E794,0x65E5BC,0x56EAB8,0x54EE34):
        substitute(addr,lambda *_:0)
    empty = g.alloc(4)
    substitute(0x55BC4C,lambda *_: empty)
    substitute(0x56E808,lambda _,group,part,text: labels.update({(group,part):g.string(text)}))
    g.cpu.mem_write(panel+88,b"\1")
    g.run(g.hooks[0x5B1294],panel)
    assert g.cpu.mem_read(panel+80,1)==bytes([16])
    g.run(g.hooks[0x5B0F70],panel)
    assert labels[14,1]=="磁斩锤"
    g.run(0x5B12D4,panel)
    selected[0]=page[0]=0
    g.run(g.hooks[0x5B1294],panel)
    assert g.cpu.mem_read(panel+80,1)==bytes([1])
    g.run(g.hooks[0x5B0F70],panel)
    assert labels[9,1]=="极之型磁斩锤"
    g.cpu.mem_write(detail+81,bytes([15,0,255]))
    g.run(g.hooks[0x646A3C],detail,cursor)
    pages=(ROOT/"src/manual.txt").read_text().strip().split("\n\n")
    assert g.cpu.mem_read(detail+80,1)==bytes([len(pages)])
    for i, body in enumerate(pages):
        page[0]=i
        g.run(g.hooks[0x646AD0],detail)
        assert labels[39,2]=="磁斩锤"
        if "\t" in body:
            assert labels[3,2]==body.splitlines()[0]
        else:
            assert labels[17,1].startswith(body.splitlines()[0])
    assert g.run(0x5B132C,panel) and g.cpu.mem_read(state+1,1)==bytes([15])


def verify(sdk_last):
    g = boot((REPO / "mhgu/main.elf").read_bytes(), None, order=())
    paths = (PLUGIN, PROVIDER) if sdk_last else (PROVIDER, PLUGIN)
    for path in paths:
        exports = g.load(path, 0x9200000 if path == PLUGIN else 0x9000000)
        for callback in ("forge_onLoad", "forge_onInit"):
            g.run(exports[callback], MANAGER)
    select_character(g, 0)

    api = api_for(g, b"mhgu::WeaponRegistryV7", b"WPN7")
    base, level, model = (g.get(api + at) for at in (24, 28, 32))
    assert g.half(g.run(base, 0x1001) + 2) == 16
    progression = [(100, 0, 10, 0), (110, 0, 17, 4), (140, 1, 13, 6), (170, 1, 16, 8),
                   (210, 1, 18, 8), (240, 2, 68, 8), (290, 2, 15, 10), (320, 3, 14, 10)]
    sale = api_for(g, b"mhgu::EquipmentSale", b"SALE")
    native_price = g.get(sale + 4)
    for i, (attack, slots, curve, sharpness) in enumerate(progression):
        header = (1 << 16) | ((i + 3) << 5) | 14
        assert g.run(native_price, 0x1001, i, header + (1 << 16)) == 2
        assert g.run(native_price, 0x1001, i, header) == 0
        record = g.run(level, 0x1001, i)
        assert struct.unpack("<3hBb3Bx", g.cpu.mem_read(record, 12)) == (
            attack, 0, 0, 0, 0, slots, curve, sharpness)
    assert not g.run(level, 0x1001, len(progression))
    assert g.string(g.run(model, 0x1001, 1)) == r"arc\frontier\magnet_spike"
    assert g.string(g.run(model, 0x1001, 0)) == r"weapon\frontier\magnet_spike\wk451"
    rarity_color = 0xffaabbcc
    g.put(0x18994B4, rarity_color)
    instance = g.get(api + 16)
    out = g.alloc(8)
    g.put(0x188C670, g.alloc(27704))
    verify_manual(g)
    wallet = g.get(0x1897F78)
    g.put(wallet + 36, 200)
    pouch, _, _ = setup_items(g)
    records = g.get(g.get(g.get(0x18583A8) + 28) + 108)
    potion_record = records + 56 * POTION
    g.put(potion_record + 8, POTION)
    g.cpu.mem_write(potion_record + 12, b"\x01\0\x0a")
    names, potion_name = g.alloc(4), g.alloc(16)
    g.cpu.mem_write(potion_name, "回复药\0".encode())
    g.put(0x18583AC, names)
    g.stub(0xB84114, lambda table, index, *_: potion_name if table == names and index == 2 * POTION else 0)
    g.put(pouch + 26, (2 << 16) | POTION)

    preview_manager, pose = g.alloc(40032), g.alloc(64)
    g.put(0x1885B58, preview_manager)
    g.put(pose + 28, 7 | (1 << 16))
    g.stub(0x275A30, lambda manager, *_: pose if manager == preview_manager else 0)
    g.stub(0x276278, lambda *_: 0)
    preview_calls = []
    for address in (0x6ED600, 0x6FC2C8):
        g.cpu.hook_add(UC_HOOK_CODE, lambda _cpu, pc, *_: preview_calls.append(pc),
                       begin=address, end=address)
    # The rest of the renderer depends on scene and graphics services. Retain
    # the actual facility preview setup and pad handling in this replay.
    g.stub(0x4AD0E8, lambda *_: 0)

    shop = Facility(g, False)
    shop.connect()
    verify_icon(g)
    values = attach_details(g, shop)
    badges = observe_badge(g, shop)
    shop_viewer = g.alloc(160)
    g.put(shop.owner + 2016, shop_viewer)
    shop.open("磁斩锤", ICON)
    assert (g.get(shop_viewer + 72), g.get(shop_viewer + 76), g.get(shop_viewer + 60)) == (7, 1, 1)
    g.put(g.get(0x188C670) + 27636, 0x10000)
    shop.press()
    assert g.get(shop_viewer + 88) == 1
    assert badges[-1] == (0, 0)
    assert values[2, 2] == 100 and values[2, 28] == 0
    assert NAME in shop.rows[0] and g.get(shop.header+24)==100
    assert g.get(shop.header + 16) == rarity_color and g.half(shop.header + 20) == ICON
    shop.press(A)
    shop.press(A)
    assert g.get(wallet + 36) == 100 and g.half(pouch + 28) == 2
    assert (0, 10) in shop.sounds
    shop.answer_equip(True)
    assert g.run(instance, 0, out) and g.get(out) == 1

    verify_member_icon(g, preview_manager)

    smith = Facility(g, True)
    smith.connect()
    attach_details(g, smith)
    badges = observe_badge(g, smith)
    smith_viewer = g.alloc(160)
    g.put(smith.owner + 12816, smith_viewer)
    smith.open("磁斩锤", ICON)
    assert (g.get(smith_viewer + 72), g.get(smith_viewer + 76), g.get(smith_viewer + 60)) == (7, 1, 1)
    smith.press()
    assert g.get(smith_viewer + 88) == 1
    assert set(preview_calls) == {0x6ED600, 0x6FC2C8}
    assert smith.rows == {0: NAME} and g.get(smith.header+24)==0
    assert g.get(smith.header + 16) == rarity_color and g.half(smith.header + 20) == ICON
    smith.press(A)
    assert g.string(g.get(smith.owner+11828))=="回复药"
    assert g.half(smith.owner+11820)==1 and g.half(smith.owner+11822)==2
    smith.press(A)
    assert g.get(wallet + 36) == 100 and g.half(pouch + 28) == 1, (
        g.get(wallet + 36), g.half(pouch + 28), g.cpu.mem_read(smith.owner + 4, 2)
    )
    assert (1, 117) in smith.sounds
    smith.answer_equip(True)
    assert g.run(instance, 0, out) and g.get(out) == 2

    for ident in (1, 2):
        assert g.run(instance, ident, out)
        assert bytes(g.cpu.mem_read(out, 8)) == struct.pack("<IHH", ident, 0x1001, 0)

    # Open the smithy home page at exactly the four stock rows in the game.
    smith.press(B)
    smith.press(B)
    assert g.cpu.mem_read(smith.owner + 4, 1) == b"\1"
    title = "扩展装备强化"
    def open_upgrades():
        rows = smith.render_category()
        assert set(rows) == set(range(5)) and rows[3] == title, rows
        g.run(0x557D90, smith.cursor, 2)
        smith.press(DOWN)
        assert smith.render_category()[3] == title and smith.highlights[3] == 4
        smith.press(A)
        assert g.cpu.mem_read(smith.owner + 4, 2) == bytes((7, 1))
        assert g.half(smith.owner + 8388) == 2
    open_upgrades()
    smith.press(B)
    assert g.cpu.mem_read(smith.owner + 4, 1) == bytes([1])
    assert smith.render_category()[3] == title and smith.highlights[3] == 4
    open_upgrades()
    g.put(pouch + 26, (1 << 16) | POTION)
    smith.press(A)
    assert g.cpu.mem_read(smith.owner + 4, 3) == bytes((7, 25, 0))
    for target in range(2, 9):
        assert badges[-1] == (int(target == 8), int(target >= 5)), (target, badges[-3:])
        assert g.half(smith.owner + 10464) == 1
        assert g.string(g.get(smith.owner + 11828)) == "回复药"
        assert g.half(smith.owner + 11820) == 1
        smith.press(A)
        assert g.cpu.mem_read(smith.owner + 6, 1) == bytes([5])
        if target == 2:
            smith.press(B)  # GU first moves Yes/No to No.
            smith.press(A)
            assert g.run(instance, 1, out) and g.half(out + 6) == 0
            smith.press(A)
        smith.press(A)
        smith.press()
        assert g.run(instance, 1, out) and g.half(out + 6) == target - 1, (target, g.half(out + 6), bytes(g.cpu.mem_read(smith.owner + 4, 4)), g.half(smith.owner + 10452), g.half(pouch + 28))
        if target == 2:
            assert g.cpu.mem_read(smith.owner + 6, 1) == bytes([10])
            smith.press(LEFT)
            smith.press(A)
            assert g.run(instance, 0, out) and g.get(out) == 1
        assert (1, 118) in smith.sounds
        assert g.get(wallet + 36) == 100 and g.half(pouch + 28) == 0
        assert g.run(instance, 2, out) and g.half(out + 6) == 0
        g.put(pouch + 26, (1 << 16) | POTION)
        smith.press()
    assert g.cpu.mem_read(smith.owner + 4, 2) == bytes((7, 1))
    # Reuse the very same details panel after painting a final-level weapon.
    smith.press(0x20)
    assert badges[-1] == (0, 0)
    smith.press(LEFT)
    assert badges[-1] == (1, 1)
    smith.press(A)
    assert g.cpu.mem_read(smith.owner + 5, 1) == bytes([1])
    smith.press(B)
    smith.press(DOWN)
    assert smith.render_category()[3] == title and smith.highlights[4] == 4
    smith.press(A)
    assert g.cpu.mem_read(smith.owner + 4, 1) == b"\2" and g.cpu.mem_read(smith.owner + 76, 1) == b"\3"

    # Exercise saving and reloading a real upgraded instance, then restore the
    # fixture's files for the independent equipment-stat scenarios below.
    if sdk_last:
        files_before_save = g.files.copy()
        verify_save(g, instance, out, expected_upgrade=7, last_instance=2)
        assert g.run(instance, 2, out) and g.half(out + 6) == 0
        g.files.clear()
        g.files.update(files_before_save)

    g.cpu.hook_del(g.traps.pop(0x276278))
    verify_equipment(g, api)
    verify_notes(g)


def verify_member_icon(g, manager):
    panel, actor, vtable, tint = (g.alloc(size) for size in (96, 5200, 420, 4))
    g.put(panel + 88, 6)
    g.put(manager + 28, actor)
    g.put(actor, vtable)
    g.put(vtable + 416, tint)
    g.stub(tint, lambda *_: 0)
    g.put(actor + 5060, manager + 124)
    draws = []
    hooks = []
    # Capture both the native atlas request and the SDK override at the renderer.
    for addr in (0x56E530, 0x56E6FC, 0x560F80, 0x56E6A8):
        def intercept(cpu, pc, _, __):
            from unicorn import arm_const as arm
            if pc == 0x56E6A8:
                draws.append((cpu.reg_read(arm.UC_ARM_REG_R3), g.get(cpu.reg_read(UC_ARM_REG_SP))))
            cpu.reg_write(arm.UC_ARM_REG_R0, 0)
            cpu.reg_write(arm.UC_ARM_REG_PC, cpu.reg_read(arm.UC_ARM_REG_LR))
        hooks.append(g.cpu.hook_add(UC_HOOK_CODE, intercept, begin=addr, end=addr))
    g.run(g.hooks[0x634124], panel)
    assert draws == [(4, g.cpu.mem_read(manager + 124 + 1236, 1)[0]), (0, ICON)], draws
    g.put(actor + 5068, 4)
    draws.clear()
    g.run(g.hooks[0x634124], panel)
    assert draws == [(5, 2)], draws
    g.put(manager + 28, 0)
    for hook in hooks:
        g.cpu.hook_del(hook)


def verify_icon(g):
    tex = (PLUGIN.parents[1] / "GUI/frontier_magnet_spike/icon.tex").read_bytes()
    assert tex[:4] == b"TEX\0" and tex[13] == 7
    assert struct.unpack_from("<I", tex, 8)[0] == 64 << 19 | 64 << 6 | 1
    assert any(tex[27::4]) and not all(tex[27::4])

    panel, layout, groups, group, nodes, sprite = [
        g.alloc(size) for size in (200, 680, 28, 240, 48, 300)
    ]
    g.put(panel + 36, layout)
    g.put(layout + 248, groups)
    g.put(layout + 664, 7)
    g.put(groups + 24, group)
    g.put(group + 216, nodes)
    g.put(nodes + 44, sprite)
    vt, ret = g.alloc(84), g.alloc(4)
    g.put(sprite, vt)
    g.put(vt + 80, ret)
    g.stub(ret, lambda *_: 0)
    stock = g.alloc(64)
    g.put(stock, 12)
    g.put(sprite + 120, layout)
    g.put(sprite + 256, 12)
    g.put(sprite + 260, stock)
    g.stub(0xB80FD0, lambda owner, ident, *_: stock if (owner, ident) == (layout, 12) else 0)

    manager, vtable, load, texture = [g.alloc(size) for size in (4, 56, 4, 240)]
    previous = g.get(0x2126A64)
    g.put(0x2126A64, manager)
    g.put(manager, vtable)
    g.put(vtable + 48, load)
    g.put(texture + 80, 1)
    g.put(texture + 220, 64)
    g.put(texture + 224, 64)
    requests = []

    def load_texture(_, kind, path, flags):
        requests.append((kind, g.string(path), flags))
        return texture

    g.stub(load, load_texture)
    draw = g.hooks[0x56E6A8]
    retail_crop = g.alloc(16)
    g.cpu.mem_write(retail_crop, struct.pack("<4f", 128, 64, 32, 32))
    for atlas in (0, 1):
        g.run(draw, panel, 6, 11, atlas, ICON)
        private = g.get(sprite + 260)
        assert private != stock and g.get(private + 40) == texture
        assert struct.unpack("<4f", g.cpu.mem_read(sprite + 268, 16)) == (0, 0, 64, 64)
        assert g.get(sprite + 256) == 12
        # GU's native equipment icon path (56EB24) uses this crop directly.
        g.run(0x546F84, layout, 6, 11, retail_crop)
        assert g.get(sprite + 260) == stock
        assert struct.unpack("<4f", g.cpu.mem_read(sprite + 268, 16)) == (128, 64, 32, 32)
    assert requests == [(0x21255D0, r"GUI\frontier_magnet_spike\icon", 1)]
    g.put(0x2126A64, previous)
    return panel, layout, groups, group, nodes, sprite


def verify_room_icons():
    g = boot((REPO / "mhgu/main.elf").read_bytes(), None, order=())
    g.inline_hooks |= {0x56E6A8}
    for path, base in ((PROVIDER, 0x9000000), (PLUGIN, 0x9200000)):
        exports = g.load(path, base)
        for cb in ("forge_onLoad", "forge_onInit"):
            g.run(exports[cb], MANAGER)
    data = json.loads(character_save({}))
    data["data"]["inventory"] = {"instances": {"1": {"equipment": 0x1001, "upgrade": 0}},
                                 "equipped": 1, "sets": {}}
    g.files[SAVE] = json.dumps(data).encode()
    select_character(g, 0)
    panel, layout, groups, group, nodes, sprite = verify_icon(g)
    manager, network, peer = g.alloc(40032), g.alloc(3200), g.alloc(1376)
    g.put(0x1885B58, manager)
    g.put(0x188CA48, network)
    g.put(network + 48, 2)
    g.stub(0x27617C, lambda _, slot, *__: peer if slot == 0 else 0)
    for eq in (manager + 124, peer):
        g.cpu.mem_write(eq + 1236, bytes([7]))  # same stock category
    native = []
    g.stub(g.originals[0x56E6A8], lambda _, grp, part, atlas: native.append((grp, part, atlas)))
    quest = g.alloc(280)
    g.stub(0x3B9FD4, lambda *_: quest)
    for address in (0x60DCEC, 0x56E530, 0x56ED3C, 0x56DF00, 0x56E528, 0x3BF718,
                    0x3B598C, 0x55BC7C, 0x56E538, 0x56EA08, 0x3B57D0, 0x56E808,
                    0x560E90, 0x56E6FC):
        g.stub(address, lambda *_: 0)
    g.put(panel + 80, 6)
    g.cpu.mem_write(panel + 141, bytes([1]))
    g.put(panel + 144, 2)
    g.put(panel + 136, 2)
    g.run(g.hooks[0x60DB68], panel)
    assert not native and g.get(g.get(sprite + 260)) == 0xffffffff, (native, g.get(sprite + 260))
    g.put(panel + 136, 0)
    g.run(g.hooks[0x60DB68], panel)
    assert native.pop() == (6, 11, 4)
    g.cpu.mem_write(panel + 168, bytes([1]))
    g.run(g.hooks[0x60DB68], panel)
    assert native.pop() == (6, 11, 22)

    quests = g.alloc(2 * 10328)
    posting = quests + 10328
    g.put(posting, 1)
    g.put(posting + 10272, 3)
    for row, slot in enumerate((0, 3, 2)):
        g.put(posting + 10280 + row * 12, slot)
    row_groups = [g.get(0x163476C + 4 * row) for row in range(4)]
    groups = g.alloc(4 * (max(row_groups) + 1))
    g.put(layout + 248, groups)
    g.put(layout + 664, max(row_groups) + 1)
    g.put(groups + 4 * row_groups[2], group)
    g.put(nodes + 8, sprite)
    g.put(sprite + 260, 0)
    g.run(g.hooks[0x61225C], panel, 0, 1, quests, 0)
    assert native == [(row_groups[0], 2, 4)] and g.get(g.get(sprite + 260)) == 0xffffffff, (native, g.get(sprite + 260))
    # A copied peer record carries its own identity. Replacing that copy with
    # an untagged weapon of the same stock class must stop the override.
    g.stub(g.originals[0xE175C], lambda *_: 0)
    g.put(groups + 4 * row_groups[0], group)
    g.put(groups + 6 * 4, group)
    g.run(g.hooks[0xE175C], peer + 4, manager + 128)
    native.clear()
    g.run(g.hooks[0x61225C], panel, 0, 1, quests, 0)
    assert not native
    g.cpu.mem_write(panel + 168, bytes([0]))
    g.run(g.hooks[0x60DB68], panel)
    assert not native
    g.run(g.hooks[0xE175C], peer + 4, g.alloc(1376) + 4)
    g.run(g.hooks[0x60DB68], panel)
    assert native == [(6, 11, 4)]
    print("Room player info and posted quest rows: local/peer identity, replaced equipment, hidden and masked rows passed")


def verify_status_panels():
    g = boot((REPO / "mhgu/main.elf").read_bytes(), None, order=("registry",))
    plugin = g.load(PLUGIN, 0x9200000)
    for callback in ("forge_onLoad", "forge_onInit"):
        g.run(plugin[callback], MANAGER)

    manager = g.alloc(40032)
    g.put(0x1885B58, manager)
    equipment = manager + 124
    weapon = equipment + 24
    g.cpu.mem_write(weapon + 4, struct.pack("<3H", 14, 1, 0))
    g.cpu.mem_write(weapon + 48, struct.pack("<3H", 1 | (2 << 10), 5, 8))
    saved = json.loads(character_save({}))
    saved["data"]["inventory"] = {"instances": {"1": {"equipment": 0x1001, "upgrade": 7}},
                                   "equipped": 1, "sets": {}}
    g.files[SAVE] = json.dumps(saved).encode()
    select_character(g, 0)

    list_panel, detail = g.alloc(464), g.alloc(256)
    g.put(list_panel + 76, g.alloc(36))
    palette = g.alloc(17900)
    g.put(0x1899458, palette)  # GU 553B64 consumes this UI revision on refresh.
    labels, icons = {}, {}

    def catalog_name(panel, group, part, record):
        layered = g.cpu.mem_read(palette + 17892, 1)[0] and g.half(record + 8)
        labels[panel, group, part] = "GU cosmetic armour" if layered else "GU equipment"

    g.stub(g.originals[0x56E810], catalog_name)
    g.stub(g.originals[0x56EA38], lambda panel, group, part, _: icons.update({(panel, group, part): 7}))
    g.stub(0x56E6A8, lambda panel, group, part, _: icons.update({(panel, group, part): g.get(g.cpu.reg_read(UC_ARM_REG_SP))}))
    for address in (0x56E808, 0x56EA08):
        g.stub(address, lambda panel, group, part, message: labels.update({(panel, group, part): g.string(message)}))
    for address in (0x545AC0, 0x54533C, 0x545C20, 0x56E6FC, 0x560EF0):
        g.stub(address, lambda *_: 0)

    weapon_group, armour_group = g.get(0x163053C), g.get(0x1630540)
    g.run(0x589D50, list_panel, equipment + 4)
    assert labels[list_panel, weapon_group, 2] == NAME and icons[list_panel, weapon_group, 1] == ICON
    g.run(0x553B5C, detail + 196, weapon)
    g.put(detail + 248, 5)
    g.cpu.mem_write(palette + 17892, bytes([1]))
    g.run(0x589C74, list_panel)
    assert labels[list_panel, weapon_group, 2] == NAME and icons[list_panel, weapon_group, 1] == ICON
    assert labels[list_panel, armour_group, 2] == "GU cosmetic armour"
    g.run(0x57E6F0, detail)
    assert labels[detail, 17, 3] == NAME
    g.run(0xDAD24, detail + 200)
    g.cpu.mem_write(palette + 17892, bytes([0]))
    g.run(0x57E6F0, detail)
    assert labels[detail, 17, 3] == "GU equipment"
    g.files[SAVE] = character_save({})
    select_character(g, 0)
    g.run(0x589D50, list_panel, equipment + 4)
    assert labels[list_panel, weapon_group, 2] == "GU equipment" and icons[list_panel, weapon_group, 1] == 7
    g.run(0x553B5C, detail + 196, weapon)
    g.cpu.mem_write(palette + 17892, bytes([1]))
    g.run(0x57E6F0, detail)
    assert labels[detail, 17, 3] == "GU equipment"

    # Keep GU's widget positioning calls. Supply only the layout, colour widths,
    # and labels so the test observes the marker's real X coordinate.
    g.files[SAVE] = json.dumps(saved).encode()
    select_character(g, 0)
    tables, curves, entries = g.alloc(256), g.alloc(112), g.alloc(20 * 15)
    g.put(0x1856B18, tables)
    g.put(tables + 252, curves)
    g.put(curves + 100, 15)
    g.put(curves + 108, entries)
    g.cpu.mem_write(entries + 20 * 14 + 4, struct.pack("<7H", 60, 100, 146, 306, 356, 410, 450))
    layout, groups, group, parts = (g.alloc(n) for n in (668, 12, 220, 84))
    g.put(detail + 36, layout)
    g.put(layout + 248, groups)
    g.put(layout + 664, 3)
    g.put(groups + 8, group)
    g.put(group + 216, parts)
    sprites = {part: g.alloc(132) for part in (9, 10, 12)}
    for part, sprite in sprites.items():
        g.put(parts + 4 * part, sprite)
    g.put(sprites[12] + 128, struct.unpack("<I", struct.pack("<f", 17.0))[0])
    g.put(detail + 192, struct.unpack("<I", struct.pack("<f", 180.0))[0])
    g.put(detail + 80, 0x8000)
    g.put(detail + 184, equipment)
    g.put(equipment + 396, g.alloc(1100))
    message = g.alloc(4)
    g.cpu.mem_write(message, b"%d\0")
    g.put(0x1899220, g.alloc(4))
    g.stub(0x55BC4C, lambda *_: message)
    g.stub(0x560E90, lambda *_: 0)
    visible, widths, bonuses = {}, {}, []
    g.stub(0x56E530, lambda _, group, part, show: visible.update({(group, part): show}))
    g.stub(0x546AFC, lambda _, group, part, __: widths.update({(group, part): struct.unpack("<f", struct.pack("<I", g.cpu.reg_read(UC_ARM_REG_S0)))[0]}))
    g.stub(0x56FFAC, lambda *_: bonuses.append(g.get(g.cpu.reg_read(UC_ARM_REG_SP))))
    skill_words = g.get(equipment + 396) + 988
    for skill, bonus, bar_end in ((83, 2, 180), (84, 1, 170), (0, 0, 160)):
        g.put(skill_words + 8, 1 << (skill % 32) if skill else 0)
        g.run(0xE174C, equipment + 4, 14, 10 + bonus)
        g.run(0x5859F4, detail, weapon)
        assert widths[2, 18] == bar_end
        assert [visible[2, part] for part in (9, 10, 20)] == [bonus == 2, bonus == 1, bonus != 0]
        if bonus:
            sprite = sprites[11 - bonus]
            assert struct.unpack("<f", g.cpu.mem_read(sprite + 128, 4))[0] == bar_end + 18
            assert bonuses[-1] == bonus
    assert bonuses == [2, 1]

    # The pause details redraw the weapon stats after the native page returns.
    # Verify that the redraw keeps the same marker when passed its stats copy.
    numbers = {}
    clears = []
    def native_preview(panel, record, compared, *_):
        if not record:
            clears.append(panel)
            return
        g.put(panel + 248, 1)
        # Simulate the original renderer leaving values from the fallback
        # catalogue record in the two panels.
        numbers[2, 2] = 90 if compared else 0
        numbers[2, 28] = 30 if compared else 0
        visible[2, 5] = visible[2, 6] = int(compared != 0)

    g.stub(g.originals[0x57F468], native_preview)
    g.stub(0x56FDE0, lambda _, group, part, value: numbers.update({(group, part): value}))
    for address in (0x13F03C, 0x550534, 0x560E5C, 0x56E528, 0x56EDB0, 0x56FE88,
                    0x585910, 0x585E98, 0x586398, 0x586574):
        g.stub(address, lambda *_: 0)
    preview, preview_skills = g.alloc(1376), g.alloc(1100)
    g.put(preview + 396, preview_skills)
    g.put(preview_skills + 996, 1 << (83 % 32))
    g.run(0x57F468, detail, weapon, 0, preview, 0, 0x8000, 0, 0)
    assert labels[detail, 17, 3] == NAME
    assert (numbers[2, 2], numbers[2, 28]) == (320, 0), numbers
    assert visible[2, 9] and bonuses[-1] == 2 and widths[2, 18] == 180
    assert struct.unpack("<f", g.cpu.mem_read(sprites[9] + 128, 4))[0] == 198

    # A second Lv.8 instance uses the same SDK definition but has its own
    # native comparison cEquip. A fallback base must not carry over.
    candidate = g.alloc(1376)
    g.run(0x553B5C, candidate + 20, weapon)
    g.put(candidate + 338, 30)
    g.put(candidate + 952, 1)  # cEquip+4 + E1BB0 affinity index.
    g.put(candidate + 832, 30)
    g.run(0x57F468, detail, candidate + 24, weapon, candidate, preview, 0xc000, 0, 0)
    assert labels[detail, 17, 3] == NAME
    assert (numbers[2, 2], numbers[2, 28]) == (320, 0), numbers
    assert not visible[2, 5] and not visible[2, 6]
    assert not visible[2, 9] and not visible[2, 10] and widths[2, 18] == 160

    g.put(candidate + 338, 0)
    g.run(0x57F468, detail, candidate + 24, weapon, candidate, preview, 0xc000, 0, 0)
    assert numbers[2, 28] == 30  # A genuine skill result can modify affinity.
    assert not clears  # Updating the values keeps the GU page and comparison widgets.
    print("GU equipment details: identity, stats, comparison and Handicraft markers passed")


def verify_equipment(g, api):
    register, base, level = (g.get(api + offset) for offset in (20, 24, 28))
    # A second model of the same class, with different stats at Lv.1 and Lv.2.
    other, rows, name = g.alloc(12), g.alloc(24), g.alloc(16)
    g.cpu.mem_write(name, b"Second spike\0")
    g.cpu.mem_write(other, struct.pack("<HH5BxH", 0x1002, 16, 4, 1, 6, 1, 0, 0))
    row = struct.Struct("<3hBb3Bx")
    g.cpu.mem_write(rows, row.pack(75, 12, 7, 5, -15, 1, 10, 0)
                    + row.pack(150, 20, 14, 8, 25, 2, 10, 2))
    assert g.run(register, other, rows, 2, name, name, name, 0) == 1
    assert not g.run(base, 0x1002)
    assert g.run(register, other, rows, 2, name, name, 0, 0) == 0
    assert g.run(register, other, rows, 2, name, name, 0, 0) == 2
    assert not g.run(level, 0x1002, 2)
    assert g.half(g.run(level, 0x1002, 1)) == 150 and g.half(g.run(level, 0x1001, 0)) == 100

    manager, actor, part = g.alloc(40032), g.alloc(14000), g.alloc(5152)
    eq = manager + 124
    g.put(0x1885B58, manager)
    g.cpu.hook_del(g.traps.pop(0x275A30))
    g.put(manager + 28, actor)
    g.put(actor + 5060, eq)
    g.put(part + 48, actor)
    g.stub(0x3F8398, lambda *_: 0)

    def equip(id, upgrade=0, jewels=None):
        saved = json.loads(character_save({}))
        saved["data"]["inventory"] = {"instances": {"1": {"equipment": id, "upgrade": upgrade}},
                                      "equipped": 1, "sets": {}}
        if jewels is not None:
            saved["data"]["inventory"]["decorations"] = {"1":jewels}
        g.files[SAVE] = json.dumps(saved).encode()
        select_character(g, 0)

    equip(0x1002, 1)
    assert g.run(g.hooks[0xDE16C], eq + 24) == 1
    assert not g.run(g.hooks[0xDE1C0], eq + 24)
    assert g.run(g.hooks[0xDE16C], g.alloc(44)) == 0
    # Execute the native DEE34 calculation with actual hooked property readers.
    # Raw stat rows and the fallback equipment slot are independent.
    g.cpu.mem_write(eq+28, bytes([7, 0, 1, 0]))
    fallback = bytes(g.cpu.mem_read(eq+24, 44))
    g.stub(0x13EFD7C, lambda dst, n, *_: g.cpu.mem_write(dst, bytes(n)))
    g.stub(0xDF824, lambda *_: 0)
    g.run(0xDEE34, eq+4)
    assert struct.unpack("<8h", g.cpu.mem_read(eq+336, 16)) == (150, 25, 2, 1, 6, 20, 14, 8)
    assert bytes(g.cpu.mem_read(eq+24, 44)) == fallback

    # The live sharpness colour uses this equipment's GU curve and changes
    # on the first point above the cumulative colour boundary.
    tables, curves, entries = g.alloc(256), g.alloc(112), g.alloc(20 * 11)
    g.put(0x1856B18, tables)
    g.put(tables + 252, curves)
    g.put(curves + 100, 11)
    g.put(curves + 108, entries)
    g.cpu.mem_write(entries + 204, struct.pack("<7H", 20, 40, 90, 150, 240, 320, 450))
    assert [g.run(g.hooks[0x29CDFC], actor, n) for n in (20, 21, 150, 151)] == [0, 1, 3, 4]

    equip(0x1001, 7)  # Load a saved Lv.8 trial instance through the equipment hook.
    g.run(0xDEE34, eq + 4)
    assert struct.unpack("<8h", g.cpu.mem_read(eq + 336, 16)) == (320, 0, 10, 0, 0, 0, 0, 0)
    # Run GU's DDD48 skill tally on the transient weapon row with two installed
    # jewels. The native fallback has a different jewel and must keep it.
    equip(0x1001,7,[2638,2639,0])
    table, entries, data = (g.alloc(n) for n in (56,112,36))
    g.put(0x18583A8,table)
    g.put(table+44,entries)
    g.put(entries+100,3)
    g.put(entries+108,data)
    for i,(cost,points) in enumerate([(2,3),(1,1),(1,7)]):
        g.cpu.mem_write(data+12*i+4,bytes([cost,11,0,points,0]))
    for part_index in range(7):
        g.put(eq+64+44*part_index,g.alloc(44))
    skills=g.get(eq+64)
    g.put(eq+396,g.alloc(1100))
    skill_manager = g.alloc(324)
    g.put(0x1896c20,skill_manager)
    g.stub(0x4CF018,lambda *_: 0)
    g.cpu.mem_write(eq+34,struct.pack("<3H",2640,0,0))
    original_record = bytes(g.cpu.mem_read(eq+24,44))
    g.cpu.hook_del(g.traps.pop(0xDF824))
    g.run(0xDEE34,eq+4)
    assert g.half(skills)==11 and g.cpu.mem_read(skills+2,1)==bytes([4])
    assert bytes(g.cpu.mem_read(eq+24,44))==original_record
    # GU E1EEC applies Handicraft to the level derived by DEE34, then caps at 12.
    for skill, expected in ((0, 10), (84, 11), (83, 12)):
        words = g.get(eq + 396) + 988
        g.cpu.mem_write(words, bytes(40))
        if skill:
            g.put(words + 4 * (skill // 32), 1 << (skill % 32))
        g.run(0xE1EEC, eq + 4)
        assert g.run(0xE173C, eq + 4, 14) == expected
    assert g.run(0xDB7E0, eq + 24) == 320
    assert g.run(0xDB028, eq + 24) == 8
    saved_slot = json.loads(g.files[SAVE])
    saved_slot["slot"] = 1
    saved_slot["data"]["inventory"]["decorations"]["1"] = [2639, 0, 0]
    saved_slot["data"]["inventory"]["instances"]["1"]["upgrade"] = 3
    g.files[SAVE.replace("-0.", "-1.")] = json.dumps(saved_slot).encode()
    preview_eq, select_manager, file_data = g.alloc(1376), g.alloc(200), g.alloc(4096)
    g.put(select_manager + 40, file_data)
    for slot in (0, 1, 2):
        g.put(select_manager + 44 + slot * 4, 1)
        g.cpu.mem_write(file_data + 4 + slot, bytes([1]))
        g.put(file_data + 16 + 4 * slot, 128)
    g.stub(0x13EFDD0, lambda dst, src, n, *_: g.cpu.mem_write(dst, bytes(g.cpu.mem_read(src, n))))
    native_weapon = file_data + 128 + 268
    g.cpu.mem_write(native_weapon + 4, struct.pack("<HH", 7, 1))
    g.cpu.mem_write(native_weapon + 10, struct.pack("<3H", 2640, 0, 0))
    candidate_points = g.alloc(44)
    g.put(manager + 12508 + 64, candidate_points)
    # A real title slot read, its aggregate copy, and the native weapon getters.
    for slot, level, power in ((1, 4, 170), (0, 8, 320), (2, 1, 0)):
        assert g.run(0x3E1208, select_manager, slot, preview_eq)
        g.run(0x274488, manager, preview_eq)
        target = manager + 12508
        assert g.run(0xDB028, target + 24) == level
        assert g.run(0xDB7E0, target + 24) == power
        assert g.run(0xDB568, target + 24) == (2 if slot == 0 else 1)
        if slot == 1:
            assert g.run(0xDB7B8, target + 24, 0) == data + 12
            assert g.run(0xDB66C, target + 24) == 0  # Lv.4 has one socket.
        g.run(0xDDD48, target + 24)
        assert g.half(candidate_points) == 11
        assert g.cpu.mem_read(candidate_points + 2, 1)[0] == {0: 4, 1: 1, 2: 7}[slot]
        assert g.run(0xDB028, eq + 24) == 8  # Hover never switches the active save.
    g.run(0x3E1208, select_manager, 1, preview_eq)
    # A copy made from another copy retains the viewed slot, even after the
    # source changes. Replacing an armour part must preserve weapon identity.
    nested = g.alloc(1376)
    g.run(0x26E600, nested, preview_eq)
    g.run(0xDADF0, nested + 68, file_data + 440)
    assert g.run(0xDB028, nested + 24) == 4 and g.run(0xDB7E0, nested + 24) == 170
    g.run(0xDADF0, preview_eq + 24, file_data + 396)
    assert g.run(0xDB7E0, preview_eq + 24) == 0
    assert g.run(0xDB028, nested + 24) == 4
    sent = []
    def capture(_, equipment, flags, target):
        sent.append((flags, struct.unpack("<3H", g.cpu.mem_read(equipment + 34, 6)), target))
        return 1
    sender = g.originals[0x202764]
    g.stub(sender, capture)
    assert g.run(0x202764, 0, nested, 1, 2) == 1
    assert sent[-1] == (0x98000001 | (3 << 17) | (0x1001 << 1), (2639, 0, 0), 2)
    assert g.half(nested + 34) == 2640
    g.run(0x26E600, nested, preview_eq)
    assert g.run(0xDB028, nested + 24) == 1
    assert g.run(0x202764, 0, nested, 1, 2) == 1
    assert sent[-1] == (1, (2640, 0, 0), 2)
    g.cpu.hook_del(g.traps.pop(sender))
    g.put(eq+396,0)
    g.cpu.mem_write(eq+34,bytes(6))
    equip(0x1001)
    resources, requests = {}, []
    def load(_, kind, path, flags):
        key = (kind, g.string(path))
        requests.append(key)
        obj = g.alloc(560)
        g.put(obj+80, 1)
        resources[key] = obj
        return obj
    mgr, vtable, loader = g.alloc(4), g.alloc(56), g.alloc(4)
    g.put(0x2126A64, mgr)
    g.put(mgr, vtable)
    g.put(vtable + 48, loader)
    g.stub(loader, load)
    binds = []
    vt, bind = g.alloc(96), g.alloc(4)
    g.put(part, vt)
    g.put(vt+88, bind)
    g.stub(bind, lambda owner, mesh, *_: binds.append((owner, mesh)) or g.put(owner + 240, mesh))
    g.stub(0x13EFF20, lambda dst, n, value, *_: g.cpu.mem_write(dst, bytes([value & 255]) * n))
    g.stub(0x957480, lambda *_: 0)
    g.stub(0x957400, lambda *_: 0)
    args = g.alloc(40)
    stock = g.alloc(560)
    g.put(args+16, stock)
    for _ in range(2):
        g.run(g.hooks[0x307F18], part, args)
    assert requests == [(0x2124DF0, r"arc\frontier\magnet_spike"),
                        (0x21251C8, r"weapon\frontier\magnet_spike\wk451")]
    assert binds == [(part, resources[requests[1]])] * 2 and g.get(args+16) == stock
    assert g.half(part + 5080) == 0
    equip(0x1002)
    g.run(g.hooks[0x307F18], part, args)
    assert binds[-1] == (part, stock)

    # Hold the native fallback constant while hovering a different character.
    title_actor = g.alloc(14000)
    title = manager + 12508
    g.put(manager + 44, title_actor)
    g.put(title_actor + 5060, title)
    g.put(part + 48, title_actor)
    for slot, expected_mesh in ((1, resources[requests[1]]), (2, stock)):
        assert g.run(0x3E1208, select_manager, slot, preview_eq)
        g.run(0x274488, manager, preview_eq)
        g.run(0x307F18, part, args)
        assert binds[-1] == (part, expected_mesh)
    g.put(manager + 44, 0)
    g.put(part + 48, actor)

    def native_copy(out, inp, *_):
        g.cpu.mem_write(out, bytes(g.cpu.mem_read(inp, 1376)))

    g.stub(0x26E600, native_copy)
    g.stub(0x26FDE8, lambda equipment, ready, *_: g.put(equipment + 1288, ready))
    g.stub(0x26FDDC, lambda equipment, *_: g.get(equipment + 1288))
    g.stub(0x13EFE3C, lambda out, inp, n, *_: g.cpu.mem_write(out, bytes(g.cpu.mem_read(inp, n))))
    for addr in (0x26E580, 0x4D31E8, 0x4D399C, 0x26FC24, 0xE174C, 0x561F44, 0x2861C4):
        g.stub(addr, lambda *_: 0)
    peer, packet = g.alloc(14000), g.alloc(1472)
    peer_eq = manager + 7004 + 1376
    g.put(0x18843F0, g.alloc(0x16280))
    g.put(manager + 32, peer)
    g.put(peer + 5060, peer_eq)
    g.put(part + 48, peer)
    g.stub(0x22D8B4, lambda _, slot, *_rest: slot == 1)
    g.put(packet + 1400, 0x98000000 | (0x1001 << 1))
    g.run(0x202894, 0, packet, 1)
    g.run(g.hooks[0x307F18], part, args)
    assert binds[-1] == (part, resources[requests[1]])
    g.put(packet + 1400, 0)
    g.run(0x202894, 0, packet, 1)
    g.run(g.hooks[0x307F18], part, args)
    assert binds[-1] == (part, stock)

    equip(0x1001)
    guild, preview, mannequin = g.alloc(7000), g.alloc(96), g.alloc(14000)
    card = guild + 32
    g.put(0x1857668, guild)
    g.put(preview + 24, card)
    g.put(preview + 28, eq)
    g.put(manager + 48, mannequin)
    g.put(mannequin + 5060, manager + 13884)
    g.put(mannequin + 5064, 4)
    g.put(part + 48, mannequin)
    g.stub(0x166A30, lambda _, viewed, own, *_rest: viewed == own)
    g.stub(0x1668F0, lambda *_: 0)
    g.stub(0x27632C, lambda *_: mannequin)
    g.stub(0x28857C, lambda *_: 0)
    # An older own card can have its appearance locked before the SDK
    # captured a snapshot. Its native weapon matches the live fallback.
    g.cpu.mem_write(card + 2232, b"\x01")
    g.cpu.mem_write(card + 84, struct.pack("<3H", 7, 1, 0))
    g.run(0x15FADC, preview)
    g.run(g.hooks[0x307F18], part, args)
    assert binds[-1] == (part, resources[requests[1]])
    g.put(mannequin + 9120, part)
    g.put(mannequin + 5068, 0x11)
    seen = []
    g.stub(g.originals[0x303790], lambda *_: seen.append(g.get(mannequin + 5068)))
    g.run(0x303790, part)
    assert seen == [1] and g.get(mannequin + 5068) == 0x11
    # Cold open: GU binds the fallback while the extension mesh is pending.
    # The card must replace it on its next update once load flags become ready.
    ready = resources[requests[1]]
    g.put(ready + 80, 0)
    g.run(g.hooks[0x307F18], part, args)
    assert g.get(part + 240) == stock
    g.run(0x303790, part)
    assert g.get(part + 240) == stock
    g.put(ready + 80, 1)
    g.run(0x303790, part)
    assert g.get(part + 240) == ready
    # A copy received from another hunter resolves its stamped weapon even
    # though the native identity comparison rejects it as the local card.
    copy = g.alloc(6328)
    g.cpu.mem_write(copy, bytes(g.cpu.mem_read(card, 6328)))
    for offset, byte in ((1, b"F"), (5, b"Z"), (43, b"W")):
        g.cpu.mem_write(copy + 84 + offset, byte)
    g.cpu.mem_write(copy + 90, struct.pack("<H", 0x1001))
    g.put(preview + 24, copy)
    g.run(0x15FADC, preview)
    g.run(g.hooks[0x307F18], part, args)
    assert binds[-1] == (part, ready)
    seen.clear()
    g.run(0x2744BC, manager, 0, eq)
    g.run(g.hooks[0x307F18], part, args)
    assert binds[-1] == (part, stock)
    g.run(0x303790, part)
    assert seen == [0x11]



def verify_action_tables():
    # Digests of the original PC w13 descriptor counts/payloads at mhfdat
    # +0x614 and the executable motion FX rows at +0x618, in source order.
    assets = ROOT / "target/assets"
    blob = (assets / "magnet_spike.fmsa").read_bytes()
    digest = hashlib.sha256()
    counts = [0, 0, 0]
    commands, conditions = set(), set()
    for state in range(165):
        for field, width in enumerate((12, 40, 12)):
            count, offset = struct.unpack_from("<II", blob, 16 + state * 24 + field * 8)
            digest.update(struct.pack("<I", count))
            digest.update(blob[offset:offset + count * width])
            counts[field] += count
            if field == 1:
                for pos in range(offset, offset + 40 * count, 40):
                    condition, command = struct.unpack_from("<HH", blob, pos + 2)
                    commands.add(command)
                    conditions.add(condition)
    assert counts == [244, 1784, 598]
    assert commands == {0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 13, 15, 17, 19, 20, 23, 24}
    assert conditions == {0, 1, 6, 8, 10, 11, 12}
    assert digest.hexdigest() == "fbd22c2071a3891d8b7a0f107210ae3ccea694b8f387e18acf811d7fc2b4a3c5"
    rows = (assets / "frontier_ms_motion_fx.bin").read_bytes()
    assert hashlib.sha256(rows).hexdigest() == "4f588b8bc6b926d2fe8ed3286dca86675bff98ac9e8aa30e353bd50e9e455a44"
    routes = list(struct.iter_unpack("<HH", (assets / "frontier_ms_routes.bin").read_bytes()))
    keys = {(bank, selector): route for bank, selector, route in
            struct.iter_unpack("<HHH", (assets / "frontier_ms_selectors.bin").read_bytes())}
    seen = set()
    for clip, _, kind, bank, selector, *_ in struct.iter_unpack("<5H2B3h", rows):
        seen.add(clip)
        if kind == 1:
            assert all(routes[i][1] for i in (46, 47, 48))
        else:
            assert routes[keys[bank, selector]][1]
            if kind == 5:
                assert routes[keys[bank + 1, selector]][1]
    assert len(rows) == 962 * 18 and len(seen) == 167
    plugin = PLUGIN.read_bytes()
    assert rows in plugin and blob in plugin


def verify_turn_table():
    data = (ROOT / "target/assets/magnet_spike.fmsa").read_bytes()
    assert data[:8] == struct.pack("<4sHH", b"FMSA", 7, 165)
    assert b"MSBR" not in data
    end = data.index(b"MSTR")
    caps = struct.unpack_from("<165h", data, end + 4)
    assert {i for i, cap in enumerate(caps) if not cap} == {98, 99, 106, 107, 108, 116, 117, 158}
    assert {i: cap for i, cap in enumerate(caps) if cap > 0} == {
        95: 10920, 96: 10920, 110: 5460, 111: 5460, 114: 5460, 115: 5460,
        159: 10920, 160: 10920, 161: 10920, 162: 10920,
    }
    assert data[end + 334:end + 338] == b"MSCT"


if __name__ == "__main__":
    verify_room_icons()
    verify_status_panels()
    verify_action_tables()
    verify_turn_table()
    verify(sdk_last=False)
    verify(sdk_last=True)
    print("Magnet Spike NRO: icons, native trade, stats, skills, pause manual and model binding passed")
