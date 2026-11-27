"""Buy and forge the actual packaged Magnet Spike through the GU facility hooks."""

import hashlib
import json
import struct
import sys
from pathlib import Path

from unicorn import UC_HOOK_CODE

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


def verify_manual(g):
    panel, dialog = g.alloc(100), g.alloc(128)
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
    for move in ("磁界瞄准", "磁界弹", "磁界接近", "磁界脱离", "垂直下落斩",
                 "垂直下落挥击", "紧急着地"):
        assert move in guide[0], move
    g.stub(0x56E808, lambda _, group, part, body: guide.append(g.string(body)))
    g.put(pressed, 0x20)
    g.run(g.hooks[0x5CB780], panel)
    assert len(guide) == 2 and "以打击形态拔刀并精准防御" in guide[1]
    for move in ("防御反击", "防御强袭", "磁力二连斩", "磁力二连挥击", "后撤斩", "磁冲击", "磁力强袭"):
        assert move in guide[1], move
    rows = guide[1].splitlines()
    assert any("<ICON PAD_A>" in row and "向右使出磁力回避" in row for row in rows)
    assert any("<ICON PAD_Y>" in row and "向左使出磁力回避" in row for row in rows)
    assert any("<ICON PAD_X>" in row and "<ICON PAD_A>" in row
               and "回避成功时武器闪光并强化，产生追加打击" in row for row in rows)
    g.run(g.hooks[0x5CB780], panel)
    assert len(guide) == 3 and "显示剩余磁力" in guide[2]
    assert "缩短强走药与强走旋律的剩余时间" in guide[2]
    assert "会心和蓄力技能影响单次回充量" in guide[2]
    assert "标记在磁界脱离的距离内" in guide[2] and "标记在磁界接近的距离内" in guide[2]
    assert "首次接触硬部位时按当前形态弹刀" in guide[2]
    assert "右摇杆（R3）" in guide[2] and "切换原版锁定目标" in guide[2]
    g.run(g.hooks[0x5CB780], panel)
    assert len(guide) == 4 and "左侧是斩击磁气，右侧是打击磁气" in guide[3]
    assert "当前形态动作值提高 10%" in guide[3]
    g.run(g.hooks[0x5CB780], panel)
    assert len(guide) == 5 and "消耗一格磁力，派生空中起手" in guide[4]
    assert "<ICON PAD_X>" in guide[4] and "<ICON PAD_A>" in guide[4]
    assert "可麻痹的怪物" in guide[4] and "每次磁缚都需积蓄1800" in guide[4]
    assert "碰到可束缚的怪物时触发磁缚" in guide[4]
    assert "游标随束缚计时移动，达标后变色" in guide[4]
    assert "延长束缚，积累终结资格" in guide[4] and "使出磁缚终结" in guide[4]
    assert "使自己束缚的地面目标倒下" in guide[4]
    g.run(g.hooks[0x5CB780], panel)
    assert len(guide) == 6 and all(word in guide[5] for word in ("拔刀冲刺", "强化砸击", "落地", "受身"))
    g.run(g.hooks[0x5CB780], panel)
    assert len(guide) == 7 and "回复药×1" in guide[6] and "Lv.8" in guide[6]
    g.stub(0x64C600, lambda *_: 0)
    g.put(pressed, 0x4000)
    g.run(g.hooks[0x5CB780], panel)
    g.put(pressed, 0)


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
    for i, (attack, slots, curve, sharpness) in enumerate(progression):
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
    shop_viewer = g.alloc(160)
    g.put(shop.owner + 2016, shop_viewer)
    shop.open("磁斩锤", ICON)
    assert (g.get(shop_viewer + 72), g.get(shop_viewer + 76), g.get(shop_viewer + 60)) == (7, 1, 1)
    g.put(g.get(0x188C670) + 27636, 0x10000)
    shop.press()
    assert g.get(shop_viewer + 88) == 1
    assert values[2, 2] == 100 and values[2, 28] == 0
    assert NAME in shop.rows[0] and g.get(shop.header+24)==100
    assert g.get(shop.header + 16) == rarity_color and g.half(shop.header + 20) == ICON
    shop.press(A)
    shop.press(A)
    assert g.get(wallet + 36) == 100 and g.half(pouch + 28) == 2
    assert (0, 10) in shop.sounds
    shop.answer_equip(True)
    assert g.run(instance, 0, out) and g.get(out) == 1

    smith = Facility(g, True)
    smith.connect()
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


def verify_icon(g):
    tex = (PLUGIN.parents[1] / "GUI/frontier_magnet_spike/icon.tex").read_bytes()
    assert tex[:4] == b"TEX\0" and tex[13] == 7
    assert struct.unpack_from("<I", tex, 8)[0] == 64 << 19 | 64 << 6 | 1
    assert any(tex[27::4]) and not all(tex[27::4])

    panel, layout, groups, group, nodes, sprite = [
        g.alloc(size) for size in (96, 680, 28, 240, 48, 300)
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

    def equip(id, upgrade=0):
        saved = json.loads(character_save({}))
        saved["data"]["inventory"] = {"instances": {"1": {"equipment": id, "upgrade": upgrade}},
                                      "equipped": 1, "sets": {}}
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
    g.run(g.hooks[0xDEE34], eq+4)
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
    g.run(g.hooks[0xDEE34], eq + 4)
    assert struct.unpack("<8h", g.cpu.mem_read(eq + 336, 16)) == (320, 0, 10, 0, 0, 0, 0, 0)
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
    g.stub(bind, lambda owner, mesh, *_: binds.append((owner, mesh)))
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
    g.run(0x2744BC, manager, 0, eq)
    g.run(g.hooks[0x307F18], part, args)
    assert binds[-1] == (part, stock)
    g.run(0x303790, part)
    assert seen == [1, 0x11]



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
    verify_action_tables()
    verify_turn_table()
    verify(sdk_last=False)
    verify(sdk_last=True)
    print("Magnet Spike NRO: icons, native trade, stats, skills, pause manual and model binding passed")
