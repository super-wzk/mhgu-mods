"""Exercise the released sprint/terrain controller and the retail w08 art routes."""

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

    def flick():
        for strength in (0, 100, 0, 100):
            half(inp + 34, strength)
            half(actor + 5136, strength)
            frame(0)

    assert g.get(actor + 13100) == 50
    flick()
    assert state() == (16, 0x8000 + 103) and g.get(actor + 13100) == 44
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
    for sex in ("m", "f"):
        folder = root / "dist/switch-axe-extreme/romfs/nativeNX/player/mot/frontier_sa"
        banks = {i: (folder / f"{sex}_{i}.lmt").read_bytes() for i in (0, 2, 4)}
        for clip in clips:
            bank = banks[(clip - 1000) // 100]
            slot = clip % 100
            assert slot < struct.unpack_from("<H", bank, 6)[0]
            at = struct.unpack_from("<I", bank, 8 + 4 * slot)[0]
            assert 0 < at < len(bank) - 48, (sex, clip, at)
    print(
        f"GU action package: {active} state entries, {steps} steps, {len(clips)} clips in both animation sets."
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
    for slash, stick in ((0, 80), (50, 0)):
        g.put(actor + 13100, 100)
        enter(103)
        g.put(actor + 13100, slash)
        set_action(1, 138)
        frame(1)
        assert state() == (1, 138)
        half(inp + 34, stick)
        half(actor + 5136, stick)
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
    print(
        "GU routes: weapon form on damage and falls, directional ledge exits, sprint brake/sheathe/climb/resume, 12 native art entries and nine continuation requests passed."
    )
