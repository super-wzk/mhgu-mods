"""Replay Frontier blade contacts through the packaged GU registry and style controllers."""
from pathlib import Path
import struct
import sys
import zlib

from unicorn import UC_HOOK_CODE, arm_const as arm

PLUGINS = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(PLUGINS / "great-sword-extreme/tests"))
from verify_assets import Machine, MEM, STOP
from verify_style import (
    CHANGE_ACTION, COMMAND_UPDATE, DB_DRAW_ROUTE, DISPATCH, ENCODE, WEAPON_DISPATCH, api_for, boot, hunters,
    load_registered_style, quest_entries, setup_controller,
)


def source():
    m = Machine(PLUGINS.parent / "mhfz/eboot.bin.elf", thumb=True)
    player, extension = MEM + 0x1000, MEM + 0x4000
    m.trap(0x81815D60, lambda *_: 0)  # Motion step has already started.
    m.trap(0x81818BF8, lambda *_: extension)
    m.trap(0x8182B54E, lambda *_: 1)  # Locally controlled hunter.
    m.trap(0x81677C8E, lambda *_: 0)
    m.trap(0x817C5C40, lambda *_: 0)
    transitions = []
    m.trap(0x8182D1D8, lambda _, group, state, flags: transitions.append((group, state, flags)))
    # Stop after the weapon's hard-contact branch, before ordinary motion events.
    m.c.hook_add(UC_HOOK_CODE, lambda cpu, *_: cpu.reg_write(arm.UC_ARM_REG_PC, STOP | 1),
                 begin=0x81816422, end=0x81816422)
    for dispatch, recoil, cases in (
        (0x811D84FE, 3, ((0, True), (13, True), (68, True), (69, True),
                         (19, False), (54, False), (64, False), (65, False))),
        (0x811EF80C, 11, ((0, True), (16, True), (20, True), (43, True),
                          (55, True), (21, False), (24, False), (79, False))),
    ):
        for state, enabled in cases:
            transitions.clear()
            m.c.mem_write(player, bytes(4096))
            m.c.mem_write(player + 5, b"\x01")
            m.c.mem_write(player + 20, bytes((state, 1)))
            m.c.mem_write(player + 24, bytes((1, 1)))  # Hard grade and a real hit.
            m.put(extension + 648, 1)
            m.call(dispatch, player)
            m.call(dispatch, player)
            assert transitions == ([(1, recoil, 0)] if enabled else []), (state, transitions)
    print("FZ w00/w06 dispatch: original recoil destinations, exempt actions and single consumption passed")

    m.c.hook_add(UC_HOOK_CODE, lambda cpu, *_: cpu.reg_write(arm.UC_ARM_REG_PC, STOP | 1),
                 begin=0x8181F11E, end=0x8181F11E)
    for drawn, group, timer, expected in (
        (1, 1, 1, (1, 4)), (1, 0, 10, (9, 0)), (0, 1, 10, (0, 0)),
    ):
        m.c.mem_write(player + 18, bytes((drawn, 0, 0, group)))
        m.c.mem_write(player + 2852, struct.pack("<HBxB", timer, 0, 4))
        m.c.reg_write(arm.UC_ARM_REG_R8, player)
        m.c.reg_write(arm.UC_ARM_REG_R7, 1)
        m.call(0x8181FFF0)
        actual = struct.unpack("<H2xB", m.c.mem_read(player + 2852, 5))
        assert actual == expected, (drawn, group, timer, actual)
    print("FZ w06: blade polish retains its last tick in weapon actions and clears stacks on exit")

    updates = []
    m.trap(0x8182D384, lambda *_: 0)
    m.trap(0x8181DF10, lambda actor, *_: updates.append(actor))
    m.c.hook_add(UC_HOOK_CODE, lambda cpu, *_: cpu.reg_write(arm.UC_ARM_REG_PC, STOP | 1),
                 begin=0x81820CEA, end=0x81820CEA)
    m.c.mem_write(player + 30, b"\x01")  # No world effect to update.
    for hitstop in (3, 0):
        m.c.mem_write(player + 1253, bytes([hitstop]))
        m.call(0x81820BD4, player)
        assert updates == ([] if hitstop else [player]), (hitstop, updates)
    print("FZ w06: the hunter skips its blade polish update during hitstop")

    m.trap(0x8182E032, lambda *_: 3)  # Extreme style.
    m.trap(0x8182D1EE, lambda _, group, state, flags: transitions.append((group, state, flags)))
    for phase, target, state, facing in (
        (0, 93, 14, 0x1234), (1, 93, 20, 0x1234),
        (2, 93, 20, 0x1234), (3, 93, 72, 0x1234),
        (0, 97, 15, 0x1234), (3, 97, 21, 0x3234),
        (0, 98, 15, 0x1234), (3, 98, 21, 0xf234),
    ):
        transitions.clear()
        m.c.mem_write(player + 3148, bytes([phase]))
        m.c.mem_write(player + 14, struct.pack("<H", 0x1234))
        m.call(0x811EF3BA, player, target)
        assert transitions == [(1, state, 12)], (phase, target, transitions)
        assert m.c.mem_read(player + 14, 2) == struct.pack("<H", facing), (phase, target)
    m.trap(0x8184373A, lambda *_: 150)
    m.trap(0x81818AAC, lambda *_: 900)
    m.trap(0x811EEEC6, lambda _, __, mask, ___: 9 if mask & (1 << 9) else 28)
    transitions.clear()
    m.call(0x811EF3BA, player, 91)
    m.call(0x811F0C32, player)
    assert transitions == [(0, 200, 12), (1, 78, 12)], transitions
    print("FZ w06: native blade polish follow-ups and facing passed")

    m.trap(0x817C45EC, lambda *_: 1)
    m.trap(0x8172C370, lambda *_: 1)
    m.trap(0x8182B03C, lambda *_: 1)
    m.trap(0x8182B4FE, lambda *_: 0)

    def forward_stick(_, output, *__):
        m.c.mem_write(output, b"\0\0")
        return 1

    m.trap(0x8172C2D4, forward_stick)
    m.c.mem_write(player + 3, b"\x06")  # w06
    m.c.mem_write(player + 18, b"\x01")  # drawn
    for phase, group, state in ((0, 0, 28), (1, 0, 28), (2, 1, 36), (3, 1, 83)):
        transitions.clear()
        m.c.mem_write(player + 3148, bytes([phase]))
        assert m.call(0x817C5CD8, player, 1) == state
        assert transitions == [(group, state, 4)], (phase, transitions)
    print("FZ w06: free-state forward evade selects the roll or demon slash by phase")


def replay(weapon, style, plugin, recoil, recoil_clip):
    elf = (PLUGINS.parent / "mhgu/main.elf").read_bytes()
    dual = weapon == 11
    order = ("registry", "sword") if dual else ("registry",)
    g = boot(elf, f"[Weapons]\n{weapon}={style}\n".encode(), order)
    dist = PLUGINS / plugin / "dist" / plugin / "romfs/nativeNX/plugins"
    trails = {}
    if dual:
        bind = g.get(api_for(g, b"mhgu::StyleBladeTrailV1", b"SBT1"))

        def capture(c, *_):
            model, snapshot = (c.reg_read(r) for r in (arm.UC_ARM_REG_R0, arm.UC_ARM_REG_R1))
            trails[g.string(model)] = snapshot

        observe = g.cpu.hook_add(UC_HOOK_CODE, capture, begin=bind, end=bind)
    descriptor = load_registered_style(g, dist / (plugin.replace("-", "_") + ".nro"), base=0x9300000)
    if dual:
        g.cpu.hook_del(observe)
        assert list(trails) == [
            rf"effect\frontier_db\m{501 + 10 * slot + model}"
            for slot in range(5) for model in range(5)
        ]
        assert 0xA956F4 not in g.hooks
    controller = g.get(descriptor + 20)
    actor, eq, remote = hunters(g)
    vt = 0x174F2B4 if dual else 0x1749A88
    g.put(actor, vt)
    g.cpu.mem_write(eq + 1236, bytes([weapon]))
    g.cpu.mem_write(actor + 5078, struct.pack("<h", 100))
    inputs, part, target, factor = [g.alloc(n) for n in (40, 88, 47000, 4)]
    g.put(actor + 240, g.alloc(32))
    g.put(actor + 12696, g.alloc(64))
    g.put(actor + 12700, g.alloc(112))
    mgr, lookup, loader, bank = [g.alloc(n) for n in (16, 64, 4, 112)]
    g.put(0x2126A64, mgr)
    g.put(mgr, lookup)
    g.put(lookup + 48, loader)
    g.stub(loader, lambda _, kind, *__: bank if kind in (0x2125210, 0x1857810, 0x1889FDC) else 0)
    g.put(bank + 80, 1)
    g.put(actor + 3844 + 15 * 4, bank)
    g.stub(0xB49E54, lambda *_: 1)
    g.stub(0x887660, lambda *_: 0)
    allow_transition = True
    set_action = setup_controller(g, actor, accept_transition=lambda: allow_transition)
    g.stub(g.originals[CHANGE_ACTION], lambda _, group, slot, flags: set_action(group, slot))
    g.stub(0x2B6E5C, lambda *_: set_action(1, 0))
    g.stub(0x2AD288, lambda *_: 0)
    for slot in (440, 520, 536, 1992):
        g.stub(g.get(vt + slot), lambda *_: 0)
    g.stub(0xDF814, lambda *_: eq)
    g.stub(0xDD4C4, lambda *_: 0)
    skills = set()
    g.cpu.hook_del(g.traps.pop(0x2864C4))
    g.stub(0x2864C4, lambda _, skill, *__: int(skill in skills))
    if dual:
        g.stub(0x2864D4, lambda *_: 0)
    row = g.alloc(56)
    g.stub(g.originals[0x2A8A88], lambda *_: row)
    serial = 0
    submitted = []

    def construct(*_):
        nonlocal serial
        hit = g.get(g.cpu.reg_read(arm.UC_ARM_REG_SP))
        serial += 1
        g.cpu.mem_write(hit + 52, struct.pack("<H", serial))
        submitted.append(hit - actor)
        return 0

    g.stub(0x2B72D4, construct)
    for address in (0x178030, 0x1748A8):
        g.stub(address, lambda *_: 0)

    def tick(frame, *, native_input=False):
        g.cpu.mem_write(actor + 1276, struct.pack("<2f", frame + 1, frame))
        if native_input:
            entry = WEAPON_DISPATCH if g.half(actor + 9482) >= 0x8000 else DISPATCH
            g.run(g.hooks[entry], actor)
        else:
            g.run(controller, actor, inputs)
        return g.half(actor + 9482) - 0x8000

    def begin(state):
        g.run(CHANGE_ACTION, actor, 16, 0x8000 + state, 0)
        g.cpu.mem_write(actor + 1284, struct.pack("<f", 240))
        g.cpu.mem_write(actor + 5088, struct.pack("<f", 900))
        submitted.clear()
        assert tick(0) == state

    def contact(ratio, hand=12320, owner=None, resolver=0x178030):
        player = actor if owner is None else owner
        g.cpu.mem_write(factor, struct.pack("<f", ratio))
        g.run(g.hooks[resolver], 0, player + hand, target, part, part, player, factor, factor)

    def transition(accepted):
        nonlocal allow_transition
        allow_transition = accepted

    hands = (12320, 12504) if dual else (12320,)
    for hand in hands:
        for ratio, expected in ((0.2299, recoil), (0.23, 0), (0.7, 0)):
            begin(0)
            assert hand in submitted, submitted
            contact(ratio, hand)
            contact(0.1, hands[-1], resolver=0x1748A8)
            assert tick(16) == expected, (plugin, hand, ratio)
            if expected == recoil:
                assert g.half(actor + 1204) == recoil_clip
                assert tick(18) == recoil
    begin(0)
    contact(0.1, owner=remote)
    assert tick(16) == 0
    begin(0)
    g.cpu.mem_write(actor + 12320 + 52, struct.pack("<H", serial + 1))
    contact(0.1)
    assert tick(16) == 0
    begin(0)
    g.cpu.mem_write(part + 10, struct.pack("<H", 0x2000))
    contact(0.9)
    g.cpu.mem_write(part + 10, bytes(2))
    assert tick(16) == recoil
    begin(0)
    skills.add(87)
    contact(0.1)
    skills.clear()
    assert tick(16) == 0
    begin(0)
    contact(0.1)
    # GU can defer a transition for a tick. The next tick must retry it.
    allow_transition = False
    assert tick(16) == 0
    allow_transition = True
    assert tick(18) == recoil
    # A refused recoil must not suspend the source clip's normal end.
    begin(0)
    contact(0.1)
    allow_transition = False
    assert tick(16) == 0
    assert tick(240) == -0x8000 and g.half(actor + 9478) == 1, (plugin, "refused recoil")
    if not dual:
        begin(0)
        g.put(inputs, 1 << 3)
        tick(80)
        g.put(inputs, 0)
        assert tick(240) == -0x8000 and g.half(actor + 9478) == 1, (plugin, "refused roll")
    allow_transition = True
    begin(0)
    assert tick(16) == 0
    # A non-initial custom slot no longer owns its clip. Never replay its strike.
    g.cpu.mem_write(actor + 1204, struct.pack("<H", 0x501))
    count = len(submitted)
    assert tick(18) == -0x8000 and g.half(actor + 9478) == 1, (plugin, "replaced clip")
    assert len(submitted) == count
    if not dual:
        # GU accepted the draw but there is no bank in which to bind its clip.
        for i in range(8, 16):
            g.put(actor + 3844 + 4 * i, target)
        g.run(CHANGE_ACTION, actor, 16, 0x8000 + 13, 0)
        assert tick(0) == -0x8000 and g.half(actor + 9478) == 1
        g.put(actor + 3844 + 15 * 4, bank)
    begin(0)
    contact(0.1)
    begin(0)  # Interrupted and re-entered the same slot before recoil dispatch.
    assert tick(16) == 0
    # The converted recoil programs accept a fresh attack only inside their
    # authored buffer and execution windows.
    if dual:
        windows = ((13, 32, False), (14, 32, True), (40, 40, False))
        attack_input, command, followup = inputs + 16, 0, 17
    else:
        windows = ((1, 18, False), (2, 18, True), (30, 30, False))
        attack_input, command, followup = inputs + 16, 12, 16
    for press, execute, follows in windows:
        begin(0)
        contact(0.1)
        assert tick(1) == recoil
        g.put(attack_input, 1 << command)
        tick(press)
        g.put(attack_input, 0)
        assert tick(execute) == (followup if follows else recoil)
    if dual:
        begin(21)
        tick(20)
        assert submitted
        contact(0.1, submitted[-1])
        assert tick(22) == 21
        begin(24)
        tick(28)  # Activate blade polish.
        begin(0)
        g.cpu.mem_write(actor + 28, struct.pack("<f", 2.))
        for _ in range(125):
            assert tick(16) == 0
        g.put(actor + 28, 0)
        contact(0.1, 12504)
        assert tick(16) == 0
        power_and_trails(g, actor, eq, vt, trails, begin, tick, set_action)
        polish_hitstop(g, actor, begin, tick, set_action, contact)
        polish_followups(g, actor, begin, tick, transition, skills, set_action, submitted)
        demon_effects(g, actor, inputs, vt, begin, tick)
        aerial_followup(g, actor, begin, tick, set_action)
        roll_followup(g, actor, set_action)
    else:
        for state in (29, 54):
            begin(state)
            assert 12320 in submitted
            contact(0.1)
            assert tick(38) == state
    print(f"{plugin}: contact threshold, recoil clip, single response, exceptions and interruption passed")


def power_and_trails(g, actor, equipment, vtable, trails, begin, tick, set_action):
    g.cpu.mem_write(equipment + 800, struct.pack("<h", 200))
    separate = g.alloc(184)

    def power(hit):
        g.put(hit + 12, actor)
        g.put(hit + 48, 0x20)
        g.run(0x169548, hit)
        return struct.unpack("<f", struct.pack("<I", g.cpu.reg_read(arm.UC_ARM_REG_S0)))[0]

    for hit, expected in ((actor + 12320, 2.1), (actor + 12504, 2.1), (separate, 2.0)):
        assert abs(power(hit) - expected) < 1e-5, (hit, power(hit), expected)
    set_action(1, 0)
    g.cpu.mem_write(actor + 28, struct.pack("<f", 2.))
    tick(0)
    g.put(actor + 28, 0)
    assert power(actor + 12320) == 2.
    begin(0)
    assert power(actor + 12504) == 2.  # Re-entering cannot reactivate expired polish.

    matrices = {joint: g.alloc(64) for joint in (8, 12)}
    for hand, (joint, matrix) in enumerate(matrices.items()):
        g.cpu.mem_write(matrix, struct.pack("<16f", 1, 0, 0, 0, 0, 1, 0, 0,
                                            0, 0, 1, 0, 50 * hand, 0, 0, 1))
        part, model = g.alloc(5072), g.alloc(240)
        g.put(actor + 9120 + 4 * hand, part)
        g.put(part + 240, model)
        g.cpu.mem_write(model + 216, struct.pack("<f", 100. + 40 * hand))
    g.put(actor + 1172, matrices[8])
    g.put(actor + 1176, g.alloc(64))
    g.stub(0xC1628, lambda _, joint, *__: matrices.get(joint, 0))
    g.stub(g.get(vtable + 304), lambda *_: 0)
    begin(65)  # Demon activation publishes both hands at the first source event.

    sample = g.alloc(244)
    for model, side, length in ((502, 0., 85.), (503, 50., 125.)):
        callback = trails[rf"effect\frontier_db\m{model}"]
        assert g.run(callback, sample) != 0
        assert struct.unpack("<f", g.cpu.mem_read(sample, 4))[0] == side
        assert struct.unpack("<f", g.cpu.mem_read(sample + 36, 4))[0] == length
        assert g.cpu.mem_read(sample + 240, 4) == b"\xff" * 4
    begin(18)
    assert tick(2) == 18
    assert all(g.run(callback, sample) == 0 for callback in trails.values())
    print("GU DB blade-only polish, shared right/left blade trail snapshots and cleanup passed")


def polish_hitstop(g, actor, begin, tick, set_action, contact):
    def elapsed(frames):
        g.cpu.mem_write(actor + 28, struct.pack("<f", frames))

    for offset in (10156, 10160):
        begin(24)
        tick(28)
        set_action(1, 0)
        elapsed(2.)
        for _ in range(119):
            tick(0)
        # The two remaining GU frames outlast either native hitstop timer.
        for stop in (6., 4., 2.):
            g.cpu.mem_write(actor + offset, struct.pack("<f", stop))
            tick(0)
        g.put(actor + offset, 0)
        elapsed(0.)
        begin(0)
        contact(0.1)
        assert tick(16) == 0, ("polish expired during hitstop", offset)
        set_action(1, 0)
        elapsed(2.)
        tick(0)
        elapsed(0.)
        begin(0)
        contact(0.1)
        assert tick(16) == 11, ("polish did not resume counting", offset)
    print("GU DB blade polish pauses for full-stop and slow-hit and resumes afterward")


def polish_followups(g, actor, begin, tick, transition, skills, set_action, submitted):
    x, a, chord = 0x20, 0x100, 0x120
    aim = 0
    previous = 0
    g.cpu.hook_del(g.traps.pop(0x27F5D0))
    g.stub(0x27F5D0, lambda *_: aim)
    g.cpu.hook_del(g.traps.pop(0x27E608))
    g.stub(0x27E608, lambda *_: int(g.half(actor + 5136) >= 48))
    g.stub(0x27E638, lambda *_: 0)
    g.stub(0x27F678, lambda *_: aim)

    def pad(frame, held=0, *, stick=0, ended=False, common=0):
        nonlocal previous
        g.cpu.mem_write(actor + 5108, struct.pack("<3H", held, previous & ~held, held & ~previous))
        previous = held
        g.cpu.mem_write(actor + 5140, bytes(32))
        g.cpu.mem_write(actor + 5136, struct.pack("<h", stick))
        g.run(0x27CA28, actor)
        g.run(0x11BAEF0, actor)
        g.put(actor + 5140, g.get(actor + 5140) | common)
        if ended:
            g.cpu.mem_write(actor + 1206, b"\x04")
        state = tick(frame, native_input=True)
        if ended:
            g.cpu.mem_write(actor + 1206, b"\0")
        return state

    def enter(state):
        nonlocal previous
        previous = 0
        g.cpu.mem_write(actor + 5108, bytes(6))
        g.cpu.mem_write(actor + 5188, bytes(16))
        begin(state)

    for activation, frame, state, strike_frame in ((68, 48, 36, 12), (69, 86, 83, 10)):
        enter(activation)
        tick(frame)
        set_action(1, 0)
        g.put(actor + 4076, 0)
        submitted.clear()
        assert pad(0, stick=100, common=1 << 3) == state
        assert g.half(actor + 1204) == 1426
        for at in range(2, strike_frame, 2):
            assert pad(at) == state
        if state == 83:
            assert not submitted
        assert pad(strike_frame) == state
        assert submitted == [12320, 12504], (state, submitted)
    print("GU DB: free-state forward demon evade plays 1426 and submits both strikes")

    # FZ states 7 and 19 can execute X or X+A as soon as frame 60.
    # GU reports the raw X press two ticks before its resolved weapon command.
    for state in (7, 19):
        for first in (0x20, a):
            enter(state)
            assert pad(59) == state
            assert pad(60, first) == state
            assert pad(61, chord) == 21
        enter(state)
        assert pad(59) == state
        assert pad(60, 0x20) == state
        assert pad(61, 0x20) == state
        assert pad(62, 0x20) == 0

    for activation, activation_frame, result in ((18, 2, 13), (65, 2, 8), (68, 48, 8), (69, 86, 72)):
        begin(activation)
        tick(activation_frame)
        enter(24)
        assert pad(35, a) == 24
        assert pad(36) == 24
        assert g.get(actor + 5156) & 6
        assert pad(49) == 24
        assert pad(50) == result, (activation, result)

    for state, execute in ((25, 20), (27, 30), (33, 22), (61, 40)):
        enter(state)
        assert pad(12, a) == state
        assert pad(13) == state
        assert pad(execute) == 72, state
    for state in (26, 28, 29, 30, 31, 32, 57, 58, 59, 60):
        enter(state)
        assert pad(18, a) == state
        assert pad(19) == state
        assert pad(42) == state

    enter(24)
    assert pad(50, a) == 24
    assert pad(51) == 24

    # The w06 X windows close at 50 for the first two attacks and at 80
    # for the sweep. A press inside the window survives GU's confirmation.
    for state, close, followup in ((0, 50, 1), (1, 50, 2), (15, 80, 5)):
        for stride in (1, 2):
            enter(state)
            assert pad(close - 2 * stride, x) == state
            assert pad(close - stride, x) == state
            assert pad(close, x) == followup
        enter(state)
        assert pad(close, x) == state
        assert pad(close + 1) == state
        assert pad(close + 2) == state

    enter(24)
    assert pad(49, a) == 24
    assert pad(50) == 72  # Extreme Demon A, confirmed on release.

    enter(16)
    assert pad(24, x) == 16
    assert pad(25, x) == 16
    assert pad(26, x, ended=True) == 16
    assert pad(42) == 5  # The second clip accepts X from its start at 28.

    enter(24)
    assert pad(36, stick=100) == 24
    assert pad(37) == 24
    assert pad(38, stick=100) == 24
    assert pad(50, stick=100) == 89
    assert pad(1, a, stick=100) == 89
    assert pad(2, stick=100) == 78

    shoulder_layout = g.get(0x1897F78) + 17518
    old_layout = bytes(g.cpu.mem_read(shoulder_layout, 1))
    for activation, delta, result, layout, shoulder, lead in (
        (18, 0, 15, 0, 0, 0), (69, 0x2000, 21, 0, 0, 0),
        (69, -0x2000, 21, 0, 0, 0), (18, 0, 15, 0, 0x40, 0),
        (18, 0, 15, 0, 0x40, 1), (18, 0, 15, 0, 0x40, 4),
        (18, 0, 15, 1, 0x800, 0), (18, 0, 15, 1, 0x800, 1),
        (18, 0, 15, 1, 0x800, 4),
    ):
        g.cpu.mem_write(shoulder_layout, bytes([layout]))
        begin(activation)
        tick(86 if activation == 69 else 2)
        enter(24)
        g.put(actor + 4076, 0x1234)
        aim = 0x5234 if delta >= 0 else 0xd234
        assert pad(0) == 24
        for frame in range(40 - lead, 40):
            assert pad(frame, shoulder, stick=64) == 24
        assert pad(40, chord | shoulder, stick=64) == 24
        assert bool(g.get(actor + 5156) & (1 << 11)) == (shoulder == 0 or lead == 4)
        transition(False)
        assert pad(50, shoulder) == 24
        assert g.half(actor + 4076) == 0x1234
        transition(True)
        actual = pad(51, shoulder)
        assert actual == result, (activation, delta, layout, lead, actual, result)
        assert g.half(actor + 4076) == (0x1234 + delta) & 0xffff, (activation, delta)
    g.cpu.mem_write(shoulder_layout, old_layout)
    begin(18)
    tick(2)
    for first in (x, a):
        enter(24)
        for frame in (28, 29, 30):
            assert pad(frame, first) == 24
        assert pad(40, chord) == 24
        assert pad(41, chord) == 24
        assert pad(42, chord) == 24
        assert pad(50) == 15

    # GU reports the added A later as a single. It must not replace a chord
    # pressed before the input window.
    enter(0)
    for frame in (3, 4, 5):
        assert pad(frame, x) == 0
    assert pad(8, chord) == 0
    assert pad(9, chord) == 0
    assert pad(10, chord) == 0
    assert g.get(actor + 5156) & 6
    assert pad(32) == 0

    enter(24)
    assert pad(0) == 24
    assert pad(38, 0x40) == 24
    assert pad(50, 0x40) == 66

    begin(18)
    tick(2)
    for state, execute in ((0, 32), (1, 26)):
        for yaw, destination in ((0, 13), (0x6000, 14), (0x4000, 14),
                                 (0x2000, 13), (0xc000, 13)):
            enter(state)
            g.put(actor + 4076, 0)
            aim = yaw
            assert pad(12, a, stick=64 if yaw else 0) == state
            assert pad(13) == state  # The stick can return to neutral before GU confirms A.
            actual = pad(execute)
            assert actual == destination, (state, yaw, actual, destination)
    aim = 0

    cancel_windows(g, actor, begin, tick, pad, enter, skills)

    begin(69)
    tick(86)
    enter(72)
    assert pad(100, x) == 72
    assert pad(102, x, ended=True) == 72
    assert pad(104, x) == 6
    enter(72)
    assert pad(102, x, ended=True) == -0x8000  # A late press cannot extend the window.

    enter(0)
    g.put(actor + 4076, 0)
    aim = 0
    manager, camera = g.alloc(120), g.alloc(120)
    old_manager = g.get(0x2126504)
    g.put(0x2126504, manager)
    g.put(manager + 84, camera)
    g.put(actor + 12016, 0)
    g.cpu.mem_write(actor + 11990, b"\x01")
    assert pad(30, stick=100) == 0
    g.cpu.mem_write(camera + 96, struct.pack("<3f", 100, 0, 0))
    assert pad(34, stick=100, common=1 << 3) == 74
    assert g.cpu.mem_read(actor + 11990, 1) == b"\x01"
    g.cpu.mem_write(actor + 11990, b"\0")
    g.cpu.mem_write(actor + 5136, bytes(2))
    g.put(0x2126504, old_manager)
    print("GU DB: delayed X/A, overlapping chords, clip transitions and live dodge direction passed")


def cancel_windows(g, actor, begin, tick, pad, enter, skills):
    evade, polish = 1 << 3, 1 << 8
    layout = g.cpu.mem_read(g.get(0x1897F78) + 17518, 1)[0]
    shoulder = 0x800 if layout == 1 else 0x40
    for activation, frame, roll, sprint_roll, sweep in (
        (68, 48, 36, 40, 8), (69, 86, 83, 84, 72),
    ):
        begin(activation)
        tick(frame)
        for at, target in ((47, 21), (48, roll), (65, roll), (66, 21),
                           (115, 21), (116, roll), (180, roll)):
            enter(21)
            assert pad(at, common=evade) == target, (activation, at)
        enter(21)
        assert pad(24, 0x100) == 21
        assert pad(25) == 21
        assert pad(47) == 21
        assert pad(48) == sweep
        enter(21)
        assert pad(24, 0x100) == 21
        assert pad(25) == 21
        assert pad(48, common=evade) == roll
        enter(21)
        assert pad(66, 0x120) == 21
        assert pad(121) == 21
        assert pad(122) == 56
        assert pad(22, ended=True) == 56  # 1434 -> 1430, starting at 18.
        assert pad(91, common=evade) == 56
        assert pad(92, common=evade) == roll
        for at, target in ((114, 56), (141, 56), (142, roll)):
            enter(56)
            assert pad(22, ended=True) == 56
            assert pad(at, common=evade) == target

        for press, execute in ((30, 48), (80, 116)):
            enter(21)
            assert pad(press - 3) == 21
            assert pad(press - 2, stick=100) == 21
            assert pad(press - 1) == 21
            assert pad(press, stick=100) == 21
            assert pad(execute) == 89

        enter(89)
        assert pad(8, 0x100) == 89
        assert pad(9, 0x100) == 89
        assert pad(10, 0x100, common=evade) == sprint_roll
        enter(89)
        assert pad(10, common=evade) == sprint_roll

        for at, target in ((48, 18), (66, 21), (122, 18), (146, 21), (148, 18)):
            begin(activation)
            tick(frame)
            enter(21)
            assert pad(at, shoulder) == target, (activation, at)

    begin(69)
    tick(86)

    # Other two-clip attacks can take input in the first clip and use it
    # only when the second clip reaches the specific execution frame.
    for state, first_end, ready, destination in ((8, 40, 0, 31), (20, 40, 0, 30), (43, 6, 88, 61)):
        enter(state)
        assert pad(first_end - 2, common=polish) == state
        target = pad(first_end, ended=state == 43)
        if ready:
            assert target == state
            assert pad(ready - 1) == state
            target = pad(ready)
        assert target == destination, state
    enter(20)
    assert pad(38, common=polish) == 20
    assert pad(40, common=evade) == 83

    enter(67)
    assert pad(10) == 67
    assert pad(22) == 67  # 1440 -> 1448, starting at 12.
    assert pad(25) == 67
    assert pad(26) == 70

    begin(69)
    tick(86)
    for constitution, stamina, target in ((0, 100., 21), (156, 74., 21),
                                           (156, 100., 83), (158, 150., 21)):
        enter(21)
        if constitution:
            skills.add(constitution)
        g.cpu.mem_write(actor + 5088, struct.pack("<f", stamina))
        assert pad(48, common=evade) == target, (constitution, stamina)
        skills.discard(constitution)
    enter(21)
    g.cpu.mem_write(actor + 5088, bytes(4))
    g.cpu.mem_write(actor + 10500, struct.pack("<f", 30.))
    assert pad(116, common=evade) == 83
    enter(89)
    g.cpu.mem_write(actor + 5088, bytes(4))
    assert pad(8, 0x100) == 89
    assert pad(9, 0x100) == 89
    assert pad(10, 0x100) == 78
    g.cpu.mem_write(actor + 10500, bytes(4))
    print("GU DB: flurry, whirlwind and other multi-clip cancels, sprint priority and stamina passed")


def aerial_followup(g, actor, begin, tick, set_action):
    layout_offset = g.get(0x1897F78) + 17518
    for layout, shoulder in ((0, 0x40), (1, 0x800)):
        g.cpu.mem_write(layout_offset, bytes([layout]))
        chord = 0x120 | shoulder
        for presses, dest in (
            ((0x120,), 78), ((0x20, 0x120), 78), ((0x100, 0x120), 78),
            ((0x20, 0x20, 0x20, 0x120), 78), ((0x100, 0x100, 0x100, 0x120), 78),
            ((chord,), 78), ((shoulder, chord), 78), ((0x20,) * 3, 91),
        ):
            begin(69)
            tick(86)  # Enter Extreme Demon.
            set_action(4, 14)
            g.cpu.mem_write(actor + 5, b"\x02")
            g.cpu.mem_write(actor + 9477, b"\x02")
            g.cpu.mem_write(actor + 5188, bytes(8))
            previous = 0
            for held in presses:
                first_x = previous == 0 and held == 0x20
                g.cpu.mem_write(actor + 5108, struct.pack("<3H", held, previous & ~held, held & ~previous))
                previous = held
                g.cpu.mem_write(actor + 5140, bytes(32))
                g.run(0x27CA28, actor)
                g.run(0x11BAEF0, actor)
                if first_x:
                    # A native jump attack request arriving on the unresolved
                    # X edge must leave the hunter available for the chord.
                    g.run(CHANGE_ACTION, actor, 16, 31, 0)
                    assert g.half(actor + 9478) == 4 and g.half(actor + 9482) == 14
            route = g.run(DB_DRAW_ROUTE, actor, 5)
            group, slot, flags = struct.unpack("<3H", g.cpu.mem_read(route, 6))
            g.run(CHANGE_ACTION, actor, group, slot, flags)
            assert g.half(actor + 9482) == 0x8000 + dest, (layout, presses, g.half(actor + 9482))
    g.cpu.mem_write(layout_offset, b"\x00")
    print("GU DB aerial routing: plain X and shoulder chords in both layouts passed")


def roll_followup(g, actor, set_action):
    raw, = (zlib.decompress(raw) for name, _, raw in quest_entries("w11_m0.arc")
            if name.endswith(b"plcmd_w11_0x0010_011a"))
    fields = [struct.unpack_from("<H", raw, off)[0] for off in (504, 510, 516, 522)]
    assert fields == [16, 11, 4096, 8]
    table, table_list, tables, entries, row, resource = (g.alloc(n) for n in (24, 120, 104, 4, 24, 124))
    g.cpu.mem_write(table, struct.pack("<6I", 0, 0, 1, 1, 0, entries))
    g.cpu.mem_write(row, struct.pack("<3I3f", 0, 1, resource, 14, 44, 58))
    g.cpu.mem_write(resource + 100, struct.pack("<4H4I", *fields, 4, 0, 0, 0))
    g.put(table_list + 104, 26)
    g.put(table_list + 116, tables)
    g.put(tables + 25 * 4, table)
    g.put(entries, row)
    for fn in (COMMAND_UPDATE, 0x2E9B7C):
        g.cpu.hook_del(g.traps.pop(fn))
    g.stub(0x285F68, lambda *_: table_list)
    g.put(actor + 1188, 1)
    g.put(actor + 5068, 1)
    g.cpu.mem_write(actor + 10057, b"\x01")
    g.cpu.mem_write(actor + 5136, bytes(2))
    g.cpu.mem_write(actor + 28, struct.pack("<f", 2))

    for clip, execute in ((28, 44), (5012, 42), (5013, 42)):
        set_action(1, 6)
        g.cpu.mem_write(actor + 5, b"\x02")
        g.cpu.mem_write(actor + 1204, struct.pack("<H", g.run(ENCODE, actor, clip)))
        g.cpu.mem_write(actor + 1284, struct.pack("<f", 80))
        g.cpu.mem_write(actor + 5188, bytes(8))
        g.put(actor + 1268, 0)
        g.put(actor + 5328, 0)
        g.run(0x11BA4FC, actor)  # Native bind of w11 table 2025.
        assert g.get(g.get(actor + 9384) + 8) == 1
        for frame in range(32, execute + 1, 2):
            g.cpu.mem_write(actor + 1276, struct.pack("<2f", frame + 1, frame))
            g.cpu.mem_write(actor + 5332, struct.pack("<f", frame))
            g.cpu.mem_write(actor + 5108, struct.pack("<3H", 0x20, 0, 0x20 if frame == 32 else 0))
            g.cpu.mem_write(actor + 5140, bytes(32))
            g.run(0x27CA28, actor)
            g.run(0x11BAEF0, actor)
            g.run(COMMAND_UPDATE, actor)
            expected = (16, 0x8006) if frame == execute else (1, 6)
            assert (g.half(actor + 9478), g.half(actor + 9482)) == expected, (clip, frame)
    print("GU DB rolls: stock attack cannot consume the buffered X follow-up")


def demon_effects(g, actor, inputs, vtable, begin, tick):
    root = PLUGINS / "dual-blades-extreme"
    routes = list(struct.iter_unpack("<HH", (root / "target/assets/effects/routes.bin").read_bytes()))
    pel = root / "dist/dual-blades-extreme/romfs/nativeNX/effect/frontier_db/particles.pel"
    assert struct.unpack_from("<I", pel.read_bytes(), 12)[0] == sum(n for _, n in routes)
    # Full-flame ARGB in mhfdat's w06 parameters (+172 glow, +176 trail).
    # Read the shipped EFL through GU's colour sampler, including peer strips.
    color_output = g.alloc(16)
    for route, prefix_set, expected in (
        (8, "e", (255, 180, 48, 100)), (35, "e", (255, 180, 48, 100)),
        (11, "eabcd", (255, 180, 94, 200)), (14, "eabcd", (255, 180, 94, 200)),
    ):
        selector, count = routes[route]
        assert count == 1
        for prefix in prefix_set:
            blocks = (pel.parent / f"{prefix}{selector}.efl").read_bytes()[48:]
            model = struct.unpack_from("<I", blocks, 4)[0] >> 8
            assert blocks[model + 3] == 0, (route, prefix, "source-alpha additive")
            keys = model + struct.unpack_from("<H", blocks, model + 66)[0]
            data = g.alloc(len(blocks))
            g.cpu.mem_write(data, blocks)
            g.run(0xAF938C, color_output, data + keys, 3, 128)
            assert tuple(g.cpu.mem_read(color_output, 4)) == expected, (route, prefix)
    manager, scene, handle, parent = [g.alloc(n) for n in (128, 16, 8, 32)]
    g.put(0x188A0D4, manager)
    g.put(0x2126F48, scene)
    g.put(0x21270E0, g.alloc(32))
    g.stub(0x3268D8, lambda *_: handle)
    g.stub(0x326C78, lambda *_: 0)
    g.stub(0x3288C4, lambda size, *_: g.alloc(size))
    g.stub(0x328900, lambda *_: 0)
    g.stub(0x2A25F0, lambda *_: 0)
    matrix = g.alloc(64)
    for address, callback in (
        (g.get(vtable + 304), lambda *_: parent),
        (0xC1628, lambda *_: matrix),
    ):
        g.cpu.hook_del(g.traps.pop(address))
        g.stub(address, callback)
    g.cpu.mem_write(matrix + 48, struct.pack("<3f", 15., 35., 25.))
    g.cpu.mem_write(actor + 64, struct.pack("<3f", 10., 20., 30.))
    g.cpu.mem_write(actor + 4076, struct.pack("<H", 0x2000))
    spawned = []
    water = -100001.

    def spawn(_, unit, *__):
        stack = g.cpu.reg_read(arm.UC_ARM_REG_SP)
        spawned.append((g.get(stack), unit + 128))
        return 1

    def ground(*_):
        stack = g.cpu.reg_read(arm.UC_ARM_REG_SP)
        g.cpu.mem_write(g.get(stack + 4), struct.pack("<f", -5.))
        g.cpu.mem_write(g.get(stack + 8) + 20, struct.pack("<f", water))
        return 1

    g.stub(0x3267D0, spawn)
    g.stub(0x18192C, ground)

    def proofs(route):
        first, count = routes[route]
        return [p for selector, p in spawned if first <= selector < first + count]

    def world(route, position):
        assert len(proofs(route)) == routes[route][1], (route, spawned)
        for p in proofs(route):
            assert struct.unpack("<3f", g.cpu.mem_read(p + 176, 12)) == position
            assert struct.unpack("<f", g.cpu.mem_read(p + 36, 4))[0] == 45.

    def palette(glow, trail):
        # The blades use separate glow routes for their local rotations.
        for joint, glow_route, trail_route in ((8, 6, 9), (12, 33, 12)):
            assert [g.half(p + 66) for p in proofs(glow_route + glow)] == [joint], spawned
            assert len(proofs(trail_route + trail)) == 1, spawned

    g.put(inputs, 1 << 27)  # Keep R held through the phase gates.
    begin(18)  # Clear the phase left by the previous scene.
    spawned.clear()
    begin(65)
    palette(0, 0)
    spawned.clear()
    assert tick(32) == 65
    palette(1, 1)
    spawned.clear()
    tick(49)
    assert not spawned
    tick(50)
    world(29, (10., 20., 30.))
    spawned.clear()
    tick(50)
    assert not spawned
    tick(63)
    palette(0, 1)  # Extreme Demon before full flame: red glow, orange strip.
    spawned.clear()
    tick(83)
    assert not spawned
    tick(84)
    world(30, (15., 35., 25.))
    world(31, (10., -5., 30.))
    assert not proofs(32)
    spawned.clear()
    g.cpu.mem_write(actor + 28, struct.pack("<f", 7200.))
    tick(86)
    palette(2, 2)
    g.put(actor + 28, 0)
    water = 12.
    begin(69)
    spawned.clear()
    tick(84)
    world(30, (15., 35., 25.))
    world(32, (10., 12., 30.))
    assert not proofs(31)
    begin(18)
    begin(68)
    spawned.clear()
    g.cpu.mem_write(actor + 28, struct.pack("<f", 7200.))
    tick(48)
    palette(2, 2)  # Full flame can be reached in true demon as well.
    print("GU DB true/extreme demon: timed bursts, dry/wet placement, both blade palettes and full flame passed")


if __name__ == "__main__":
    source()
    replay(0, 1, "great-sword-extreme", 3, 1404)
    replay(11, 2, "dual-blades-extreme", 11, 1408)
