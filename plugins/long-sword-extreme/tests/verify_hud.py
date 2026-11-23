"""Exercise the packaged LS HUD hook and GU's texture, polygon and visibility setters."""
import math
import struct
from pathlib import Path

from unicorn import UC_HOOK_CODE, arm_const


def check_atlas_window():
    atlas = (
        Path(__file__).resolve().parents[1]
        / "dist/long-sword-extreme/romfs/nativeNX/GUI/frontier_ls/gauge.tex"
    ).read_bytes()
    assert atlas[:4] == b"TEX\0"
    dimensions = struct.unpack_from("<I", atlas, 8)[0]
    width = (dimensions >> 6) & 0x1fff
    assert (width, dimensions >> 19, dimensions & 63) == (3072, 1024, 1)
    base = 24 + struct.unpack_from("<I", atlas, 20)[0]

    def rgba(x, y):
        # Switch TEX stores RGBA8 in block-linear GOBs of 64 bytes by 8 rows.
        x *= 4
        gob = (y // 128 * (width // 16) + x // 64) * 16 + y // 8 % 16
        offset = gob * 512 + x % 64 // 32 * 256 + y % 8 // 2 * 64
        offset += x % 32 // 16 * 32 + y % 2 * 16 + x % 16
        return atlas[base + offset:base + offset + 4]

    # The fill and the unfilled tail show through the same two-row opening.
    gold = bytes((245, 206, 25, 255))
    lengths = []
    for frame in range(60 + 24 * 16):
        x, y = frame % 16 * 192, frame // 16 * 32
        assert rgba(x + 50, y + 4)[3] == 255
        for row in (5, 6):
            assert rgba(x + 46, y + row)[3] and rgba(x + 143, y + row)[3]
            assert all(rgba(x + col, y + row) == bytes(4) for col in range(47, 143))
        if frame < 60:
            lengths.append(sum(rgba(x + col, y + 21) == gold for col in range(60, 150)))
    assert lengths[0] == 0 and 30 <= lengths[30] <= 40 and lengths[-1] >= 69
    assert lengths == sorted(lengths)


def replay(g, actor, begin, tick, set_action, drawn, target, shape):
    check_atlas_window()

    def block(fmt, address, *values):
        g.cpu.mem_write(address, struct.pack("<" + fmt, *values))

    def floats(address, count):
        return struct.unpack("<" + str(count) + "f", g.cpu.mem_read(address, 4 * count))

    widget, tree, groups, group, children = [g.alloc(n) for n in (200, 680, 12, 240, 16)]
    plate, fill, back, maximum = [g.alloc(304) for _ in range(4)]
    g.put(widget + 36, tree)
    g.put(tree + 248, groups)
    g.put(tree + 664, 3)
    g.put(groups, group)
    max_group, max_children = g.alloc(240), g.alloc(12)
    g.put(groups + 8, max_group)
    g.put(max_group + 216, max_children)
    g.put(max_children + 8, maximum)
    block("2f", maximum + 180, 72., 32.)
    # The quest crash reached A344F4 with a non-object from the subtree.
    # Execute its real code and catch that entry before it can call address 0.
    def check_reset(cpu, *_):
        node = cpu.reg_read(arm_const.UC_ARM_REG_R0)
        assert node != 0x10002, "HUD hide walked the invalid quest-entry child"

    reset_check = g.cpu.hook_add(UC_HOOK_CODE, check_reset, begin=0xA344F4, end=0xA344F4)
    g.put(group + 216, children)
    block("3I", children + 4, plate, fill, back)
    # Match hud5_l_sword.gui's sprite and polygon priorities.
    render_owner = g.alloc(680)
    g.put(group + 96, plate)
    for node, priority, sibling in ((plate, 4, fill), (fill, 20, back), (back, 24, 0)):
        g.put(node + 8, priority)
        block("3I", node + 100, sibling, group, render_owner)
    g.put(render_owner + 328, 8)  # Enable A34070's draw-pass filter.
    block("H", plate + 88, 4)
    block("H", fill + 88, 1)
    draw_context = g.alloc(32)
    vt, refresh, manager, manager_vt, load, texture = [g.alloc(n) for n in (128, 4, 32, 64, 4, 240)]
    for node in (plate, fill, back, maximum):
        g.put(node, vt)
    g.put(max_group + 96, 0x10002)
    g.put(vt + 76, refresh)
    g.put(vt + 80, refresh)
    g.stub(refresh, lambda *_: 0)
    polygon_vt = g.alloc(128)
    g.put(polygon_vt + 56, refresh)
    g.put(polygon_vt + 60, 0xA4E7B4)
    g.put(polygon_vt + 64, refresh)
    g.put(polygon_vt + 76, refresh)
    g.put(polygon_vt + 80, 0x54868C)
    for node, offset in ((back, 192), (fill, 96)):
        g.put(node, polygon_vt)
        block("2I", node + 188, offset, 0x01800060)
        g.put(node + 204, 0x10)
    g.put(manager, manager_vt)
    g.put(manager_vt + 48, load)
    previous_manager = g.get(0x2126A64)
    g.put(0x2126A64, manager)
    g.stub(load, lambda _, kind, name, __: texture if kind == 0x21255D0 and g.string(name) == r"GUI\frontier_ls\gauge" else shape if kind == 0x1857810 else 0)
    block("2I", texture + 220, 3072, 1024)
    g.stub(0x887660, lambda *_: 0)
    gui, directory, desc = [g.alloc(n) for n in (128, 256, 64)]
    g.put(plate + 120, gui)
    g.put(gui + 100, directory)
    g.put(directory + 232, desc)
    g.put(directory + 112, 1)
    g.put(desc, 23)
    g.put(desc + 40, texture)
    g.put(plate + 256, 23)
    g.put(plate + 260, desc)
    stock = (368, 32, 496, 56)
    block("4f", plate + 268, *stock)
    # Values from hud5_l_sword.gui group 0. Geometry is checked below
    # through the native transform update, including non-uniform HUD scale.
    block("2f", plate + 128, 0., 0.)
    block("2f", plate + 180, 128., 24.)
    g.put(plate + 84, 0x01000009)
    for node in (fill, back):
        block("2f", node + 128, 20., 10.)
        block("2f", node + 180, 98., 4.)
    block("2f", fill + 180, 98., 4.)
    g.put(fill + 160, 0)
    g.put(fill + 84, 0x03000009)
    colors = (0xff010203, 0xff020304, 0xff030405, 0xff040506)
    block("4I", fill + 164, *colors)
    g.put(back + 84, 0x0306000D)
    block("4I", back + 164, *([0xff414141] * 4))
    g.put(maximum + 84, 0x02060007)
    calls = []

    def native_hud(*_):
        assert floats(back + 128, 2) == (20., 10.)
        assert floats(fill + 128, 2) == (20., 10.)
        assert floats(maximum + 180, 2) == (72., 32.)
        assert g.get(maximum + 84) == 0x02060007
        assert (g.get(back + 8), g.get(fill + 8), g.get(plate + 8)) == (24, 20, 4)
        assert g.half(fill + 88) in (1, 5)
        calls.append(1)
        return 57

    g.stub(g.originals[0x64B3D8], native_hud)

    def draw():
        before = len(calls)
        assert g.run(g.hooks[0x64B3D8], widget) == 57
        assert len(calls) == before + 1

    def cell():
        x,y,right,bottom = floats(plate + 268, 4)
        assert right-x == 192 and bottom-y == 32
        return int(x//192 + 16*(y//32))

    def draw_order():
        g.run(0xA34070, group, draw_context)
        node = g.get(group + 96)
        ordered = []
        for _ in range(3):
            ordered.append(node)
            node = g.get(node + 100)
        assert node == 0
        return ordered

    vertex_manager, vertices, command = [g.alloc(n) for n in (224, 288, 640)]
    previous_vertex_manager = g.get(0x212668C)
    g.put(0x212668C, vertex_manager)
    g.put(vertex_manager + 144, 1)
    g.put(vertex_manager + 152, 1)
    g.put(vertex_manager + 156, 288)
    g.put(vertex_manager + 160, 96)
    g.put(vertex_manager + 176, vertices)
    g.put(vertex_manager + 192, plate + 188)
    g.put(draw_context + 4, command)
    g.put(command + 360, 2)  # Draw the backing's native pass.
    g.put(vt + 56, 0xA4E628)
    g.put(vt + 60, 0xA4FC9C)
    g.put(vt + 64, refresh)
    g.put(vt + 80, 0xA4FC68)
    block("16f", plate + 16, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 17, 22, 0, 1)
    block("4I", plate + 188, 0, 0x41800060, vertex_manager + 180, 0)
    block("4I", plate + 164, *([0xffffffff] * 4))
    submissions, transforms, textures = [], [], []
    for helper in (0x541624, 0x541844, 0x8826FC, 0x882760):
        g.stub(helper, lambda *_: 0)
    g.stub(0xA4EB9C, lambda _, matrix, *__: transforms.append(floats(matrix + 48, 3)))
    g.stub(0xA3E3B0, lambda _, matrix, *__: transforms.append(floats(matrix + 48, 3)))
    g.stub(0x542AA0, lambda _, descriptor, *__: textures.append(g.get(descriptor + 40)))

    def submit(cmd, count, first, *_):
        assert (cmd, count, first) == (command, 4, 0)
        submissions.append(g.get(cmd + 312))

    g.stub(0x93EFF4, submit)

    math_imports = []
    for address, fn in ((0x13EFDE8, math.sin), (0x13EFDF4, math.cos), (0x13EFDDC, math.sqrt)):
        if address not in g.traps:
            def host_math(*_, fn=fn):
                bits = g.cpu.reg_read(arm_const.UC_ARM_REG_S0)
                value, = struct.unpack("<f", struct.pack("<I", bits))
                g.cpu.reg_write(arm_const.UC_ARM_REG_S0, struct.unpack("<I", struct.pack("<f", fn(value)))[0])
            g.stub(address, host_math)
            math_imports.append(address)
    g.run(0x7C72E4)
    parent = g.alloc(64)
    block("16f", parent, 1.5, 0, 0, 0, 0, 2.25, 0, 0, 0, 0, 1, 0, 17, 22, 0, 1)
    block("2f", render_owner + 456, 1., 1.)
    for node in (plate, fill, back):
        g.put(node + 116, render_owner)
        g.put(node + 160, g.get(node + 160) | 0xff0)
        g.put(g.get(node) + 36, refresh)
        g.put(g.get(node) + 116, refresh)  # All three shapes are unmasked.
    def check_rendered_fill(width, sign=1):
        for node in (plate, fill, back):
            g.run(0xA4E604, node, parent)
            g.put(node + 192, g.get(node + 192) | 0x01000000)
        # Capture stock polygon pass 0 before the opaque artwork in pass 2.
        submissions.clear()
        transforms.clear()
        textures.clear()
        g.put(draw_context + 12, 2)
        for draw_pass in (0, 2):
            g.put(command + 360, draw_pass)
            g.run(0xA34070, group, draw_context)
        g.put(draw_context + 12, 0)
        assert submissions == [96, 192, 0], submissions
        assert textures == [texture]
        px, py, _ = transforms[2]
        sx = floats(plate + 16, 1)[0]
        sy = floats(plate + 36, 1)[0]
        assert abs(sx - 1.5) < 0.001 and abs(sy - 2.25) < 0.001, (sx, sy)
        for offset, start, span, origin in zip((96,192), (0.,width),
                                              (width,96.-width), transforms[:2]):
            assert abs(origin[0] - px - (47 + start)*sx) < 0.001, (origin, px, sx)
            assert abs(origin[1] - py - 5*sign*sy) < 0.001, (origin, py, sy)
            for i in range(4):
                x,y,z,color = struct.unpack("<3fI", g.cpu.mem_read(vertices+offset+i*24,16))
                assert abs(x - (i % 2)*span) < 0.001 and y == (i//2)*sign*2 and z == 0
                assert color == (g.get(fill+164) if offset==96 else 0xff0e0e0e), hex(color)
        color = g.get(fill+164)
        assert color & 0xff0000ff == 0xff0000ff
        return (color >> 8) & 255

    begin(0)
    tick(56)
    draw()
    assert floats(plate + 268, 4) == stock and g.get(back + 84) & 1
    assert draw_order() == [plate, fill, back]
    g.put(texture + 80, 1)
    # Let GU submit both stock polygons in the same pass as the plate.
    for node in (fill, back):
        block("H", node + 88, 1)
    for value in (0, 1, 25, 28, 75, 99, 100):
        g.put(actor + 10112, value)
        draw()
        assert cell() == 0 and g.get(plate + 256) == 0x40000003
        assert g.get(g.get(plate + 260) + 12) == (1024 << 16) | 3072
        assert floats(fill + 180, 2)[1] == 2 and floats(maximum + 180, 2) == (0., 0.)
        assert g.get(fill + 84) & 0x01000008 == 0x01000008
        assert g.get(back + 84) & 0x01000008 == 0x01000008
        assert g.half(fill + 88) in (1, 5)
        assert g.get(maximum + 84) == 0x02060007
        assert draw_order() == [plate, fill, back]
        check_rendered_fill((120 * value // 100) * 0.8)

    # Check both native monster gain and the Frontier contact callback.
    hit = actor + 12320
    g.cpu.hook_del(g.traps.pop(0x2A309C))
    g.cpu.hook_del(g.traps.pop(0x2864C4))
    skill = 0
    g.stub(0x2864C4, lambda _, candidate, *__: int(skill == candidate))

    # The corresponding stock contact still earns GU's 26 × 0.75 points.
    g.put(actor + 10112, 0)
    g.cpu.mem_write(hit + 87, bytes([26]))
    g.run(0x1720D4, 0, hit, target, 0, actor, 2)
    assert g.get(actor + 10112) == 19

    def vertical_slash():
        set_action(16, 0x8000)
        tick(0)
        tick(56)
        g.run(0x1720D4, 0, hit, target, 0, actor, 3)
        g.run(g.hooks[0x119F574], actor, hit, target, 1)

    for skill, expected in ((152, 33), (153, 22)):
        begin(0)
        g.put(actor + 10112, 0)
        for count in (1, 2):
            vertical_slash()
            assert g.get(actor + 10112) == expected * count
    skill = 0
    begin(0)
    g.put(actor + 10112, 0)
    vertical_slash()
    assert g.get(actor + 10112) == 28
    draw()
    assert 0 < cell() < 60 and draw_order() == [plate, fill, back]
    check_rendered_fill(26.4)
    for node in (plate, fill, back):
        g.put(node + 84, g.get(node + 84) | 0x00400000)
    draw()
    check_rendered_fill(26.4, -1)
    for node in (plate, fill, back):
        g.put(node + 84, g.get(node + 84) & ~0x00400000)
    vertical_slash()
    assert g.get(actor + 10112) == 56
    draw()
    check_rendered_fill(53.6)
    g.stub(0x2A309C, lambda *_: 0)
    g.cpu.hook_del(g.traps.pop(0x2864C4))
    g.stub(0x2864C4, lambda *_: 0)

    # The cut gauge keeps filling independently of changes to the spirit field.
    stages = []
    for _ in range(32):
        g.run(g.hooks[0x119F574], actor, actor + 12320, target, 1)
        draw()
        stages.append(cell())
    assert 0 < stages[0] < 60 and stages[-1] in range(60,76), stages
    set_action(1, 0)
    block("h", g.get(actor + 5060) + 800, 200)
    g.put(hit + 12, actor)
    g.put(hit + 48, 0x20)

    def attack_factor():
        g.run(0x169548, hit)
        bits = g.cpu.reg_read(arm_const.UC_ARM_REG_S0)
        return struct.unpack("<f", struct.pack("<I", bits))[0]

    assert attack_factor() == 2.
    g.stub(g.originals[0x119EE7C], lambda *_: 0)
    g.run(g.hooks[0x119EE7C], actor)
    assert abs(attack_factor() - 2.10) < 0.001
    sampled_shades = []
    for i in range(1,34):
        tick(i*2)
        draw()
        sampled_shades.append(check_rendered_fill(53.6))
        assert (cell()-60)//16 == min(i,23), (i,cell())
    assert len(set(stages)) > 4
    assert min(sampled_shades) == 32 and max(sampled_shades) == 255
    assert sampled_shades[0:13] == sampled_shades[20:33]
    # The Extreme style keeps the cut gauge across sheathing.
    drawn[0] = 0
    tick(0)
    draw()
    assert cell() >= 60, cell()
    drawn[0] = 1

    # Lose ownership of the HUD, then rebind the same widget.
    weapon_vt = g.get(actor)
    g.put(actor, 0x1749A88)
    draw()
    assert floats(plate+268,4) == stock and g.get(plate+260)==desc
    assert floats(fill+180,2) == (98,4) and floats(fill+128,2) == (20,10)
    assert struct.unpack("<4I",g.cpu.mem_read(fill+164,16)) == colors
    assert g.get(fill+160)&15 == 0 and g.get(back+84)&1
    assert draw_order() == [plate, fill, back]
    assert floats(back+180,2) == (98,4) and floats(back+128,2) == (20,10)
    assert floats(maximum+180,2) == (72.,32.)
    g.put(actor, weapon_vt)
    draw()
    assert g.get(plate+256) == 0x40000003
    g.put(0x2126A64, previous_manager)
    g.put(0x212668C, previous_vertex_manager)
    g.cpu.hook_del(reset_check)
    for helper in (*math_imports, 0xA4EB9C, 0xA3E3B0, 0x541624, 0x541844, 0x542AA0,
                   0x8826FC, 0x882760, 0x93EFF4):
        g.cpu.hook_del(g.traps.pop(helper))
    print("GU LS HUD: two strikes, Focus/Slow, no duplicate spirit gain, rendered fill and stock restoration passed")
