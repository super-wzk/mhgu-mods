"""Replay ground inputs through the retail w08 mappers and idle command rows."""
import re
import struct
import zlib

from unicorn import arm_const as arm

from verify_style import (
    BLOCK, CHANGE_ACTION, COMMAND_BIND, COMMAND_UPDATE, DISPATCH, ENCODE, WEAPON_DISPATCH,
    boot, hunters, load_registered_style,
    quest_entries, setup_controller,
)


def roll_chains(g, actor, set_action, form):
    native = {name: zlib.decompress(raw) for name, kind, raw in quest_entries("w08_m0.arc")
              if kind in (0x4B4C2BD3, 0x5139901D)}
    data = native[b"player\\cmd\\w08\\plcmd_w08_guild"]
    refs = list(re.finditer(rb"rPlBaseCmd\0(player[\\a-zA-Z\d_]+)\0", data))
    table_list, tables = g.alloc(120), g.alloc(27 * 4)
    g.put(table_list + 104, 27)
    g.put(table_list + 116, tables)
    for index, rows, stock_slot in ((25, refs[183:189], 41), (26, refs[189:195], 72)):
        table, entries = g.alloc(24), g.alloc(4 * len(rows))
        g.cpu.mem_write(table, struct.pack("<6I", 0, 0, len(rows), len(rows), 0, entries))
        g.put(tables + 4 * index, table)
        for i, row in enumerate(rows):
            raw = native[row[1]]
            fields = [struct.unpack_from("<H", raw, off)[0] for off in (504, 510, 516, 522)]
            params = [struct.unpack_from("<I", raw, off)[0] for off in (528, 536, 544, 552)]
            if i == 1:
                assert fields[:2] == [16, stock_slot]
            times = struct.unpack_from("<IfIfIf", data, row.end())[1::2]
            entry, resource = g.alloc(24), g.alloc(124)
            g.cpu.mem_write(entry, struct.pack("<3I3f", 0, 1, resource, *times))
            g.cpu.mem_write(resource + 100, struct.pack("<4H4I", *fields, *params))
            g.put(entries + 4 * i, entry)
    for fn in (COMMAND_BIND, 0x27F5D0):
        g.cpu.hook_del(g.traps.pop(fn))
    g.stub(0x285F68, lambda *_: table_list)
    g.stub(0x2A81DC, lambda *_: 0)  # level terrain
    previous = 0

    def number(off, value):
        g.cpu.mem_write(actor + off, struct.pack("<f", value))

    def current():
        return g.half(actor + 9478), g.half(actor + 9482)

    def action(state):
        return 16, 0x8000 + state

    def tick(frame, held=0, *, side=False, flick=False, ended=False):
        nonlocal previous
        g.cpu.mem_write(actor + 5108, struct.pack("<3H", held, previous & ~held, held & ~previous))
        previous = held
        strength = 100 if flick else 64 if side else 0
        g.cpu.mem_write(actor + 5132, struct.pack("<H2xh", 0x8000, strength))
        g.cpu.mem_write(actor + 5140, bytes(32))
        g.cpu.mem_write(actor + 1276, struct.pack("<2f", frame + 1, frame))
        g.cpu.mem_write(actor + 1206, bytes([4 if ended else 0]))
        number(5332, frame)
        g.run(0x27CA28, actor)
        g.run(0x11A03A8, actor)
        g.run(COMMAND_UPDATE, actor)
        group, slot = current()
        if group != 16:
            g.run(DISPATCH, actor)
        elif slot >= 0x8000:
            g.run(g.hooks[WEAPON_DISPATCH], actor)
        return current()

    def roll(button, state, *, sword=True, press_frame=40, flick=False):
        nonlocal previous
        form(sword)
        g.put(actor + 13100, 100)
        g.put(actor + 4076, 0)
        g.put(actor + 5280, 0)
        set_action(1, 6)
        g.cpu.mem_write(actor + 5, b"\x02")
        g.cpu.mem_write(actor + 1204, struct.pack("<H", g.run(ENCODE, actor, 28)))
        g.cpu.mem_write(actor + 5188, bytes(8))
        g.put(actor + 5328, 0)
        number(1268, 0)
        number(1284, 58)  # co_0[28] has 59 samples
        previous = 0
        g.run(0x119FE44, actor)
        assert g.get(actor + 9388) == (2025 if sword else 2026)
        assert g.get(g.get(actor + 9384) + 8) == 5
        for frame in range(0, 52, 2):
            expected = (1, 6) if frame < 50 or state is None else action(state)
            held = button if frame >= press_frame else 0
            assert tick(frame, held, flick=flick and frame in (20, 24)) == expected, (
                sword, button, frame, current(), expected,
            )
        number(1284, 180)

    # The X held through the roll must not mask the subsequent directional chord.
    roll(0x20, 7)
    assert tick(54, 0x20) == action(7)
    observed = tick(66, 0x120, side=True)
    assert observed == action(153), (observed, g.get(actor + 5188), g.get(actor + 5156))
    roll(0x20, 7)
    tick(54)
    assert tick(66, 0x160, side=True) == action(153), current()
    for side, target in ((False, 70), (True, 153)):
        roll(0x20, 7)
        tick(54)
        for f in (66, 68, 70):
            tick(f, 0x20)
        assert current() == action(29)
        tick(26, ended=True)  # Advance from clip 1430 to the second part of the slash.
        tick(44)
        assert tick(56, 0x120, side=side) == action(target), (side, current())

    def double_slash():
        roll(0x100, 88)
        assert tick(14, ended=True) == action(88)  # Roll follow-up clip 1467 ends.
        tick(80)
        for f in (92, 94, 96):
            assert tick(f, 0x100) == action(88), (f, current())
        assert tick(122) == action(95), (current(), g.get(actor + 13096))

    double_slash()
    assert tick(60, 0x120, side=True) == action(95)
    assert tick(92) == action(95)
    assert tick(110) == action(153)
    double_slash()
    assert tick(60, 0x120) == action(95)
    assert tick(92) == action(98)
    assert tick(26, 0x120, side=True) == action(153)

    roll(0x20, 20, sword=False)
    roll(0x100, 28, sword=False)
    for sword, guard, sprint in ((False, 89, 103), (True, 91, 148)):
        roll(0x800, None, sword=sword, press_frame=48)  # GP uses a fresh press at 50.
        roll(0x800, guard, sword=sword, press_frame=50)
        roll(0, sprint, sword=sword, flick=True)
        roll(0x800, guard, sword=sword, press_frame=50, flick=True)
        roll(0x40, None if sword else 111, sword=sword)
    roll(0x20, 20, sword=False, flick=True)
    g.stub(COMMAND_BIND, lambda *_: 0)
    print("GU w08 forward roll: attack chains, absorption, axe charge and sprint priority passed.")


def replay(repo, root):
    g = boot((repo / "mhgu/main.elf").read_bytes(), b"[Weapons]\n8=13\n", ("registry",))
    plugin = root / "dist/switch-axe-extreme/romfs/nativeNX/plugins/switch_axe_extreme.nro"
    desc = load_registered_style(g, plugin)
    actor, eq, _ = hunters(g)
    set_action = setup_controller(g, actor)
    g.cpu.mem_write(eq + 1236, b"\x08")
    g.put(actor, 0x174D838)
    g.stub(0x2A4324, lambda *_: 0)
    g.run(0x11A8BBC, actor, 0)  # initialize the native w08 slash gauge
    g.put(actor + 240, g.alloc(128))
    g.put(0x2126504, g.alloc(128))  # no active camera in the neutral roll scene
    g.put(actor + 5068, 1)
    g.put(actor + 1188, 1)
    g.cpu.mem_write(actor + 5078, struct.pack("<h", 100))
    g.cpu.mem_write(actor + 28, struct.pack("<f", 2))
    g.cpu.mem_write(actor + 10057, b"\x01")
    g.cpu.mem_write(actor + 5088, struct.pack("<f", 900))
    g.cpu.mem_write(actor + 1284, struct.pack("<f", 180))
    inputs = g.alloc(40)
    controller = g.get(desc + 20)

    def frame(value):
        g.cpu.mem_write(actor + 1280, struct.pack("<f", value))
        g.run(controller, actor, inputs)

    defer_change = False

    def defer_transition():
        nonlocal defer_change
        defer_change = True

    def change(_, group, slot, flags):
        nonlocal defer_change
        if defer_change:
            defer_change = False
            return 0
        set_action(group, slot)
        g.cpu.mem_write(actor + 9384, bytes(64))
        return 1

    g.stub(g.originals[CHANGE_ACTION], change)
    g.stub(0x2B6E5C, lambda *_: set_action(1, 0))
    for fn in (0x27D5C4, 0x3A5ED0, 0x5584A0, 0x27FE18):
        g.stub(fn, lambda *_: 0)
    g.cpu.hook_del(g.traps.pop(0x27E608))
    # Each monster absorption awards 75 via its authored event.
    for _ in range(2):
        set_action(16, 0x8000 + 90)
        frame(0)
        frame(8)
    set_action(1, 0)
    frame(0)
    g.cpu.hook_del(g.traps.pop(COMMAND_UPDATE))
    for offset in (752, 756, 792, 796, 808, 812):
        g.stub(g.get(g.get(actor) + offset), lambda *_: 0)

    rows = {
        name.rsplit(b"\\", 1)[-1]: zlib.decompress(raw)
        for name, kind, raw in quest_entries("w08_m0.arc")
        if kind == 0x5139901D
    }
    table, entries, entry, resource = [g.alloc(n) for n in (24, 4, 24, 124)]
    g.cpu.mem_write(table, struct.pack("<6I", 0, 0, 1, 1, 0, entries))
    g.put(entries, entry)
    g.cpu.mem_write(entry, struct.pack("<3I3f", 0, 1, resource, 0, 0, 999))

    g.stub(g.originals[DISPATCH], lambda *_: 0)

    def press(button, row_name, expected, *, stick=0):
        row = rows[row_name.encode()]
        fields = [struct.unpack_from("<H", row, at)[0] for at in (504, 510, 516, 522)]
        params = [struct.unpack_from("<I", row, at)[0] for at in (528, 536, 544, 552)]
        g.cpu.mem_write(resource + 100, struct.pack("<4H4I", *fields, *params))
        set_action(1, 0)
        g.cpu.mem_write(actor + 5, b"\x01")
        g.put(actor + 9384, table)
        g.cpu.mem_write(actor + 9392, bytes(48))
        g.cpu.mem_write(actor + 5136, struct.pack("<h", stick))
        g.put(actor + 5188, 0)
        for edge in (button, 0, 0):
            g.cpu.mem_write(actor + 5140, bytes(32))
            g.cpu.mem_write(actor + 5108, struct.pack("<HHH", button, 0, edge))
            g.run(0x27CA28, actor)
            g.run(0x11A03A8, actor)
            g.run(COMMAND_UPDATE, actor)
            g.run(DISPATCH, actor)
            current = (g.half(actor + 9478), g.half(actor + 9482))
            if current != (1, 0):
                break
        assert current == expected, (hex(button), row_name, stick, expected, current)
        g.cpu.mem_write(actor + 5108, bytes(6))
        g.cpu.mem_write(actor + 5140, bytes(32))

    def weapon(button, row, state, **kwargs):
        press(button, "plcmd_w08_0x0010_" + row, (16, 0x8000 + state), **kwargs)

    weapon(0x1000, "003a", 94)
    frame(124)
    frame(180)
    weapon(0x1000, "019a", 31)
    frame(180)
    weapon(0x1000, "003a", 30)
    frame(180)

    def form(sword):
        set_action(16, 0x8000 + (30 if sword else 31))
        frame(0)
        frame(180)

    for sword, x, moving_x, a, chord, charge, guard in (
        (False, 0, 3, 1, 2, 111, 89), (True, 5, 17, 78, 70, 162, 91),
    ):
        for button, row, state, stick in (
            (0x20, "005b" if sword else "002a", x, 0),
            (0x20, "005a" if sword else "004a", moving_x, 120),
            (0x100, "019a" if sword else "007a", a, 0),
            (0x120, "020a" if sword else "003a", chord, 0),
            (0x40, "000b" if sword else "001a", charge, 0),
            (0x800, "000b" if sword else "001a", guard, 0),
        ):
            form(sword)
            weapon(button, row, state, stick=stick)
    form(False)
    press(0x80, "plcmd_com_0x0001_006d", (1, 6))

    roll_chains(g, actor, set_action, form)

    from verify_chains import replay as verify_chains
    verify_chains(g, root, actor, form, weapon, set_action, defer_transition)

    layout = g.get(0x1897F78) + 17518
    g.cpu.mem_write(layout, b"\x01")
    form(False)
    weapon(0x800, "001a", 111)
    form(False)
    weapon(0x40, "001a", 89)

    for held, expected in ((0x800, 106), (0x20, 123)):
        set_action(16, 0x8000 + 106)
        frame(0)
        g.cpu.mem_write(actor + 5108, struct.pack("<H", held))
        frame(39)
        assert g.half(actor + 9482) == 0x8000 + expected, (held, g.half(actor + 9482))
    # A locked, partial meter cannot turn the special input into a stock attack.
    g.run(controller, actor, 0)
    set_action(1, 0)
    press(0x1000, "plcmd_w08_0x0010_003a", (1, 0))

    def enter(state):
        set_action(16, 0x8000 + state)
        frame(0)

    def advance(value, *, special=False):
        button = 0x1000 if special else 0
        g.cpu.mem_write(actor + 5108, struct.pack("<HHH", button, 0, button))
        g.cpu.mem_write(actor + 5140, bytes(32))
        g.run(0x27CA28, actor)
        g.run(0x11A03A8, actor)
        g.cpu.mem_write(actor + 1280, struct.pack("<f", value))
        g.run(g.hooks[WEAPON_DISPATCH], actor)
        return g.half(actor + 9482) - 0x8000

    enter(0)
    assert advance(74, special=True) == 0
    for _ in range(2):
        enter(90)
        frame(8)
    # The full-meter absorption route buffers at frame 10 and unlocks at 20.
    assert advance(10, special=True) == 90
    assert advance(19) == 90
    assert advance(20) == 146
    frame(180)
    frame(124)
    assert g.half(actor + 9482) == 0x8000 + 110
    frame(180)

    for sword, start, early, execute, target in (
        (False, 0, 64, 74, 174),  # Axe vertical slash to light-sword morph slash.
        (True, 5, 38, 48, 64),   # Light-sword vertical slash to axe morph slash.
        (False, 90, 10, 30, 147),  # Absorption after the light sword is unlocked.
    ):
        form(sword)
        enter(start)
        assert advance(early - 2, special=True) == start
        assert advance(early - 1) == start
        assert advance(early, special=True) == start
        assert advance(execute - 1) == start
        assert advance(execute) == target
    print("GU w08 inputs: normal attacks, charge, roll, and special-key idle, buffered morph and absorption routes passed.")

    # Use GU's drawn flag and replay the sprint filter before the input mappers.
    g.cpu.hook_del(g.traps.pop(0x2A2444))
    g.cpu.hook_del(g.traps.pop(0x2A2560))

    def draw(current, dash=0, *, key=0x120, pressed=True, filter_sprint=True, stick=0):
        edge = key if pressed else 0
        set_action(*current)
        running = current in ((2, 2), (2, 13), (2, 14), (2, 15))
        g.cpu.mem_write(actor + 10057, bytes([2 if running else 0]))
        g.put(actor + 9488, 0x181 if running else 0)
        if current in ((2, 13), (2, 15)):
            g.put(actor + 9488, 0x8181)
        g.put(actor + 5188, 0)
        g.cpu.mem_write(actor + 5136, struct.pack("<h", stick))
        g.cpu.mem_write(actor + 5140, bytes(32))
        g.cpu.mem_write(actor + 5108, struct.pack("<3H", key | dash, 0, edge))
        g.put(actor + 5124, edge)
        if dash and filter_sprint:
            g.put(actor + 10096, 1)
            for reg, value in (
                (arm.UC_ARM_REG_R4, actor), (arm.UC_ARM_REG_R6, actor + 5112),
                (arm.UC_ARM_REG_R8, 5114), (arm.UC_ARM_REG_R11, actor + 5108),
                (arm.UC_ARM_REG_R9, 0), (arm.UC_ARM_REG_R10, actor + 5110),
            ):
                g.cpu.reg_write(reg, value)
            g.run(0x2A7D68, until=0x2A7DC0)
            assert g.half(actor + 5112) == 0
            g.put(actor + 10096, 0)
        g.run(0x27CA28, actor)
        g.run(0x11A03A8, actor)
        g.run(DISPATCH, actor)
        return g.half(actor + 9478), g.half(actor + 9482)

    g.run(controller, actor, 0)
    assert draw((2, 1)) == (2, 1)  # Locked and below full.
    for _ in range(2):
        enter(90)
        frame(8)
    assert draw((2, 1), key=0x1000) == (2, 1)  # The kick does not unlock the blade.
    assert draw((2, 1)) == (16, 0x8000 + 93)
    g.cpu.mem_write(actor + 1206, b"\x04")
    frame(108)  # End the draw opener so the unlock phase can run.
    g.cpu.mem_write(actor + 1206, b"\0")
    frame(124)
    frame(126)
    assert g.half(actor + 9482) == 0x8000 + 110
    frame(180)
    assert draw((4, 14)) == (4, 14)  # Airborne cannot draw the light sword.
    for current in ((1, 0), (2, 0), (2, 1), (2, 3), (2, 5)):
        assert draw(current, key=0x1000) == current
        assert draw(current) == (16, 0x8000 + 4), current
    for shoulder_layout, dash in ((0, 0x40), (1, 0x800)):
        g.cpu.mem_write(layout, bytes([shoulder_layout]))
        assert draw((1, 0), key=dash) == (16, 0x8000 + 111)
        assert draw((1, 0), key=dash, stick=49) == (16, 0x8000 + 111)
        assert draw((1, 0), key=dash, stick=50) == (1, 0)
        assert draw((1, 0), key=dash, pressed=False) == (1, 0)
        for current in ((1, 2), (2, 0), (2, 1), (2, 2), (2, 13)):
            for pressed in (True, False):
                assert draw(current, key=dash, pressed=pressed) == current, (
                    current, shoulder_layout, pressed,
                )
        for key in (0x20, 0x100, 0x1000):
            assert draw((2, 2), dash, key=key) == (2, 2)
        assert draw((2, 2), dash) == (16, 0x8000 + 4)
    print("GU light-sword draw: X+A in idle, walk and both sprint layouts, unlock, kick and airborne rejection passed.")
    print("GU draw charge: a fresh R press in sheathed idle, both shoulder layouts and native stick deadzone passed.")

    for shoulder_layout, dash, gp in ((0, 0x40, 0x800), (1, 0x800, 0x40)):
        g.cpu.mem_write(layout, bytes([shoulder_layout]))
        for current in ((2, 2), (2, 13), (2, 14), (2, 15)):
            assert draw(current, dash, key=gp, pressed=False) == current
            for filtered in (False, True):
                assert draw(current, dash, key=gp, filter_sprint=filtered) == (16, 0x8000 + 89), (
                    current, shoulder_layout, filtered,
                )
                assert g.half(actor + 1204) == 1471
                assert g.run(BLOCK, actor, g.half(actor + 5280)) == 1
        for current in ((1, 4), (4, 14)):
            assert draw(current, dash, key=gp, filter_sprint=False) == current
    print("GU draw-absorption: regular and panic sprint with both shoulder layouts and native guard passed.")
