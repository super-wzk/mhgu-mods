"""Replay Extreme Gunlance input, ammunition, heat and presentation on GU 1.4."""
import struct
import sys
import math
import zlib
from pathlib import Path
from unicorn.arm_const import UC_ARM_REG_S0, UC_ARM_REG_SP

ROOT = Path(__file__).resolve().parents[1]
REPO = ROOT.parents[1]
NX = ROOT / "dist/gunlance-extreme/romfs/nativeNX"
sys.path.insert(0, str(REPO / "plugins/great-sword-extreme/tests"))
from verify_native_fx import emit_model_layer, model_renderer
from verify_style import (CHANGE_ACTION, DISPATCH, WEAPON_DISPATCH, api_for, boot,
                          hunters, load_registered_style, quest_arc, quest_entries, setup_controller)

ADD_HEAT = 0x11AA480
UPDATE_HEAT = 0x11AA34C
HEAT_HUD = 0x6487CC
NATIVE_HEAT_FLAGS = 0x11A9C98
OVERHEAT_MATERIAL = 0x312728
OVERHEAT_PARTS = 0x312DA0
DRAW = 0x11A9D08
SHOT = 0x11B0544
WYVERN = 0x11B086C
AA_WYVERN = 0x11B0244
VTABLE = 0x174E110


def real(value):
    return struct.unpack("<I", struct.pack("<f", value))[0]


def memory_float(g, addr):
    return struct.unpack("<f", g.cpu.mem_read(addr, 4))[0]


def floats(g, address, values):
    g.cpu.mem_write(address, struct.pack(f"<{len(values)}f", *values))


def efl_model_offset(data):
    return 48 + (struct.unpack_from("<I", data, 52)[0] >> 8)


def blade_point(tip, multiplier, accessory=False):
    x, y, z = (105, 15, 167) if accessory else (128, 25, 201)
    pitch, yaw = math.radians(-5), .5462880730628967
    sin_pitch, cos_pitch = math.sin(pitch), math.cos(pitch)
    sin_yaw, cos_yaw = math.sin(yaw), math.cos(yaw)
    # Undo the source X rotation followed by Y (8100433E).
    x, z = cos_yaw * x - sin_yaw * z, sin_yaw * x + cos_yaw * z
    y, z = cos_pitch * y + sin_pitch * z, -sin_pitch * y + cos_pitch * z
    source_tip = (128 * sin_yaw + 201 * cos_yaw) * cos_pitch - 25 * sin_pitch
    return tuple(v * multiplier * tip / source_tip for v in (x, y, z))


def scene(selected=True):
    config = b"[Weapons]\n9=15\n" if selected else b""
    g = boot((REPO / "mhgu/main.elf").read_bytes(), config, ("registry",))
    g.inline_hooks |= {ADD_HEAT, UPDATE_HEAT, HEAT_HUD, DRAW, AA_WYVERN}
    desc = load_registered_style(g, NX / "plugins/gunlance_extreme.nro")
    a, eq, remote = hunters(g)
    g.cpu.mem_write(eq + 1236, b"\x09")
    for hunter in (a, remote):
        g.put(hunter, VTABLE)
    return g, a, eq, remote, desc


def equip_blade(g, actor):
    path = quest_arc("w09.arc").parents[2] / "weapon/w09/001.arc"
    mod = next(zlib.decompress(raw) for name, kind, raw in quest_entries(path)
               if kind == 0x58A15856 and name.endswith(b"gun001\\gun001"))
    count, = struct.unpack_from("<H", mod, 6)
    bones, = struct.unpack_from("<I", mod, 0x24)
    inverse_binds = bones + count * (24 + 64)
    remap_at = inverse_binds + count * 64
    lo, hi = (struct.unpack_from("<3f", mod, offset) for offset in (0x50, 0x60))
    weapon, model, matrices, remap, joints = (
        g.alloc(size) for size in (5152, 544, count * 64, 256, count * 160)
    )
    g.put(actor + 9120, weapon)
    g.put(weapon, 0x1752088)
    g.put(weapon + 12, 1)
    floats(g, weapon + 96, (1, 1, 1))
    g.put(weapon + 240, model)
    g.put(weapon + 1172, joints)
    g.put(weapon + 1176, remap)
    g.put(model + 104, count)
    g.put(model + 112, matrices)
    g.put(model + 168, real(max(b - a for a, b in zip(lo, hi))))
    floats(g, model + 192, lo)
    floats(g, model + 208, hi)
    g.cpu.mem_write(matrices, mod[inverse_binds:remap_at])
    g.cpu.mem_write(remap, mod[remap_at:remap_at + 256])
    # Point the native variant lookup at a configuration for barrel joint 1.
    data, table, rows = (g.alloc(size) for size in (432, 112, 40))
    g.put(0x1856B18, data)
    g.put(data + 388 + 4 * 9, table)
    g.put(table + 100, 2)
    g.put(table + 108, rows)
    g.put(weapon + 4992, 1)
    g.cpu.mem_write(rows + 20 + 17, b"\x02")
    g.cpu.mem_write(rows + 20 + 18, b"\x03")
    floats(g, 0x1890250 + 16 * 3, (7., -5., 72.))
    g.put(0x188FF10 + 32 * 2, 1)
    bone = mod[remap_at + 1]
    pivot = struct.unpack_from("<3f", mod, bones + 24 * bone + 12)
    return weapon, joints + 160 * bone + 16, pivot, hi[2]


def verify_heat():
    g, a, eq, remote, _ = scene()
    native_add, timers = [], []
    g.stub(g.originals[ADD_HEAT], lambda *args: native_add.append(args[:3]) or 13)
    g.stub(0x282850, lambda *_: 0)
    g.stub(0x282870, lambda *_: 0)

    def timer(actor, field, *_):
        assert actor == a
        timers.append(field - a)
        current = memory_float(g, field)
        g.put(field, real(max(0, current - 2)))
        return 0

    g.stub(0x53AA84, timer)
    g.put(a + 13096, real(120))
    for offset in (13152, 13164, 13168):
        g.put(a + offset, real(10))
    assert g.run(ADD_HEAT, a, 30, 1) == 0 and not native_add
    assert g.run(ADD_HEAT, remote, 30, 1) == 13
    g.run(UPDATE_HEAT, a)
    assert timers == [13096, 13168, 13160]
    assert memory_float(g, a + 13096) == 118
    assert all(g.get(a + offset) == 0 for offset in (13152, 13164, 13168))

    for route in (0, 1, 3):
        row = g.run(DRAW, a, route)
        assert struct.unpack("<3H", g.cpu.mem_read(row, 6)) == (16, 0x8014, 0)
    for route in (2, 4, 5, 6, 7):
        assert g.run(DRAW, a, route) == g.run(g.originals[DRAW], a, route)
    assert g.run(DRAW, remote, 3) == g.run(g.originals[DRAW], remote, 3)

    g.cpu.mem_write(eq + 1236, b"\x08")
    assert g.run(ADD_HEAT, a, 30, 1) == 13
    assert g.run(DRAW, a, 1) == g.run(g.originals[DRAW], a, 1)
    stock, actor, _, _, _ = scene(False)
    stock.stub(stock.originals[ADD_HEAT], lambda *_: 77)
    assert stock.run(ADD_HEAT, actor, 40, 1) == 77
    print("Extreme heat isolation, native cooldown and draw route passed")


def verify_ignition_hud():
    assert not (NX / "GUI/frontier_gl/ignition.tex").exists()
    g, a, equipment, peer, desc = scene()
    set_action = setup_controller(g, a)
    tick_style, inputs = g.get(desc + 20), g.alloc(40)
    g.put(a + 240, g.alloc(128))
    g.put(a + 28, real(2))
    g.stub(0x53A058, lambda *_: 5001)
    g.stub(g.get(VTABLE + 508), lambda *_: 4)

    widget, layout, groups, gauge, root, members, marker, ammo = (
        g.alloc(size) for size in (196, 720, 12, 240, 192, 16, 240, 240)
    )
    red, orange, fill = (g.alloc(256) for _ in range(3))
    full_width, height = 112., 16.  # hud9_g_lance's polygon size before segmentation.
    g.put(widget + 36, layout)
    g.put(layout + 664, 3)
    g.put(layout + 248, groups)
    g.cpu.mem_write(groups, struct.pack("<3I", ammo, gauge, marker))
    g.put(gauge + 216, members)
    g.put(gauge + 220, root)
    g.cpu.mem_write(members, struct.pack("<4I", root, red, orange, fill))
    g.put(root + 104, red)
    g.put(red + 108, orange)
    g.put(orange + 108, fill)
    for node in (red, orange, fill):
        g.put(node, 0x176C744)
        g.put(node + 128, real(8))
        g.cpu.mem_write(node + 180, struct.pack("<2f", full_width, height))
    g.stub(g.get(0x176C744 + 80), lambda *_: 0)
    stock_colors = (0xE014FFFF, 0xD014FFFF, 0xC014FFFF, 0xB014FFFF)
    g.cpu.mem_write(fill + 164, struct.pack("<4I", *stock_colors))
    for node in (ammo, gauge, red, orange, fill, marker):
        g.put(node + 84, 0x02060003)

    # GU caches the full width before splitting it by the native shelling thresholds.
    g.put(widget + 132, g.alloc(12))
    g.put(widget + 140, 3)
    g.put(widget + 152, g.alloc(12))
    g.put(marker + 144, real(10))
    g.put(a + 13156, 300)
    g.stub(g.get(VTABLE + 2076), lambda *_: 0x16AEF5C)  # Normal shelling row.
    for address in (0x657894, 0x56E528, 0x547288, 0x56E4E4, 0x56E530):
        g.stub(address, lambda *_: 0)
    g.run(0x6485F0, widget)
    assert g.get(widget + 100) == full_width
    native_size = (memory_float(g, fill + 180), height)
    assert math.isclose(native_size[0], full_width * 78 / 300, abs_tol=0.001)
    edge = memory_float(g, red + 128) + memory_float(g, red + 180)
    assert math.isclose(edge, full_width + 8, abs_tol=0.001)

    # Run GU's real widget update. Stub the text and animation sinks downstream.
    counts = []

    def update_ammo(widget_arg, group, child, count):
        assert (widget_arg, group, child) == (widget, 0, 3)
        assert struct.unpack("<2f", g.cpu.mem_read(fill + 180, 8)) == native_size
        assert struct.unpack("<4I", g.cpu.mem_read(fill + 164, 16)) == stock_colors
        assert all(g.get(node + 84) & 3 == 3 for node in (gauge, red, orange, fill, marker))
        counts.append(count)

    g.stub(0x56FDE0, update_ammo)
    for address in (0x560E90, 0x56E6FC, 0x56E538, 0x545A40):
        g.stub(address, lambda *_: 0)

    # Both original model callbacks must choose the stock heated geometry.
    # Capture their motion / part submissions after the native flag checks.
    model = g.alloc(5200)
    g.put(a + 9120, model)
    g.put(peer + 9120, model)
    materials, parts = [], []

    def apply_material(model_arg, actor, channel, bank):
        assert model_arg == model
        if channel == 2:
            assert bank == 1
            sp = g.cpu.reg_read(UC_ARM_REG_SP)
            materials.append(g.get(sp))

    g.stub(0x308210, apply_material)
    g.stub(0x3082A8, lambda *_: 0)
    g.stub(0x3090CC, lambda model_arg, part, *_: parts.append(part))
    g.stub(0x13F614, lambda *_: 0)

    def barrel(actor, hot):
        timer = g.get(actor + 13096)
        g.put(actor + 10340, 0x200000)
        g.run(NATIVE_HEAT_FLAGS, actor)
        native_flags = 0x201000 if hot else 0x200000
        assert g.get(actor + 10340) == native_flags
        g.run(OVERHEAT_MATERIAL, model, actor)
        assert materials.pop() == (238 if hot else 239)
        assert g.get(actor + 10340) == native_flags
        parts.clear()
        g.run(OVERHEAT_PARTS, model, actor)
        assert (13 in parts) == hot
        assert g.get(actor + 10340) == native_flags and g.get(actor + 13096) == timer

    def draw(active, loaded=0, progress=1., native_heat=False):
        g.run(HEAT_HUD, widget)
        assert counts[-1] == loaded and g.get(ammo + 84) & 3 == 3
        assert all(g.get(node + 84) & 3 == 0 for node in (red, orange, marker))
        assert g.get(fill + 84) & 3 == (3 if progress else 0)
        width, drawn_height = struct.unpack("<2f", g.cpu.mem_read(fill + 180, 8))
        assert math.isclose(width, full_width * progress, abs_tol=0.001) and drawn_height == height
        assert memory_float(g, fill + 128) == 8
        rgb = 0x0A6EF5 if active else 0xF3A84C
        assert struct.unpack("<4I", g.cpu.mem_read(fill + 164, 16)) == tuple(
            (color & 0xFF000000) | rgb for color in stock_colors
        )
        barrel(a, native_heat)

    def tick(frame=0, end=False):
        g.put(a + 1280, real(frame))
        g.cpu.mem_write(a + 1206, bytes([4 if end else 0]))
        g.run(tick_style, a, inputs)

    def ignite():
        set_action(16, 0x801C)
        tick()
        tick(147, end=True)
        tick()
        tick(147, end=True)
        assert g.half(a + 1204) == 1428

    def try_ignite(frame=0):
        g.put(inputs, 1 << 8)
        tick(frame)
        g.put(inputs, 0)
        return g.half(a + 9482) - 0x8000

    g.cpu.mem_write(a + 11140, b"\x05")
    draw(False)
    g.put(a + 13096, real(60))
    draw(False, native_heat=True)  # Ready for ignition while Wyvern Fire cools.
    g.put(a + 13096, 0)
    g.run(0x2AD4F4, a, 5)
    draw(False, loaded=5)
    g.run(0x2AD4F4, a, -2 & 0xFFFFFFFF)
    draw(False, loaded=3)
    g.run(0x2AD4F4, a, -3 & 0xFFFFFFFF)
    set_action(16, 0x801C)
    tick()
    tick(100)
    draw(False)
    set_action(1, 0)  # An interrupted ignition leaves the full blue meter.
    tick()
    draw(False)
    ignite()
    draw(True)
    g.put(a + 13096, real(60))
    draw(True, native_heat=True)  # Wyvern Fire's cooldown is independent of ignition.
    g.put(a + 13096, 0)
    set_action(1, 0)
    g.cpu.mem_write(a + 10057, b"\0")  # Idle with the weapon sheathed.
    g.put(a + 28, real(3600))
    tick()
    draw(True, progress=0.5)
    tick()
    draw(False, progress=0.)
    barrel(peer, False)
    g.put(a + 28, real(0))
    assert try_ignite() == 80
    set_action(16, 0x8000)
    tick()
    assert try_ignite(36) == 80  # The buffered thrust branch also checks the cooldown.
    set_action(1, 0)
    g.put(a + 28, real(1200))
    tick()
    draw(False, progress=0.5)
    g.put(a + 13096, real(60))
    draw(False, progress=0.5, native_heat=True)
    g.put(a + 13096, 0)
    g.put(a + 28, real(1199))
    tick()
    draw(False, progress=2399 / 2400)
    g.put(a + 28, real(0))
    assert try_ignite() == 80
    set_action(1, 0)
    g.put(a + 13096, real(900))  # Ignition can recover while Wyvern Fire is cooling.
    g.put(a + 28, real(1))
    assert try_ignite() == 28
    draw(False, native_heat=True)
    g.put(a + 28, real(2))
    ignite()
    g.cpu.mem_write(equipment + 1236, b"\x08")
    g.run(HEAT_HUD, widget)
    assert all(g.get(node + 84) & 3 == 3 for node in (gauge, red, orange, fill, marker))
    assert struct.unpack("<2f", g.cpu.mem_read(fill + 180, 8)) == native_size
    assert struct.unpack("<4I", g.cpu.mem_read(fill + 164, 16)) == stock_colors
    barrel(a, True)
    g.put(a + 13096, 0)
    barrel(a, False)
    g.cpu.mem_write(equipment + 1236, b"\x09")
    set_action(1, 0)
    g.run(tick_style, 0, 0)  # Quest exit or controller deselection.
    tick()
    draw(False)
    print("Gunlance HUD and model: native ammo, ignition progress, red barrel, and native restore passed")


def verify_actions():
    g, a, equipment, peer, _ = scene()
    set_action = setup_controller(g, a)
    g.put(a + 240, g.alloc(128))
    g.put(a + 28, real(2))
    g.put(a + 5088, real(900))
    settings = g.alloc(17600)
    g.put(0x1897F78, settings)
    g.put(0x188C670, g.alloc(27800))
    for addr, value in ((0x53A058, 5001), (0x27F728, 0)):
        g.stub(addr, lambda *_, value=value: value)
    g.cpu.mem_write(a + 10057, b"\x01")
    g.cpu.mem_write(a + 11140, b"\x05")
    g.stub(g.get(VTABLE + 508), lambda *_: 4)
    aa_lift_flashes = []
    g.stub(0x11B0E94, lambda actor, *_: aa_lift_flashes.append(actor) or 1)
    for addr in (0x2A063C, 0x11B0C38, 0x2AD288, 0x282850, 0x282870):
        g.stub(addr, lambda *_: 0)
    g.cpu.mem_write(equipment + 800 + 2 * 28, struct.pack("<h", 3))

    attacks = []
    api = api_for(g, b"mhgu::StyleRegistryV3", b"SRG3")
    g.stub(g.get(api + 8), lambda _, hit, *__: attacks.append(
        struct.unpack("<HHffIII", g.cpu.mem_read(hit, 24))) or 1)
    def dispatch():
        if g.half(a + 9478) == 16:
            g.run(g.hooks[WEAPON_DISPATCH], a)
        else:
            g.run(g.hooks[DISPATCH], a)

    def tick(frame=0, end=False, command=0, common=0, next_frame=None):
        g.put(a + 1280, real(frame))
        g.put(a + 1276, real(frame if next_frame is None else next_frame))
        g.cpu.mem_write(a + 1206, bytes([4 if end else 0]))
        g.put(a + 5140, common)
        g.put(a + 5156, command)
        dispatch()

    def pad(held, pressed):
        g.cpu.mem_write(a + 5140, bytes(32))
        g.cpu.mem_write(a + 5108, struct.pack("<H2xH", held, pressed))
        g.run(0x27CA28, a)
        g.run(0x11AA60C, a)
        if pressed or g.get(a + 5140) or g.get(a + 5156):
            dispatch()

    def press(mask):
        g.put(a + 5188, 0)
        g.put(a + 1280, 0)
        g.cpu.mem_write(a + 1206, b"\0")
        for i in range(3):
            pad(mask, mask if i == 0 else 0)

    def state():
        return g.half(a + 9482) - 0x8000

    def start(slot):
        set_action(16, slot + 0x8000)
        tick()

    def ammo():
        return g.cpu.mem_read(a + 11138, 1)[0]

    def shots():
        return list(g.cpu.mem_read(a + 13100, 3))

    def clear_shots():
        g.cpu.mem_write(a + 13100, b"\0" * 3)

    g.run(0x2AD4F4, a, 5)
    set_action(1, 0)
    press(0x100)  # Native A resolves after the X+A input window.
    assert state() == 1 and ammo() == 4 and shots() == [1, 0, 0], (state(), ammo(), shots())
    assert g.get(a + 13116) == 3 and memory_float(g, a + 13140) == 1
    tick(1)
    assert ammo() == 4
    clear_shots()
    set_action(32, 0)
    press(0x140)  # GU emits weapon command 3 for R+A with no motion.
    assert state() == 5 and ammo() == 5
    for presses in ((0x140,), (0x40, 0x140), (0x100, 0x140)):
        start(1)
        clear_shots()
        g.cpu.mem_write(a + 5188, bytes(16))
        g.put(a + 1280, real(32))
        g.put(a + 1276, real(32))
        previous = 0
        for held in presses + (0x140,) * 2:
            pad(held, held & ~previous)
            previous = held
        assert state() == 39 and ammo() == 5, (presses, state(), ammo())
    g.run(0x2AD4F4, a, -4 & 0xFFFFFFFF)
    start(39)
    assert ammo() == 2
    tick(1)
    assert ammo() == 2
    start(30)
    tick(49)
    assert ammo() == 2 and shots() == [0, 0, 0]
    tick(50)
    assert ammo() == 0 and shots() == [1, 1, 0]
    clear_shots()
    start(3)
    tick(40, end=True)
    for _ in range(49):
        tick(24)
        assert state() == 3
    tick(24)
    assert state() == 59 and shots() == [0, 0, 0]
    tick(2)
    assert shots() == [1, 0, 0] and memory_float(g, a + 13096) == 2400
    clear_shots()
    tick(4)
    assert shots() == [0, 0, 0]
    set_action(1, 0)
    press(0x160)  # GU emits the shoulder-chord flag for R+X+A.
    assert state() == 19
    # With R held before X+A, GU instead emits the ordinary rising-slash bit.
    # The same input must reach Wyvern Fire from guard and from a thrust.
    for layout, shoulder in ((0, 0x40), (1, 0x800)):
        g.cpu.mem_write(settings + 17518, bytes([layout]))
        for start_from_guard, first_attack in ((True, 0), (False, 0), (False, 0x20), (False, 0x100)):
            if start_from_guard:
                set_action(32, 0)
            else:
                start(0)
            g.cpu.mem_write(a + 5188, bytes(16))
            g.put(a + 1280, real(36))
            g.put(a + 1276, real(36))
            pad(shoulder, shoulder)
            for _ in range(3):
                pad(shoulder, 0)
            if first_attack:
                pad(shoulder | first_attack, first_attack)
                assert state() == 0
            pad(shoulder | 0x120, 0x120 & ~first_attack)
            assert g.get(a + 5188) & 0x40 and not g.get(a + 5188) & 0x400
            assert state() == 19, (layout, start_from_guard, first_attack, state())
        # R+X starts guard thrust immediately. R+A has no branch here,
        # so the next sample can still complete Wyvern Fire in that case.
        for first, after_pair, expected in ((0x20, 17, 17), (0x100, 0, 19)):
            start(0)
            g.cpu.mem_write(a + 5188, bytes(16))
            g.put(a + 1280, real(36))
            g.put(a + 1276, real(36))
            pad(shoulder | first, shoulder | first)
            assert g.get(a + 5188) & 0x180 and state() == after_pair
            pad(shoulder | 0x120, 0x120 & ~first)
            assert state() == expected, (layout, first, state())
    g.cpu.mem_write(settings + 17518, b"\0")
    g.put(a + 13096, 0)
    tick(45, end=True)
    assert shots() == [0, 0, 0]
    set_action(1, 0)
    press(0x120)
    assert state() == 4
    set_action(32, 0)
    press(0x60)
    assert state() == 2
    set_action(1, 0)
    press(0x160)
    assert state() == 3
    tick(40, end=True)
    g.run(0x2AD4F4, a, 2)
    tick(24, command=1 << 2)
    assert state() == 47 and ammo() == 1
    tick(35, end=True)
    tick(24, command=1 << 2)
    assert state() == 47 and ammo() == 0
    tick(24, command=1 << 2)
    assert state() == 73
    tick(58, end=True)
    tick(29)
    assert shots() == [0, 0, 0]
    tick(30)
    assert shots() == [1, 0, 0] and abs(memory_float(g, a + 13096) - 3240) < .001
    clear_shots()
    g.put(a + 13096, 0)

    # FZ Heat Blade is an action-triggered buff, independent of GU's gauge.
    for layout, auxiliary in ((0, 0x800), (1, 0x40)):
        g.cpu.mem_write(settings + 17518, bytes([layout]))
        assert g.run(0x65FA04) == auxiliary
        set_action(1, 0)
        press(auxiliary)
        assert (g.half(a + 9478), g.half(a + 9482)) == (1, 0)
        press(0x1000)  # GU special attack / kick.
        assert state() == 28
    g.cpu.mem_write(settings + 17518, b"\0")
    start(0)
    tick(36)
    pad(0x1000, 0x1000)
    assert state() == 28 and shots() == [0, 0, 0]
    pad(0, 0)
    # Capture the PEL selectors and live joint proofs submitted by the NRO.
    parent, pel, handle, manager, vt, load = (g.alloc(128) for _ in range(6))
    for offset in (1172, 1176):
        g.put(a + offset, parent)
    g.stub(g.get(VTABLE + 304), lambda owner, *_: parent if owner == a else owner + 4048)
    g.stub(0xC1628, lambda *_: parent)
    g.put(0x188A0D4, manager)
    g.put(0x2126F48, manager)
    g.put(0x2126A64, manager)
    g.put(manager, vt)
    g.put(vt + 48, load)
    g.put(pel + 80, 1)
    sound_banks = {}
    for name in ("blade", "common", "weapon"):
        path = f"sound/se/pl/frontier_gl/{name}"
        data = (NX / f"{path}.srqr").read_bytes()
        count, = struct.unpack_from("<I", data, 8)
        rows = g.alloc(132 * count)
        g.cpu.mem_write(rows, data[52:52 + 132 * count])
        bank = g.alloc(176)
        g.put(bank + 80, 1)
        g.put(bank + 104, count)
        g.put(bank + 108, rows)
        sound_banks[path] = bank

    def load_resource(_, kind, path, __):
        if kind == 0x1889FDC:
            return pel
        if kind == 0x2125BEC:
            return sound_banks[g.string(path).replace("\\", "/")]
        return 0

    g.stub(load, load_resource)
    g.put(0x1896F5C, g.alloc(128))
    sounds = []
    g.stub(0x4EEAA4, lambda _, __, bank, cue: sounds.append((bank, cue - 4096)) or 1)
    g.stub(0x3268D8, lambda *_: handle)
    g.stub(0x326C78, lambda *_: 0)
    skip_alloc = []
    def allocate(*_):
        return 0 if skip_alloc and skip_alloc.pop() else g.alloc(416)
    g.stub(0x3288C4, allocate)
    stopped = []
    g.stub(0x32A400, lambda unit, *_: stopped.append(unit) or 0)
    visuals, unit_selectors, proofs = [], {}, {}

    def spawn(_, unit, *__):
        selector = g.get(g.cpu.reg_read(UC_ARM_REG_SP))
        unit_selectors[unit] = selector
        proofs[selector] = bytes(g.cpu.mem_read(unit + 128, 208))
        visuals.append((selector, g.half(unit + 194), g.get(unit + 320),
                        struct.unpack("<3f", g.cpu.mem_read(unit + 144, 12))))
        return 1

    g.stub(0x3267D0, spawn)
    routes = list(struct.iter_unpack("<2H", (ROOT / "target/assets/effects/routes.bin").read_bytes()))

    def blade_selectors(index):
        return [selector for first, count in routes[38 + index * 2:40 + index * 2]
                for selector in range(first, first + count)]

    weapon, blade_matrix, pivot, tip = equip_blade(g, a)
    weapon_parent = weapon + 4048  # GU's 6BE88 returns the weapon's particle parent.
    remap = g.get(weapon + 1176)
    for cycle in range(2):
        tick(6)
        tick(16)
        tick(99)
        assert not visuals
        tick(100)
        assert [(s, j, p) for s, j, p, _ in visuals] == [
            (s, 1, weapon_parent) for s in blade_selectors(5)
        ], (visuals, hex(weapon_parent))
        for i, (_, _, _, offset) in enumerate(visuals):
            point = blade_point(tip, (.86, .98, 1.)[min(i, 2)], accessory=i >= 2)
            assert math.dist(offset, tuple(a - b for a, b in zip(point, pivot))) < .001
        visuals.clear()
        tick(101)
        assert not visuals
        tick(147, end=True)
        if cycle == 0:
            tick(0)
    assert {v[0] for v in visuals} == set(range(routes[30][0], sum(routes[31]))) | {
        routes[32][0], routes[34][0], routes[36][0]}
    common_bank = sound_banks["sound/se/pl/frontier_gl/common"]
    weapon_bank = sound_banks["sound/se/pl/frontier_gl/weapon"]
    assert sounds.count((common_bank, 101)) == 1
    assert sounds.count((weapon_bank, 2)) == 2 and sounds.count((weapon_bank, 3)) == 2
    tick(20)
    assert sounds.count((weapon_bank, 2)) == 3
    visuals.clear()
    assert shots() == [0, 0, 0] and g.get(a + 13096) == 0
    start(0)
    assert attacks[-1][0:2] == (1, 33) and attacks[-1][-1] == 1, (state(), attacks)
    assert [(s, j, p) for s, j, p, _ in visuals] == [
        (s, 1, weapon_parent) for s in blade_selectors(5)
    ]
    for element in range(1, 6):
        # 27C708 reads GU's equipped property table at equipment+800.
        g.cpu.mem_write(equipment + 800 + 2 * 17, struct.pack("<h", element))
        g.cpu.mem_write(equipment + 800 + 2 * 2, struct.pack("<h", 20))
        visuals.clear()
        start(0)
        assert [(s, j, p) for s, j, p, _ in visuals] == [
            (s, 1, weapon_parent) for s in blade_selectors(element - 1)
        ]
    visuals.clear()
    g.put(weapon + 1176, 0)
    start(0)
    assert not visuals
    g.put(weapon + 1176, remap)
    skip_alloc.append(True)
    tick(1)
    tick(2)
    tick(3)
    accepted = [v[0] for v in visuals]
    expected = blade_selectors(4)
    assert accepted == expected[1:] + expected[:1]
    stopped.clear()
    set_action(1, 0)
    tick()
    assert len(stopped) == len(set(stopped)) == len(expected)
    print("Special key, ignition audio, both flame cycles and buffed thrust playback passed")
    start(34)
    assert shots() == [0, 0, 0]  # dry charged shell
    start(5)
    start(34)
    tick(49)
    assert shots() == [0, 0, 0]
    tick(50)
    assert shots() == [1, 0, 0] and ammo() == 4
    clear_shots()
    start(5)
    start(30)
    tick(50)
    assert shots() == [1, 1, 1] and ammo() == 2
    clear_shots()
    tick(52)
    assert shots() == [1, 1, 0] and ammo() == 0
    clear_shots()

    # The run opener and the drafted forward thrust are distinct motions.
    g.stub(0x27F678, lambda *_: 0)
    set_action(1, 0)
    for strength in (0, 100, 0, 100):
        g.cpu.mem_write(a + 5136, struct.pack("<h", strength))
        tick()
    assert state() == 82 and g.half(a + 1204) == 1030
    tick(32, command=1)
    assert state() == 75
    start(82)
    tick(32, command=1 << 5)
    assert state() == 76
    start(82)
    tick(32, command=1 << 2)
    assert state() == 78 and shots() == [0, 0, 0]
    print("Native shell queue, chamber counts, burst and Wyvern Fire timing passed")

    sounds.clear()
    start(0)
    tick(17)
    assert not sounds
    tick(18)
    tick(18)
    assert sounds == [(weapon_bank, 6)]
    sounds.clear()
    start(5)
    tick(8)
    assert sounds == [(weapon_bank, 2)]
    shot_sound = (weapon_bank, 13)
    for slot, firing, cue in ((1, 0, 8), (12, 0, 8), (13, 0, 8),
                              (33, 68, 70), (34, 50, 54), (35, 50, 54), (36, 50, 54)):
        clear_shots()
        start(5)
        sounds.clear()
        start(slot)
        tick(firing)
        assert shots() == [1, 0, 0], (slot, shots())
        tick(cue - 1)
        assert shot_sound not in sounds, (slot, sounds)
        tick(cue)
        tick(cue)
        assert sounds.count(shot_sound) == 1, (slot, sounds)
    clear_shots()
    g.run(0x2AD4F4, a, -5 & 0xFFFFFFFF)
    sounds.clear()
    start(14)
    tick(12)
    assert shots() == [0, 0, 0] and sounds == [(weapon_bank, 8)]
    print("Packaged thrust, reload, dry fire and all shell angles play at their authored frames")
    sounds.clear()
    visuals.clear()
    muzzle, rumble, projectiles = [], [], []
    for address in (0x11B0C38, 0x2A063C):
        g.cpu.hook_del(g.traps.pop(address))
    g.stub(0x11B0C38, lambda _, kind, *__: muzzle.append(kind) or 0)
    g.stub(0x2A063C, lambda _, kind, *__: rumble.append(kind) or 0)
    g.stub(0x29C368, lambda *_: 0)
    g.stub(0x475850, lambda size, *_: g.alloc(size))

    def spawn_shell(_, task, *__):
        projectiles.append(struct.unpack("<5I", g.cpu.mem_read(task + 4, 20)))
        return g.alloc(5104)

    g.stub(0x48C4F0, spawn_shell)

    def selector_ids(index):
        first, count = routes[index]
        return list(range(first, first + count))

    def emitted(index):
        return [entry for entry in visuals if entry[0] in selector_ids(index)]

    set_action(1, 0)
    g.put(a + 28, real(7200))  # Let the earlier ignition expire before testing the plain A routes.
    tick()
    g.put(a + 28, real(2))
    for slot, firing, route in ((12, 0, 55), (13, 0, 55), (35, 50, 56), (36, 50, 56)):
        clear_shots()
        start(5)
        visuals.clear()
        if slot in (12, 13):
            start(2 if slot == 12 else 4)
            tick(32 if slot == 12 else 54, command=1 << 2)
            assert state() == slot, (slot, state())  # A after a guard thrust or rising slash.
        else:
            start(slot)
        if firing:
            tick(firing - 1)
            assert not emitted(route)
        tick(firing)
        tick(firing + 1)
        assert [entry[0] for entry in emitted(route)] == selector_ids(route)
        assert all(joint == 1 and owner == weapon_parent and
                   math.dist(offset, tuple(x-y for x, y in zip((7, -5, 72), pivot))) < .001
                   for _, joint, owner, offset in emitted(route))
        assert not muzzle, (slot, muzzle)
        assert shots() == [1, 0, 0] and ammo() == 4
    clear_shots()
    visuals.clear()
    g.run(0x2AD4F4, a, -5 & 0xFFFFFFFF)
    start(36)
    tick(50)
    assert not emitted(56) and shots() == [0, 0, 0]
    print("Inclined and vertical shells use one muzzle flash at the shot, including charged and dry cases")
    sounds.clear()
    muzzle.clear()
    rumble.clear()

    log = lambda: (len(muzzle), len(rumble), len(projectiles))
    g.run(0x2AD4F4, a, -5 & 0xFFFFFFFF)
    start(5)
    tick(43)
    assert not emitted(50)
    tick(44)
    tick(44)
    assert ammo() == 5 and len(emitted(50)) == 5
    g.run(0x2AD4F4, a, -2 & 0xFFFFFFFF)
    visuals.clear()
    start(39)
    tick(13)
    assert not emitted(50)
    tick(30, end=True)
    tick(14)
    assert ammo() == 4 and len(emitted(50)) == 1
    visuals.clear()
    start(67)
    tick(58)
    assert ammo() == 5 and len(emitted(50)) == 1

    start(30)
    tick(50)
    g.run(0x11A9A54, a)
    assert log() == (1, 1, 3) and shots() == [0, 0, 0]
    assert sounds.count(shot_sound) == 1
    tick(52)
    g.run(0x11A9A54, a)
    assert log() == (1, 1, 5) and all(p[0] == 48 and p[2] == a for p in projectiles)
    assert sounds.count(shot_sound) == 1
    start(5)
    start(31)
    tick(50)
    g.run(0x11A9A54, a)
    tick(52)
    g.run(0x11A9A54, a)
    assert log() == (3, 3, 7)
    assert sounds.count(shot_sound) == 3

    # The base model bank has no zones for these Wyvern Fire cues.
    sounds.clear()
    visuals.clear()
    start(3)
    tick(41)
    assert not emitted(51) and (weapon_bank, 28) not in sounds
    tick(42)
    assert len(emitted(51)) == 1 and sounds.count((weapon_bank, 28)) == 1
    stopped.clear()
    set_action(32, 0)
    tick()
    assert sum(unit_selectors[unit] in selector_ids(51) for unit in stopped) == 1, stopped

    start(59)
    tick(2)
    assert [s for s, *_ in emitted(56)] == selector_ids(56)
    g.run(0x11A9A54, a)
    tick(6)
    assert sounds.count((weapon_bank, 14)) == 1
    tick(43)
    assert not emitted(54)
    tick(44)
    assert len(emitted(54)) == 4
    tick(60)
    assert sounds.count((weapon_bank, 20)) == 1
    stopped.clear()
    g.cpu.mem_write(a + 4180, b"\x01")
    tick(61)
    assert sum(unit_selectors[unit] in selector_ids(54) for unit in stopped) == 4, stopped
    g.cpu.mem_write(a + 10057, b"\0")
    visuals.clear()
    tick(62)
    assert not emitted(54)
    g.cpu.mem_write(a + 10057, b"\x01")
    tick(63)
    assert len(emitted(54)) == 4
    g.put(a + 13096, 0)
    tick(64)
    g.run(0x2AD4F4, a, 4)
    start(47)
    visuals.clear()
    tick(6)
    assert sounds.count((weapon_bank, 28)) == 2 and len(emitted(51)) == 1
    assert (weapon_bank, 12) not in sounds
    tick(32)
    tick(32)
    assert len(emitted(50)) == 1 and sounds.count((weapon_bank, 12)) == 1
    tick(34, end=True)
    tick(24, command=1 << 2)
    assert len(emitted(50)) == 2 and len(emitted(52)) == len(emitted(53)) == 1

    for slot, frame in ((37, 18), (38, 18), (49, 4)):
        clear_shots()
        g.put(a + 13096, 0)
        sounds.clear()
        start(slot)
        tick(frame - 1)
        assert (weapon_bank, 28) not in sounds, (slot, sounds)
        tick(frame)
        assert sounds.count((weapon_bank, 28)) == 1, (slot, sounds)

    for slot, frame in ((48, 10), (50, 6), (60, 2), (61, 2), (73, 2), (74, 2)):
        clear_shots()
        g.put(a + 13096, 0)
        sounds.clear()
        start(slot)
        if slot in (73, 74):
            tick(58, end=True)
        tick(frame)
        tick(frame)
        assert sounds.count((weapon_bank, 14)) == 1, (slot, sounds)
    assert not aa_lift_flashes, aa_lift_flashes
    print("Reload casings, native shell consumer, burst feedback, Wyvern Fire effects and audio, and steam cleanup passed")

    water = -100001.
    g.put(0x21270e0, g.alloc(128))

    def ground(*_):
        sp = g.cpu.reg_read(UC_ARM_REG_SP)
        g.put(g.get(sp + 4), real(0))
        g.put(g.get(sp + 8) + 20, real(water))
        return 1

    g.stub(0x18192c, ground)
    visuals.clear()
    start(82)
    tick(2, next_frame=3)
    assert len(emitted(29)) == 1
    water = 25.
    tick(14, next_frame=15)
    assert len(emitted(27)) == 6

    # FZ node 21 is the equipped model; the recoil animation moves it independently.
    visuals.clear()
    start(74)
    assert g.half(a + 1204) == 1481
    tick(28, next_frame=29)
    assert [(s, j, p) for s, j, p, _ in emitted(7)] == [(s, 0, weapon_parent) for s in selector_ids(7)]
    for selector in selector_ids(7):
        assert proofs[selector][72] == 0 and struct.unpack_from("<I", proofs[selector], 4)[0] & 128
    print("Dry and wet sprint particles and the live barrel anchor passed")
    peer_parent = g.alloc(128)
    for address in (g.get(VTABLE + 304), 0x53A06C):
        g.cpu.hook_del(g.traps.pop(address))
    g.stub(g.get(VTABLE + 304), lambda obj, *_: {a: parent, peer: peer_parent}.get(obj, obj + 4048))
    g.stub(0x53A06C, lambda actor, _, motion, *__:
           g.cpu.mem_write(actor + 1204, struct.pack("<H", motion & 0xFFFF)) or 1)
    g.put(g.get(0x1885B58) + 32, peer)
    for field in (1172, 1176):
        g.put(peer + field, g.get(a + field))
    g.put(peer + 240, g.alloc(128))
    peer_weapon, *_ = equip_blade(g, peer)
    peer_barrel = peer_weapon + 4048
    g.put(peer + 28, real(2))
    g.cpu.mem_write(peer + 4180, b"\x01")
    g.cpu.mem_write(peer + 10057, b"\x01")
    g.cpu.mem_write(peer + 9478, struct.pack("<H2xH", 16, 0x8000 + 3))
    g.run(g.hooks[WEAPON_DISPATCH], peer)
    assert g.half(peer + 1204) == 1407
    visuals.clear()
    g.put(peer + 1280, real(42))
    g.run(g.hooks[WEAPON_DISPATCH], peer)
    assert len(emitted(51)) == 1 and emitted(51)[0][2] == peer_barrel, (emitted(51), peer_barrel, peer_parent)
    stopped.clear()
    g.cpu.mem_write(peer + 4180, b"\x02")
    set_action(1, 0)
    tick()
    assert sum(unit_selectors[unit] in selector_ids(51) for unit in stopped) == 1
    assert g.cpu.mem_read(peer + 11138, 1) == b"\0"
    g.cpu.mem_write(peer + 4180, b"\x01")
    g.cpu.mem_write(peer + 9478, struct.pack("<H2xH", 16, 0x8000 + 13))
    g.cpu.mem_write(peer + 5, b"\0")
    visuals.clear()
    g.run(g.hooks[WEAPON_DISPATCH], peer)
    assert [(s, j, p) for s, j, p, _ in emitted(55)] == [(s, 1, peer_barrel) for s in selector_ids(55)]
    assert g.cpu.mem_read(peer + 13100, 3) == bytes(3)
    visuals.clear()
    g.cpu.mem_write(peer + 9478, struct.pack("<H2xH", 16, 0x8000 + 59))
    g.cpu.mem_write(peer + 5, b"\0")
    g.run(g.hooks[WEAPON_DISPATCH], peer)
    g.put(peer + 1280, real(2))
    for _ in range(2):
        g.run(g.hooks[WEAPON_DISPATCH], peer)
    assert [(s, j, p) for s, j, p, _ in emitted(56)] == [(s, 1, peer_barrel) for s in selector_ids(56)]
    assert g.cpu.mem_read(peer + 13100, 3) == bytes(3)
    print("Peer charge follows the remote actor and retires on area change")

    # Run the native command-table interpreter for the sprint cancel.
    g.cpu.hook_del(g.traps.pop(0x2E9D54))
    g.put(a + 5068, 1)
    g.put(a + 5104, 1)
    g.cpu.mem_write(a + 5078, struct.pack("<h", 100))
    g.stub(g.originals[CHANGE_ACTION], lambda _, group, slot, flags: set_action(group, slot))
    art_prepared = []
    g.stub(0x2A8B34, lambda *_: int(bool(g.get(a + 5180) & 7)))
    g.stub(0x2ACBEC, lambda *_: art_prepared.append(True))

    def resolve_art(_, group, slot, flags):
        for address, value in ((group, 16), (slot, 65), (flags, 0x40)):
            g.cpu.mem_write(address, struct.pack("<H", value))
        return 1

    g.stub(0x2ACB60, resolve_art)
    for running in (82, 83):
        for slot in range(3):
            g.cpu.mem_write(a + 5136, struct.pack("<h", 120))
            start(running)
            assert state() == running
            g.put(a + 5180, 1 << slot)
            before = len(art_prepared)
            tick(2)
            assert (g.half(a + 9478), g.half(a + 9482)) == (16, 65), (
                running, slot, state(), art_prepared,
            )
            assert len(art_prepared) == before + 1
            g.put(a + 5180, 0)
    print("GU Gunlance: all three Hunter Art slots activate from sprint and brake")
    return g, weapon, blade_matrix, pivot, tip, proofs


def verify_native_arts():
    g, actor, *_ = scene()
    starts = {struct.unpack_from("<H4xH", zlib.decompress(raw), 0x1F8)
              for name, kind, raw in quest_entries("w09_m.arc")
              if kind == 0x5139901D and b"\\sa\\" in name}
    assert starts == {(16, slot) for slot in (65, 66, 67, 77, 78, 79, 80, 81, 82, 95, 96, 97)}
    seen = []

    def forwarded(target, group, slot, flags):
        assert target == actor
        seen.append((group, slot, flags))
        return 1

    g.stub(g.originals[CHANGE_ACTION], forwarded)
    g.cpu.mem_write(actor + 9478, struct.pack("<H2xH", 16, 0x8000 + 5))
    for group, slot in sorted(starts):
        assert g.run(g.hooks[CHANGE_ACTION], actor, group, slot, 0x40) == 1
    assert seen == [(group, slot, 0x40) for group, slot in sorted(starts)]
    for current, following in ((65, 68), (77, 83), (80, 88), (95, 98)):
        g.cpu.mem_write(actor + 9482, struct.pack("<H", current))
        assert g.run(g.hooks[CHANGE_ACTION], actor, 16, following, 0) == 1
        assert seen[-1] == (16, following, 0)
    for tier in range(3):
        g.cpu.mem_write(actor + 9482, struct.pack("<H", 98 + tier))
        assert g.run(g.hooks[CHANGE_ACTION], actor, 16, 135 + tier, 0x0C) == 1
        assert seen[-1] == (16, 135 + tier, 0x0C)
        assert g.run(g.hooks[CHANGE_ACTION], actor, 16, 135 + (tier + 1) % 3, 0x0C) == 0
    print("All four GU Hunter Arts retain their native action entry points")


def verify_aa_flare():
    g, actor, *_ = scene()
    # Use the native 2B8178 dispatcher, handlers, action request and shell queue.
    # The offline scene records animation and particle requests without rendering.
    def change(_, group, slot, flags):
        g.cpu.mem_write(actor + 9478, struct.pack("<H2xH", group, slot))
        g.cpu.mem_write(actor + 4, bytes(4))
        return 1

    g.stub(g.originals[CHANGE_ACTION], change)
    for addr in (0x282FD8, 0x283238, 0x28321C, 0x2A063C, 0x2AD288, 0x11AA480, 0x539DA0):
        if addr in g.traps:
            g.cpu.hook_del(g.traps.pop(addr))
        g.stub(addr, lambda *_: 0)
    motions, flashes, projectiles = [], [], []
    g.stub(0x2A8D10, lambda _, row, *__: motions.append(g.get(row + 4) & 0xFFFF) or 1)
    g.stub(0x281998, lambda _, motion, *__: motions.append(motion) or 1)
    g.stub(0x11B0C38, lambda _, kind, *__: flashes.append(kind) or 1)
    g.stub(0x11B0E94, lambda *_: flashes.append("burst") or 1)
    g.stub(0x2B6E94, lambda *_: change(actor, 1, 0, 0))
    g.stub(0x27BCD0, lambda *_: 1)
    g.stub(0x282850, lambda *_: 0)
    g.stub(0x282870, lambda _, index, *__: g.cpu.reg_write(UC_ARM_REG_S0, real((120, 90)[index])))
    g.stub(g.get(VTABLE + 2224), lambda *_: g.cpu.reg_write(UC_ARM_REG_S0, g.get(actor + 13096)))
    g.stub(0x29C368, lambda *_: 0)
    g.stub(0x475850, lambda size, *_: g.alloc(size))
    g.stub(0x48C4F0, lambda _, task, *__: projectiles.append(g.get(task + 8)) or g.alloc(5104))
    g.put(actor + 28, real(2))
    g.cpu.mem_write(actor + 11140, b"\x05")

    def tick(previous, current, end=False, duration=256.):
        floats(g, actor + 5328, (previous, current - previous))
        floats(g, actor + 1276, (current, previous, duration))
        g.cpu.mem_write(actor + 1206, bytes([4 if end else 0]))
        g.run(g.hooks[WEAPON_DISPATCH], actor)

    for tier, skill, cooldown in ((0, 0, 2400), (1, 176, 1920), (2, 175, 1200)):
        motions.clear()
        flashes.clear()
        projectiles.clear()
        if 0x2864C4 in g.traps:
            g.cpu.hook_del(g.traps.pop(0x2864C4))
        g.stub(0x2864C4, lambda _, query, *__, skill=skill: int(query == skill))
        g.put(actor + 13096, 0)
        g.cpu.mem_write(actor + 11138, b"\x05")
        g.run(g.hooks[CHANGE_ACTION], actor, 16, 95 + tier, 0x40)
        tick(0, 0)
        tick(85, 87)
        assert flashes == ["burst"]
        tick(89, 90)
        g.run(0x11A9A54, actor)
        tick(93, 96)
        g.run(0x11A9A54, actor)
        assert projectiles == [27] * 5 and g.cpu.mem_read(actor + 11138, 1) == b"\0"
        tick(98, 100)
        assert g.half(actor + 9482) == 98 + tier, (tier, g.half(actor + 9482), motions)
        tick(0, 0)
        tick(58, 60)
        assert g.half(actor + 9482) == 135 + tier, (tier, g.half(actor + 9482), motions)
        tick(0, 0)
        assert motions[-1] == 7152 and g.cpu.mem_read(actor + 13100, 1) == b"\x01"
        assert 30 <= g.get(actor + 13104) <= 32 and memory_float(g, actor + 13096) == cooldown
        g.run(0x11A9A54, actor)
        assert projectiles[-1] == 30
        tick(100, 100, end=True, duration=100.)
        assert motions[-1] == 7151
        tick(210, 212)
        assert flashes == ["burst", 0]
        tick(256, 256, end=True)
        assert (g.half(actor + 9478), g.half(actor + 9482)) == (1, 0)

    g.put(actor + 13096, real(600))
    g.run(g.hooks[CHANGE_ACTION], actor, 16, 95, 0x40)
    tick(0, 0)
    tick(98, 100)
    assert g.half(actor + 9482) == 95 and g.cpu.mem_read(actor + 5, 1) == b"\x02"
    tick(256, 256, end=True)
    assert (g.half(actor + 9478), g.half(actor + 9482)) == (1, 0)
    assert memory_float(g, actor + 13096) == 600
    print("AA Flare I–III reach the native Wyvern Fire queue, muzzle feedback and recovery")


def verify_particles():
    g, *_ = scene()
    load, _, draw = model_renderer(g, NX, depth_scene=True)
    folder = NX / "effect/frontier_gl"

    def offsets(model):
        uv, = struct.unpack_from("<H", model, 308)
        if not uv:
            return []
        offset = struct.unpack_from("<I", model, uv + 48)[0]
        if not offset:
            return []
        meta, = struct.unpack_from("<I", model, offset)
        if meta >> 24 != 0x18:
            return []
        return [struct.unpack_from("<If", model, offset + 4 + i * 12) for i in range(meta & 255)]

    stepped = faded = 0
    candidates = []
    for path in sorted(folder.glob("*.efl")):
        data = path.read_bytes()
        start = efl_model_offset(data)
        model = data[start:]
        keys = offsets(model)
        if len({value for _, value in keys}) > 1:
            stepped += 1
        if model[136:139] != b"FZF":
            continue
        threshold, = struct.unpack_from("<f", model, 140)
        if threshold <= 0:
            continue
        assert any(abs(threshold - expected) < 1e-7 for expected in (.01, .03))
        faded += 1
        if len({value for _, value in keys}) > 1:
            candidates.append((path, data, keys[1][0]))

    assert stepped > 0 and faded > 0 and candidates
    for path, data, sample in candidates[:8]:
        gen = emit_model_layer(g, load, data, sample + 1)
        params = g.get(gen + 52)
        assert g.cpu.mem_read(params + 136, 3) == b"FZF"
        signature = bytes(g.cpu.mem_read(params + 136, 8))
        prefix = bytes(g.cpu.mem_read(params + 80, 1))
        g.cpu.mem_write(params + 136, bytes(8))
        g.cpu.mem_write(params + 80, b"_")
        control = draw(gen)
        g.cpu.mem_write(params + 80, prefix)
        g.cpu.mem_write(params + 136, signature)
        result = draw(gen)
        if len(control) != 1 or len(result) != 2:
            continue
        original, = control
        near, far = result
        expected = tuple((channel * 7 + 5) // 10 for channel in original.rgb)
        if original.alpha == 0 or expected == original.rgb:
            continue
        assert near.rgb == far.rgb == expected, path
        assert near.alpha + far.alpha == original.alpha, path
        assert near.uv == far.uv == original.uv, path
        assert near.position != far.position, path
        print(f"Native GU draw verified fade, RGB gain and alpha for {path.name}; {stepped} stepped UV emitters")
        break
    else:
        raise AssertionError("No active soft atlas layer submitted both GU draw packets")

    routes = list(struct.iter_unpack("<2H", (ROOT / "target/assets/effects/routes.bin").read_bytes()))
    delays = [(0, 5, 0, 5, 10), (0, 5, 0, 5, 10), (0, 5, 0, 0),
              (0, 5, 0, 0), (0, 5, 0, 3, 6), (0, 5, 0, 5, 0, 5)]
    for element, delay_slots in enumerate(delays):
        core, accent = routes[38 + element * 2:40 + element * 2]
        assert (core[1], accent[1]) == (2, len(delay_slots))
        for i, delay in enumerate((0, 0) + delay_slots):
            selector = core[0] + i
            data = (folder / f"e{selector}.efl").read_bytes()
            threshold, = struct.unpack_from("<f", data, efl_model_offset(data) + 140)
            assert abs(threshold - (.01 if i < 2 else .03)) < 1e-7
            gen = emit_model_layer(g, load, data, delay + 6)
            params = g.get(gen + 52)
            signature = bytes(g.cpu.mem_read(params + 136, 8))
            assert signature[:3] == b"FZF"
            near, far = draw(gen)
            g.cpu.mem_write(params + 136, bytes(8))
            g.cpu.mem_write(params + 80, b"_")
            control, = draw(gen)
            assert control.alpha > 0 and any(control.rgb)
            assert near.rgb == far.rgb == tuple((channel * 7 + 5) // 10 for channel in control.rgb)
            assert near.alpha + far.alpha == control.alpha
            assert near.position != far.position and not (near.flags & 0x18 or far.flags & 0x18)
            if delay > 1:
                early = draw(emit_model_layer(g, load, data, delay - 1))
                assert not any(p.alpha for p in early), (element, selector, delay)
    for index, threshold, delays in (
        (27, .03, (0, 4, 0, 4, 0, 4)), (28, .03, (0,)), (29, .03, (0,)),
        (51, .01, (0,)), (52, .01, (0,)), (53, .03, (0,)), (54, .03, (0, 6, 12, 18)),
        (55, .01, (0, 0, 0)), (56, .01, (0, 0, 0)),
    ):
        first, count = routes[index]
        assert count == len(delays)
        for selector, delay in zip(range(first, first + count), delays):
            data = (folder / f"e{selector}.efl").read_bytes()
            fade_at = struct.unpack_from("<f", data, efl_model_offset(data) + 140)[0]
            assert abs(fade_at - threshold) < 1e-7
            gen = emit_model_layer(g, load, data, delay + (2 if index >= 55 else 6))
            params = g.get(gen + 52)
            result = draw(gen)
            g.cpu.mem_write(params + 136, bytes(8))
            g.cpu.mem_write(params + 80, b"_")
            control = draw(gen)
            assert control and len(result) == 2 * len(control), (index, selector)
            for original, (near, far) in zip(control, zip(result[::2], result[1::2])):
                assert original.alpha > 0
                assert near.alpha + far.alpha == original.alpha
                assert near.rgb == far.rgb == tuple((channel * 7 + 5) // 10 for channel in original.rgb)
    print("Blade, accessory, terrain and muzzle layers passed fade, alpha and 0.7 RGB checks")


def verify_blade_pose(replay):
    # Compare the compiled flame/halo with the original Vita table and curves.
    fz = (REPO / "mhfz/eboot.bin.elf").read_bytes()
    phoff, = struct.unpack_from("<I", fz, 28)
    entsize, count = struct.unpack_from("<2H", fz, 42)
    segments = [struct.unpack_from("<8I", fz, phoff + i * entsize) for i in range(count)]

    def read_vita(address, size):
        offset, base = next((off, base) for kind, off, base, _, length, *_ in segments
                            if kind == 1 and base <= address <= base + length - size)
        offset += address - base
        return fz[offset:offset + size]

    scale_pointers = struct.unpack("<2I", read_vita(0x82C8B5D4, 8))
    scale_keys = [list(struct.iter_unpack("<4f", read_vita(ptr, 16 * count)))
                  for ptr, count in zip(scale_pointers, (3, 2))]
    pulse = struct.unpack("<2f", read_vita(0x827BF420, 8))

    def sample_scale(keys, age):
        for (a, *first), (b, *last) in zip(keys, keys[1:]):
            if age <= b:
                return [x + (y - x) * (age - a) / (b - a) for x, y in zip(first, last)]
        return keys[-1][1:]

    # Replay the NRO's PEL proofs with retail bind data and native EFL ticks.
    g, weapon, blade_matrix, pivot, tip, proofs = replay
    load, camera, draw = model_renderer(g, NX, depth_scene=True)
    folder = NX / "effect/frontier_gl"
    routes = list(struct.iter_unpack("<2H", (ROOT / "target/assets/effects/routes.bin").read_bytes()))
    core_layers = {}
    for route, source_row in zip(range(38, 50, 2), (1, 2, 3, 4, 5, 7)):
        first, count = routes[route]
        assert count == 2
        mesh, sx, sy, sz, main_position, halo_position, main_color, halo_color = struct.unpack(
            "<I5f2I", read_vita(0x827BF144 + 52 * source_row, 32))
        core_layers[first] = (mesh, False, main_position, (sx, sy, sz), main_color)
        core_layers[first + 1] = (81, True, halo_position, (1., 1., 1.), halo_color)
    pel = (folder / "particles.pel").read_bytes()
    records = {}
    offset = 16
    for selector in range(struct.unpack_from("<I", pel, 12)[0]):
        for _ in range(2):
            offset += 1 + pel[offset]
        if selector in core_layers:
            records[selector] = pel[offset:offset + 160]
        offset += 160

    parent, parent_vt = weapon + 4048, g.alloc(8)
    g.put(parent, parent_vt)
    for slot, value in ((0, 1), (4, weapon)):
        target = g.alloc(4)
        g.put(parent_vt + slot, target)
        g.stub(target, lambda *_, value=value: value)
    g.put(0x2126504, g.alloc(1800))
    g.stub(0x9B7108, lambda unit, *_: g.put(unit + 252, real(1)))
    # Hold the weapon root fixed, and animate the barrel independently of it.
    floats(g, weapon + 176, (1, 0, 0, 0, 0, 0, 1, 0, 0, -1, 0, 0, 100, 60, -20, 1))

    for selector, record in records.items():
        mesh, is_halo, multiplier, factors, color_address = core_layers[selector]
        efl = (folder / f"e{selector}.efl").read_bytes()
        model = efl[efl_model_offset(efl):]
        assert model[80:].startswith(f"effect\\frontier_gl\\m{mesh}\0".encode())
        assert model[139] == (0 if is_halo else 4)
        color_start, = struct.unpack_from("<H", model, 66)
        colors = struct.iter_unpack("<I4s", read_vita(color_address, 40))
        for i, (frame, argb) in enumerate(colors):
            rgba = bytes((argb[2], argb[1], argb[0], argb[3]))
            assert struct.unpack_from("<I4s", model, color_start + 4 + 12 * i) == (frame, rgba)
        scale_start, = struct.unpack_from("<H", model, 306)
        for age in range(51):
            scale_row = scale_start + 4 + age * 28
            frame, x, _, y, _, z, _ = struct.unpack_from("<I6f", model, scale_row)
            source_scale = sample_scale(scale_keys[is_halo], age)
            expected = [v * m * pulse[age % 2] for v, m in zip(source_scale, factors)]
            assert frame == age and math.dist((x, y, z), expected) < .00001

        owner, proof, row, bank, entries, desc = (g.alloc(n) for n in (416, 256, 160, 144, 4, 32))
        g.put(owner + 52, bank)
        g.put(bank + 104, 1)
        g.put(bank + 116, entries)
        g.put(entries, desc)
        g.cpu.mem_write(proof + 48, proofs[selector])
        g.put(owner + 12, 1)
        g.cpu.mem_write(owner + 396, b"\x01")
        g.cpu.mem_write(row, record)
        g.run(0x32A548, owner, proof, row)
        unit = g.alloc(896)
        g.run(0x3273F0, unit, owner, proof)
        g.put(unit + 244, load(efl))
        assert g.run(0x9BDA64, unit) == 1
        gen = g.get(unit + 496)
        # Inspect the geometry before the separately tested depth-fade passes.
        g.put(g.get(gen + 52) + 140, 0)
        g.put(unit + 272, 1)
        lx, ly, lz = (a - b for a, b in zip(blade_point(tip, multiplier), pivot))

        for frame in range(10):
            turn = int(frame >= 5)
            c, s = 1 - turn, turn
            x, y, z = 100. + 16 * turn, 60. + 35 * turn, -20. + 14 * turn
            floats(g, blade_matrix, (1, 0, 0, 0, 0, c, -s, 0, 0, s, c, 0, x, y, z, 1))
            floats(g, unit + 28, (1,))
            floats(g, unit + 248, (1,))
            g.run(0x327948, unit)
            origin = struct.unpack("<3f", g.cpu.mem_read(unit + 64, 12))
            expected = x + lx, y + c * ly + s * lz, z - s * ly + c * lz
            assert math.dist(origin, expected) < .01, (origin, expected)
            if frame in (4, 9):
                for eye in ((700, 400, 850), (85, 230, 620)):
                    floats(g, camera + 48, eye)
                    packet, = draw(gen)
                    assert packet.alpha > 0 and math.dist(origin, packet.position) < .01
                    if not is_halo:
                        assert math.dist(unit_vector(packet.normal), (0, s, c)) < .01
                        dx, dy, dz = (a - b for a, b in zip(eye, origin))
                        side = unit_vector((dy * c - dz * s, -dx * c, dx * s))
                        assert math.dist(unit_vector(packet.basis_x), side) < .01
    print("All six palettes match Vita flame/halo scale, color and live barrel attachment")


def unit_vector(vector):
    length = math.hypot(*vector)
    return tuple(value / length for value in vector)


def audible_wave(data):
    # Decode the Nintendo DSP blocks to catch valid wave files containing silence.
    coefficients = struct.unpack_from("<16h", data, 24)
    samples, = struct.unpack_from("<I", data, 96)
    previous = earlier = 0
    at = 120
    for start in range(0, samples, 14):
        header = data[at]
        predictor, scale = header >> 4, header & 15
        a, b = coefficients[2 * predictor:2 * predictor + 2]
        for i in range(min(14, samples - start)):
            packed = data[at + 1 + i // 2]
            delta = (packed >> (0 if i % 2 else 4)) & 15
            if delta >= 8:
                delta -= 16
            sample = (a * previous + b * earlier + 1024) >> 11
            sample += delta << scale
            if abs(sample) > 256:
                return True
            previous, earlier = sample, previous
        at += 8
    return False


def verify_assets():
    data = (ROOT / "target/assets/gunlance.fgla").read_bytes()
    assert data[:8] == b"FGLA\x02\0R\0"
    native = {
        3: (1407, 1418), 37: (1442, 1444), 38: (1441, 1443),
        47: (1437, 1438), 48: (1440,), 49: (1460, 1461), 50: (1463,),
        59: (1408,), 60: (1452,), 61: (1451,),
    }
    motions = {1030, 1031}
    empty = set()
    for state in range(82):
        count, offset = struct.unpack_from("<II", data, 16 + 24 * state)
        if not count:
            empty.add(state)
            motions.update(native.get(state, ()))
        for _ in range(count):
            op, clip, *_ = struct.unpack_from("<6H", data, offset)
            if 3 <= op <= 5:
                motions.add(clip)
            offset += 12
    assert empty == native.keys() | {8}
    for sex in "mf":
        for clip in motions:
            bank, slot = divmod(clip - 1000, 100)
            lmt = (NX / f"player/mot/frontier_gl/{bank}.lmt").read_bytes()
            slots, = struct.unpack_from("<H", lmt, 6)
            motion, = struct.unpack_from("<I", lmt, 8 + 4 * slot)
            if sex == "f" and slot + 128 < slots:
                variant, = struct.unpack_from("<I", lmt, 8 + 4 * (slot + 128))
                motion = variant or motion
            assert slot < slots and motion, (sex, clip)
            equipment, = struct.unpack_from("<I", lmt, motion + 52)
            assert struct.unpack_from("<2H", lmt, equipment) == (3, 2), (sex, clip)
    tracks = ROOT / "target/assets"
    banks = []
    for name in ("blade", "common", "weapon"):
        base = NX / f"sound/se/pl/frontier_gl/{name}"
        request = base.with_suffix(".srqr").read_bytes()
        bank = base.with_suffix(".sbkr").read_bytes()
        count, = struct.unpack_from("<I", request, 8)
        assert bank[:4] == b"SBKR" and struct.unpack_from("<I", bank, 12)[0] == count
        cues, cursor = set(), 32
        for i in range(count):
            cue, note = struct.unpack_from("<HB", request, 52 + 132 * i)
            assert note == i + 1
            cues.add(cue - 4096)
            end = bank.index(0, cursor)
            wave = bank[cursor:end].decode().replace("\\", "/")
            assert bank[end + 10] > 0, (name, cue - 4096, wave)
            assert audible_wave((NX / f"{wave}.adpcm").read_bytes()), (name, cue - 4096, wave)
            cursor = end + 1 + 80
        assert cursor == len(bank)
        banks.append(cues)
    sound = (tracks / "frontier_gl_sound.flse").read_bytes()
    for slot, (offset, count) in enumerate(struct.iter_unpack("<2H", sound[:2000])):
        for row in struct.iter_unpack("<H10B", sound[offset:offset + 12 * count]):
            frame, bank, choices, *weights = row
            for cue in weights[:2 * choices:2]:
                assert cue == 255 or cue in banks[bank], (1000 + slot, frame, bank, cue)

    fx = list(struct.iter_unpack("<5H2B3h", (tracks / "effects/motion_fx.bin").read_bytes()))
    assert len(fx) == 433 and {row[2] for row in fx} == {1, 3, 5, 6, 7}
    routes = (tracks / "effects/routes.bin").read_bytes()
    assert len(routes) == 228 and all(count for _, count in struct.iter_unpack("<2H", routes))
    print(f"71 authored states, 10 native states, {len(motions)} clips per sex and 57 particle routes, 433 FX rows and motion sound requests passed")


if __name__ == "__main__":
    verify_assets()
    verify_heat()
    verify_ignition_hud()
    blade_replay = verify_actions()
    verify_native_arts()
    verify_aa_flare()
    verify_particles()
    verify_blade_pose(blade_replay)
