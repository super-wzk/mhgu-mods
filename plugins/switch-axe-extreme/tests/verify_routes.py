"""Exercise the released sprint/terrain controller and the retail w08 art routes."""

import os
from pathlib import Path
import struct
import zlib
from unicorn.arm_const import UC_ARM_REG_S1
from verify_style import (
    DISPATCH,
    SA_DRAW_ROUTE,
    boot,
    hunters,
    load_registered_style,
    quest_entries,
    setup_controller,
)


def replay(repo, root):
    g = boot((repo / "mhgu/main.elf").read_bytes(), b"[Weapons]\n8=4\n", ("registry",))
    g.inline_hooks = g.inline_hooks | {0x308B3C}
    desc = load_registered_style(
        g,
        root / "dist/switch-axe-extreme/romfs/nativeNX/plugins/switch_axe_extreme.nro",
    )
    actor, eq, _ = hunters(g)
    g.cpu.mem_write(eq + 1236, b"\x08")
    g.put(actor, 0x174D838)
    g.put(actor + 240, g.alloc(128))
    set_action = setup_controller(g, actor)
    g.stub(0x2A4324, lambda *_: 0)  # hunter equipment already supplied
    g.run(0x11A8BBC, actor, 0)  # GU initializes the w08 slash gauge
    inp = g.alloc(40)
    tick = g.get(desc + 20)

    def number(p, v):
        g.cpu.mem_write(p, struct.pack("<f", v))

    def half(p, v):
        g.cpu.mem_write(p, struct.pack("<H", v))

    def stub(a, fn):
        if a in g.traps:
            g.cpu.hook_del(g.traps.pop(a))
        g.stub(a, fn)

    def state():
        return (g.half(actor + 9478), g.half(actor + 9482))

    def frame(f, dt=2):
        number(actor + 28, dt)
        number(actor + 1280, f)
        g.run(tick, actor, inp)

    def enter(s):
        set_action(16, 0x8000 + s)
        frame(0, 0)

    def play(hunter, _, clip, __):
        half(hunter + 1204, clip & 0xFFFF)
        start = struct.pack("<I", g.cpu.reg_read(UC_ARM_REG_S1))
        starts.append(struct.unpack("<f", start)[0])
        return 1

    number(actor + 5088, 900)
    number(actor + 1284, 260)
    flags = []
    tables = []
    starts = []
    stub(0x53A06C, play)
    stub(0x283238, lambda _, __, bits, ___: flags.append(bits) or 0)
    stub(0x2E9B7C, lambda _, table, *__: tables.append(table) or 0)
    g.stub(0x2B6E5C, lambda *_: set_action(1, 0) or 0)
    turns = []
    stub(0x2A8458, lambda *_: turns.append(g.half(actor + 4076)) or 1)
    stub(0x2831E0, lambda _, group, slot, flags: set_action(group, slot) or 1)

    # Use the selected style's draw route and the actual double push detector.
    # The axe sprint's weapon track shows the extended blade after frame 30.
    # A normal damage interruption must still leave the hunter in axe form.
    drawn = False
    stub(0x2A2444, lambda *_: int(drawn))
    stock_draw = g.alloc(6)
    g.cpu.mem_write(stock_draw, struct.pack("<3H", 16, 14, 0))
    stub(g.originals[SA_DRAW_ROUTE], lambda *_: stock_draw)
    set_action(1, 0)
    draw = g.run(SA_DRAW_ROUTE, actor, 0)
    assert (g.half(draw), g.half(draw + 2)) == (16, 0x8000 + 10)
    enter(10)
    drawn = True
    frame(180)
    set_action(1, 0)
    g.cpu.hook_del(g.traps.pop(0x27E608))
    g.cpu.hook_del(g.traps.pop(0x27F5D0))
    g.put(0x2126504, g.alloc(96))  # GU resolves stick yaw without a camera override.

    def stick(strength, yaw):
        angle = (yaw + 0x4000) & 0xFFFF
        half(inp + 32, angle)
        half(actor + 5132, angle)
        half(inp + 34, strength)
        half(actor + 5136, strength)

    def flick(yaw=0, first=0):
        for i, strength in enumerate((0, 100, 0, 100)):
            stick(strength, yaw)
            frame(first + 2 * i)

    assert g.get(actor + 13100) == 50
    flick(0x4000)
    assert state() == (16, 0x8000 + 103) and g.get(actor + 13100) == 44
    idle_facing = g.get(actor + 4076), g.get(actor + 5280)
    frame(80)
    assert g.get(actor + 13096) == 1 and g.get(actor + 13100) == 42
    set_action(64, 6)
    frame(1)
    set_action(1, 0)
    flick()
    assert state() == (16, 0x8000 + 103), state()
    assert g.get(actor + 13100) == 36
    frame(80)
    assert g.get(actor + 13100) == 34

    # Buffer during recovery, then sample the stick on the actual branch frame.
    for attack, first, execute, sprint in ((0, 40, 80, 103), (5, 30, 56, 148)):
        for yaw, strength in ((0x6000, 100), (0xE000, 100), (0xA000, 100), (0, 49)):
            g.run(tick, 0, 0)
            g.run(0x11A8BBC, actor, 0)
            g.put(actor + 4076, 0x2000)
            g.put(actor + 5280, 0x2000)
            enter(attack)
            flick(0x2000, first)
            assert state() == (16, 0x8000 + attack)
            stick(strength, yaw)
            frame(execute - 1)
            assert state() == (16, 0x8000 + attack) and g.get(actor + 4076) == 0x2000
            frame(execute)
            assert state() == (16, 0x8000 + sprint)
            facing = yaw if strength >= 50 else 0x2000
            assert g.get(actor + 4076) == g.get(actor + 5280) == facing, (attack, yaw, strength)
    assert idle_facing == (0x4000, 0x4000)
    half(inp + 34, 0)
    half(actor + 5136, 0)
    stub(0x27E608, lambda *_: 0)
    g.run(tick, 0, 0)
    g.run(0x11A8BBC, actor, 0)

    for s, yaw in ((36, 0xC000), (37, 0x4000), (119, 0xC000), (120, 0x4000)):
        half(actor + 4076, 0)
        enter(s)
        frame(4)
        assert state() == (4, 14) and turns[-1] == yaw
    for s in (103, 148):
        enter(s)
        assert flags[-1] == 0x1180 and tables[-1] == 2000
        frame(50, 50)
        assert state() == (16, s + 0x8000)
        frame(55, 5)
        assert state() == (16, 0x8000 + (105 if s == 103 else 151))

    from verify_branches import Frontier
    reference = Frontier(repo / "mhfz/eboot.bin.elf")
    for sprint in (103, 148):
        for target in (0, 500, 850, 910, -500, -850, 0x4000, -0x4000):
            for updates in ((2,), (1, 1)):
                stub(0x27E608, lambda *_: 1)
                stub(0x27F5D0, lambda *_: target & 0xffff)
                g.put(actor + 13100, 100)
                enter(sprint)
                frame(60, 52)
                g.put(actor + 4076, 0)
                g.put(actor + 5280, 0)
                for dt in updates:
                    frame(62, dt)
                facing = reference.sprint_facing(target)
                assert (g.half(actor + 4076), g.half(actor + 5280)) == (facing, facing)
    stub(0x27E608, lambda *_: 0)
    g.cpu.hook_del(g.traps.pop(0x27F5D0))

    # Use GU's weapon model fallback to resolve the animation of an equipped w08.
    model = g.alloc(5160)
    g.stub(g.originals[DISPATCH], lambda *_: 0)  # the scene supplies the active reaction
    g.stub(0x53A058, lambda *_: 2133)  # a common clip with no w08 motion track
    g.stub(0x954640, lambda obj, layer, clip, _: half(obj + 1204 + 320 * layer, clip) or 1)
    for s, transient_sword, weapon_clip in ((127, 1, 1), (30, 0, 20)):
        enter(s)
        assert g.get(actor + 13096) == transient_sword
        for reaction in ((64, 6), (64, 7), (4, 14)):
            set_action(*reaction)
            half(actor + 1204, 0x85)
            number(actor + 1280, 2)
            g.run(g.hooks[DISPATCH], actor)
            g.run(0x308B3C, model, actor)
            layers = (g.half(model + 1204), g.half(model + 1524))
            assert layers == (weapon_clip, 256 + weapon_clip), (s, reaction, layers)
            assert state() == reaction and g.half(actor + 1204) == 0x85

    # FZ 812113BE restores the axe when 93/94/146 is interrupted and
    # commits both the sword form and unlock when it enters state 110.
    def interrupt_unlock():
        set_action(64, 6)
        half(actor + 1204, 0x85)
        number(actor + 1280, 2)
        g.run(g.hooks[DISPATCH], actor)
        g.run(0x308B3C, model, actor)
        assert state() == (64, 6)
        return g.half(model + 1204)

    for opener in (93, 94, 146):
        g.run(tick, 0, 0)
        for _ in range(2):
            enter(90)
            frame(8)
        enter(opener)
        if opener != 94:
            g.cpu.mem_write(actor + 1206, b"\x04")
            frame(180)
            g.cpu.mem_write(actor + 1206, b"\0")
            assert g.half(actor + 1204) == 1476
        frame(120)
        assert state() == (16, 0x8000 + opener) and g.get(actor + 13096) == 1
        assert interrupt_unlock() == 1, opener  # visible sword retracts on hit

        set_action(1, 0)
        g.put(inp + 16, 1 << 14)  # X uses the retained axe form
        frame(0)
        g.put(inp + 16, 0)
        assert state() == (16, 0x8000)

        set_action(1, 0)
        g.put(inp, 1 << 8)  # Special can retry with the retained full gauge
        frame(0)
        g.put(inp, 0)
        assert state() == (16, 0x8000 + 94)

    frame(124)
    assert state() == (16, 0x8000 + 110)
    assert interrupt_unlock() == 20  # the completed unlock survives the hit

    set_action(1, 0)
    g.put(inp, 1 << 8)
    frame(0)
    assert state() == (16, 0x8000 + 31)
    g.put(inp, 0)
    frame(180)
    set_action(1, 0)
    g.put(inp, 1 << 8)
    frame(0)
    g.put(inp, 0)
    assert state() == (16, 0x8000 + 30)  # full but already unlocked
    print("FZ/GU light-sword unlock: damage during each opener restores axe; completed unlock survives damage.")

    g.put(actor + 9120, model)
    stub(0x53A058, lambda p, *_: 32441 if g.half(p + 1204) == 1441 else 2133)
    weapon_dir = Path(os.environ["MHGU_ROMFS"]) / "nativeNX/arc/weapon/w08"
    model_banks = []
    for arc in sorted(weapon_dir.glob("g*.arc")):
        data = arc.read_bytes()
        count, = struct.unpack_from("<H", data, 6)
        for name, kind, size, _, offset in struct.iter_unpack("<64s4I", data[12:12 + 80 * count]):
            if kind != 0x76820D81 or b"_sa_" in name:
                continue
            lmt = zlib.decompress(data[offset:offset + size])
            count, = struct.unpack_from("<H", lmt, 6)
            if count == 0:  # g00 has no articulated weapon motion.
                continue
            poses = [struct.unpack_from("<I", lmt, 8 + 4 * slot)[0] for slot in (1, 20, 161)]
            assert all(poses) and struct.unpack_from("<I", lmt, poses[2] + 8)[0] == 15
            model_banks.append(lmt)
    assert model_banks
    lmt = model_banks[0]
    bank, table = g.alloc(104), g.alloc(len(lmt))
    g.cpu.mem_write(table, lmt)
    g.put(bank + 100, table)
    g.put(model + 3844, bank)
    assert g.run(0x953B10, model, 161) != 0 and g.run(0x953B10, model, 417) == 0
    enter(31)
    for f, clip, sword, track_frame in (
        (63, 20, 1, None), (64, 161, 0, 0.), (70, 161, 0, 7.),
        (75.5, 161, 0, 14 * 11.5 / 12), (76, 1, 0, None),
    ):
        frame(f)
        g.run(0x308B3C, model, actor)
        assert g.half(model + 1204) == clip and g.get(actor + 13096) == sword
        if track_frame is not None:
            assert g.half(model + 1524) == 0xffff and g.get(model + 1212) & 16
            actual, = struct.unpack("<f", g.cpu.mem_read(model + 1276, 4))
            assert abs(actual - track_frame) < 0.0001
    g.put(actor + 9120, 0)
    print("FZ/GU: both sprint forms at 30/60 Hz and weapon-track handoff at frames 64/76 passed.")
    for fn in (0x53A058, 0x954640):
        g.cpu.hook_del(g.traps.pop(fn))

    # State entry uses the NRO's normal controller; every subsequent authored
    # clip also needs a nonempty record in each shipped GU animation bank.
    program = (root / "target/assets/switch_axe.fsaa").read_bytes()
    clips = set()
    steps = 0
    active = 0
    for state_id in range(struct.unpack_from("<H", program, 6)[0]):
        count, offset = struct.unpack_from("<II", program, 16 + 24 * state_id)
        if not count:
            continue
        ids = [
            struct.unpack_from("<H", program, offset + 12 * j + 2)[0]
            for j in range(count)
        ]
        enter(state_id)
        assert g.half(actor + 1204) == ids[0], (state_id, ids[0], g.half(actor + 1204))
        active += 1
        steps += count
        clips.update(ids)
    folder = root / "dist/switch-axe-extreme/romfs/nativeNX/player/mot/frontier_sa"
    banks = {i: (folder / f"{i}.lmt").read_bytes() for i in (0, 2, 4)}
    for clip in clips:
        bank = banks[(clip - 1000) // 100]
        slot = clip % 100
        assert slot < struct.unpack_from("<H", bank, 6)[0]
        at = struct.unpack_from("<I", bank, 8 + 4 * slot)[0]
        assert 0 < at < len(bank) - 48, (clip, at)
    print(
        f"GU action package: {active} state entries, {steps} steps, {len(clips)} clips shared by both models."
    )
    # GU handles the climb and landing. Keep the stick held while native action
    # 1:138 owns the actor, then redispatch sprint in 2:30 without another flick.
    g.run(tick, 0, 0)
    g.run(0x11A8BBC, actor, 0)
    g.cpu.hook_del(g.traps.pop(0x27E608))
    half(inp + 34, 80)
    half(actor + 5136, 80)
    for s in (103, 105, 148, 151):
        enter(s)
        frame(18, 18)
        # GU's common sheathe command must work with the stick still held.
        g.put(inp, 1 << 13)
        frame(20)
        g.put(inp, 0)
        assert state() == (1, 2), (s, state())
    for sprint, terrain, resumed, clip, brake in (
        (103, ((1, 138),), 178, 1479, 105),
        (148, ((4, 14), (4, 3)), 179, 1055, 151),
    ):
        g.put(actor + 13100, 50 if sprint == 103 else 0)
        enter(sprint)
        assert starts[-1] == 0
        frame(85, 54)
        slash = g.get(actor + 13100)
        for native in terrain:
            set_action(*native)
            frame(1)
            assert state() == native
        set_action(2, 30)
        frame(1)
        assert state() == (16, 0x8000 + resumed), (sprint, state())
        assert (g.half(actor + 1204), starts[-1], g.get(actor + 13096)) == (clip, 90., 1)
        assert g.get(actor + 13100) == slash
        assert flags[-1] == 0x1180 and tables[-1] == 2000
        frame(120)
        frame(90)
        assert g.get(actor + 13100) == slash - (2 if sprint == 103 else 0)
        # Braking need not wait for the opening steer delay again.
        half(inp + 34, 0)
        half(actor + 5136, 0)
        frame(92)
        assert state() == (16, 0x8000 + brake), (sprint, state())
        half(inp + 34, 80)
        half(actor + 5136, 80)
    for slash, strength in ((0, 80), (50, 0)):
        g.put(actor + 13100, 100)
        enter(103)
        g.put(actor + 13100, slash)
        set_action(1, 138)
        frame(1)
        assert state() == (1, 138)
        half(inp + 34, strength)
        half(actor + 5136, strength)
        set_action(2, 30)
        frame(1)
        assert state() == (16, 0x8000 + 105), (slash, stick, state())
    # In a native art GU issues both flagged entries and unflagged continuations.
    # Exercise the registry's real slot resolver before the engine's state write.
    accepted = []
    stub(
        g.originals[0x2AB670],
        lambda _, group, slot, flags: (
            accepted.append((group, slot, flags)) or set_action(group, slot) or 1
        ),
    )
    arts = [
        struct.unpack_from("<H4xH", zlib.decompress(raw), 0x1F8)
        for name, kind, raw in quest_entries("w08_m.arc")
        if kind == 0x5139901D and b"\\sa\\" in name
    ]
    assert [s for _, s in arts] == [82] * 3 + [85, 86, 87, 88, 89, 90] + [118] * 3
    for group, slot in arts:
        g.run(g.hooks[0x2AB670], actor, group, slot, 0x40)
        assert accepted[-1] == (16, slot, 0x40)
    for slot in (97, 98, 99, 101, 102, 103, 137, 138, 139):
        g.run(g.hooks[0x2AB670], actor, 16, slot, 0)
        assert accepted[-1] == (16, slot, 0)

    def native_action(group, slot, phase=0):
        # GU 283090 saves the action being replaced.
        old_group, old_slot = state()
        set_action(group, slot)
        half(actor + 9480, old_group)
        half(actor + 9484, old_slot)
        g.cpu.mem_write(actor + 5, bytes([phase]))

    def press(held, edge=0, yaw=0, strength=0, dt=2):
        half(actor + 5108, held)
        half(actor + 5112, edge)
        stick(strength, g.half(actor + 4076) - yaw)
        g.run(0x11A03A8, actor)  # Native w08 weapon command mapper.
        g.cpu.mem_write(inp + 16, bytes(g.cpu.mem_read(actor + 5156, 16)))
        frame(1, dt)

    def begin_jump(slot=56, slash=100, sword=False):
        g.run(tick, 0, 0)
        g.put(actor + 13100, slash)
        if sword:
            for _ in range(2):
                enter(90)
                frame(8)
            enter(94)
            frame(124)
            assert state() == (16, 0x8000 + 110)
        native_action(16, slot, 1)

    def land(slot=56):
        g.cpu.mem_write(actor + 5, b"\x02")
        press(0, dt=24)
        assert state() == (16, slot)
        native_action(1, 0)
        press(0)

    # Use GU's dispatcher for the in-place 1 -> 2 -> 3 -> idle sequence.
    stub(0x539F4C, lambda *_: 0)
    stub(0x2A84D8, lambda *_: 1)
    stub(0x2A063C, lambda *_: 0)
    stub(0x280E4C, lambda *_: 0)
    stub(0x281998, lambda hunter, clip, *_: half(hunter + 1204, clip & 0xFFFF) or 1)
    stub(0x950BCC, lambda *_: 1)
    stub(0x2B6E94, lambda _, mode, motion, *__: (
        (mode, motion) == (0, 4) and (native_action(1, 0) or 1)
    ))
    begin_jump()
    g.cpu.mem_write(actor + 6, b"\x01")
    press(0x20, 0x20)
    assert state() == (16, 56)
    g.run(0x11A08E0, actor)
    assert g.cpu.mem_read(actor + 5, 1) == b"\x02"
    g.run(g.hooks[0x2AB670], actor, 16, 2, 0)
    assert state() == (16, 56)  # Stock ground attacks cannot preempt recovery.
    press(0x120, 0x100, yaw=-0x4000, strength=100)
    g.run(0x11A08E0, actor)
    assert g.cpu.mem_read(actor + 5, 1) == b"\x03"
    press(0x120, yaw=0x4000, strength=100)
    g.run(0x11A08E0, actor)
    assert state() == (1, 0)
    press(0)
    assert state() == (16, 0x8000 + 107), state()  # Fresh chord replaced X; yaw was captured.
    native_action(1, 0)
    press(0)
    assert state() == (1, 0)

    for sword, button, yaw, strength, expected in (
        (False, 0x20, 0, 0, 0), (False, 0x20, 0, 100, 3),
        (False, 0x100, 0, 0, 1), (False, 0x120, 0, 0, 2),
        (True, 0x20, 0, 0, 5), (True, 0x20, 0, 100, 17),
        (True, 0x100, 0, 0, 78), (True, 0x120, 0, 100, 70),
        (True, 0x120, 0x4000, 100, 154),
    ):
        begin_jump(slot=57, sword=sword)
        press(button, button, yaw, strength)
        land(57)
        assert state() == (16, 0x8000 + expected), (sword, button, yaw, state())

    begin_jump(slot=67)
    press(0x100, 0x100)
    stub(0x2831E0, lambda _, group, slot, __: native_action(group, slot) or 1)
    g.cpu.mem_write(actor + 6, b"\x01")
    g.run(0x11A08E0, actor)
    assert state() == (16, 68)
    press(0, dt=24)
    assert state() == (16, 68)
    native_action(1, 0)
    press(0)
    assert state() == (16, 0x8001)

    begin_jump()
    press(0x20, 0x20)
    press(0x20, strength=100, dt=8)
    land()
    assert state() == (1, 0)  # Holding a button or changing the stick cannot refresh it.

    begin_jump(slash=0)
    press(0x120, 0x120, yaw=-0x4000, strength=100)
    land()
    assert state() == (1, 0)  # No gauge means no slash and no fallback to X+A.

    for interruption in ((64, 6), (1, 2), (4, 14)):
        begin_jump()
        press(0x100, 0x100)
        native_action(*interruption)
        press(0)
        native_action(1, 0)
        press(0)
        assert state() == (1, 0), interruption

    begin_jump()
    press(0x100, 0x100)
    g.cpu.mem_write(actor + 4180, b"\x02")
    land()
    assert state() == (1, 0)

    begin_jump()
    press(0x20, 0x20)
    g.cpu.mem_write(actor + 5, b"\x02")
    press(0)
    native_action(1, 0)
    press(0x100, 0x100)
    assert state() == (16, 0x8001)

    print("GU aerial routes: native landing, newest press, both forms, gauge rejection, expiry and interruption passed.")
    print(
        "GU routes: weapon form on damage and falls, directional ledge exits, sprint brake/sheathe/climb/resume, 12 native art entries and nine continuation requests passed."
    )
