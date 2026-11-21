"""Replay ground inputs through the retail w08 mappers and idle command rows."""
import struct
import zlib

from unicorn import arm_const as arm

from verify_style import (
    CHANGE_ACTION, COMMAND_UPDATE, DISPATCH, WEAPON_DISPATCH, boot, hunters, load_registered_style,
    quest_entries, setup_controller,
)


def replay(repo, root):
    g = boot((repo / "mhgu/main.elf").read_bytes(), b"[Weapons]\n8=4\n", ("registry",))
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

    def change(_, group, slot, flags):
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

    from verify_chains import replay as verify_chains
    verify_chains(g, root, actor, form, weapon, set_action)

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

    def draw(current, dash=0, *, key=0x120, pressed=True):
        edge = key if pressed else 0
        set_action(*current)
        g.cpu.mem_write(actor + 10057, b"\0")
        g.put(actor + 5188, 0)
        g.cpu.mem_write(actor + 5140, bytes(32))
        g.cpu.mem_write(actor + 5108, struct.pack("<3H", key | dash, 0, edge))
        g.put(actor + 5124, edge)
        if dash:
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
        assert draw((2, 2), dash, pressed=False) == (2, 2)
        for key in (0x20, 0x100, 0x1000):
            assert draw((2, 2), dash, key=key) == (2, 2)
        assert draw((2, 2), dash) == (16, 0x8000 + 4)
    print("GU light-sword draw: X+A in idle, walk and both sprint layouts, unlock, kick and airborne rejection passed.")
