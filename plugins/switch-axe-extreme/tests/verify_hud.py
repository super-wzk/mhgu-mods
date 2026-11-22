"""Replay quest-entry HUD updates with GU 1.4 shape setters and the packaged NRO."""
import struct


def replay(g, actor, leave_style):
    def put(address, fmt, *values):
        g.cpu.mem_write(address, struct.pack("<" + fmt, *values))

    def floats(address, n):
        return struct.unpack("<" + str(n) + "f", g.cpu.mem_read(address, n * 4))

    widget, layout, table = [g.alloc(n) for n in (200, 680, 12)]
    groups = [g.alloc(240) for _ in range(3)]
    g.put(widget + 36, layout)
    g.put(layout + 248, table)
    g.put(layout + 664, 3)
    put(table, "3I", *groups)
    stock_flags = (0x02060003, 0x02060000, 0x02060003)
    for group, flags in zip(groups, stock_flags):
        g.put(group + 84, flags)

    gui, directory, descriptions = [g.alloc(n) for n in (128, 256, 128)]
    manager, manager_vt, load, atlas = [g.alloc(n) for n in (32, 64, 4, 240)]
    g.put(gui + 100, directory)
    g.put(directory + 232, descriptions)
    g.put(directory + 112, 2)
    g.put(manager, manager_vt)
    g.put(manager_vt + 48, load)
    original_manager = g.get(0x2126A64)
    g.put(0x2126A64, manager)
    put(atlas + 220, "2I", 3072, 1024)
    loads = []
    def load_atlas(_, kind, name, flags):
        assert (kind, g.string(name), flags) == (0x21255d0, r"GUI\frontier_sa\gauge", 2)
        loads.append(1)
        return 0 if len(loads) == 2 else atlas
    g.stub(load, load_atlas)
    g.stub(0x887660, lambda *_: 0)
    g.put(0x212668C, g.alloc(192))

    sprites, polygons, member_tables = [], [], []
    crop = (368., 32., 496., 56.)
    for i in range(2):
        desc = descriptions + 64 * i
        sprite, back, fill, children = [g.alloc(n) for n in (304, 256, 256, 16)]
        sprites.append(sprite)
        polygons.append([back, fill])
        member_tables.append(children)
        g.put(groups[i] + 216, children)
        put(children + 4, "3I", back, fill, sprite)
        g.put(sprite, (0x176C7C4, 0x178FCC8)[i])
        g.put(sprite + 120, gui)
        g.put(desc, 23 + i)
        g.put(desc + 40, atlas)
        put(sprite + 256, "2I", 23 + i, desc)
        put(sprite + 268, "4f", *crop)
        put(sprite + 128, "2f", 2., 4.)
        put(sprite + 180, "2f", 128., 24.)
        for node in (back, fill):
            g.put(node, (0x176C744, 0x178FC48)[i])
            g.put(node + 84, 0x02060003)
            put(node + 180, "2f", 58., 4.)

    # Observe the entry that recursed into a non-object in the quest log.
    in_native = False
    resets = []

    def recursive_reset(node, *_):
        assert in_native and node in groups, "overlay invoked GU's recursive visibility reset"
        resets.append(node)

    g.stub(0xA344F4, recursive_reset)

    def stock():
        assert tuple(g.get(group + 84) for group in groups) == stock_flags
        for i, sprite in enumerate(sprites):
            assert (g.get(sprite + 256), g.get(sprite + 260)) == (23 + i, descriptions + 64 * i)
            assert floats(sprite + 268, 4) == crop
            assert floats(sprite + 128, 2) == (2., 4.)
            assert floats(sprite + 180, 2) == (128., 24.)
            for node in polygons[i]:
                assert floats(node + 180, 2) == (58., 4.)

    # Execute GU's actual 64AEFC -> 64AF2C -> 54533C mode selection and
    # visibility writes. Substitute the engine's widget and animation callbacks.
    native_calls = []
    sword_mode = False

    def begin_native(widget_arg, *_):
        nonlocal in_native
        assert widget_arg == widget and not in_native
        stock()
        in_native = True
        native_calls.append(1)

    def end_native(*_):
        nonlocal in_native, stock_flags
        bits = (0, 3, 0) if sword_mode else (3, 0, 3)
        stock_flags = tuple(0x02060000 | n for n in bits)
        assert tuple(g.get(group + 84) for group in groups) == stock_flags
        in_native = False
        return 57

    g.stub(0x6578F8, begin_native)
    g.stub(0x56E538, lambda *_: 0)
    g.stub(0x54E38C, end_native)
    g.stub(0x2B4EB8, lambda *_: 0)
    g.stub(0x2B4E94, lambda *_: int(sword_mode))
    g.stub(0x269A38, lambda *_: 1)
    g.stub(0x2B4E70, lambda *_: 50)
    for gauge in (widget + 100, widget + 148):
        put(gauge + 24, "2f", 100., 100.)

    def draw():
        count = len(native_calls)
        assert g.run(g.hooks[0x64AEFC], widget) == 57
        assert len(native_calls) == count + 1

    def custom():
        assert tuple(g.get(group + 84) & 3 for group in groups) == (3, 3, 0)
        for i, sprite in enumerate(sprites):
            assert g.get(sprite + 256) == 0x40000400 + i
            assert g.get(g.get(sprite + 260) + 40) == atlas
            assert floats(sprite + 128, 2) == (2. + 104. * i, 4.)
            assert floats(sprite + 180, 2) == (192., 32.)
            left, top, right, bottom = floats(sprite + 268, 4)
            assert right - left == 192 and bottom - top == 32
            cell = int(top // 32) * 16 + int(left // 192)
            assert cell in (range(101) if i == 0 else range(101, 404))
            for node in polygons[i]:
                assert floats(node + 180, 2) == (0., 0.)

    # First frame has a pending atlas. Next, only gauge zero binds: revert
    # that bind before returning the stock frame. Both bind on the third draw.
    draw()
    stock()
    g.put(atlas + 80, 1)
    draw()
    stock()
    assert len(loads) == 2
    draw()
    custom()
    assert not resets
    # A GU gauge change reaches the next HUD draw without a style tick.
    saved_slash = g.get(actor + 13100)
    for slash in (23, 86):
        g.put(actor + 13100, slash)
        draw()
        left, top, _, _ = floats(sprites[0] + 268, 4)
        assert int(top // 32) * 16 + int(left // 192) == slash
    g.put(actor + 13100, saved_slash)
    sword_mode = True
    draw()
    custom()
    assert resets == [groups[0], groups[2]]
    draw()
    custom()
    assert len(resets) == 2
    # GU replaces a polygon while the overlay is visible. Restore the surviving
    # shapes, leaving the detached object untouched and picking up the new leaf.
    removed = polygons[1][0]
    new = g.alloc(256)
    g.put(new, 0x178FC48)
    put(new + 180, "2f", 58., 4.)
    polygons[1][0] = new
    put(member_tables[1] + 4, "3I", sprites[1], polygons[1][1], new)
    draw()
    custom()
    assert floats(removed + 180, 2) == (0., 0.)

    # Wrong leaf type: the entire overlay must fall back to the stock HUD.
    g.put(member_tables[0] + 4, g.alloc(256))
    # The removed leaf stays untouched. It is no longer part of the stock HUD.
    detached = polygons[0].pop(0)
    draw()
    stock()
    assert floats(detached + 180, 2) == (0., 0.)
    put(detached + 180, "2f", 58., 4.)
    polygons[0].insert(0, detached)
    g.put(member_tables[0] + 4, detached)
    draw()
    custom()
    leave_style()
    draw()
    stock()
    g.put(0x2126A64, original_manager)
    print("GU HUD: quest entry, asset readiness, native mode switch, member replacement and style exit passed.")
