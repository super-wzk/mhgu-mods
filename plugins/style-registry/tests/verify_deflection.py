"""Replay Frontier blade contacts through the packaged GU registry and style controllers."""
from pathlib import Path
import struct
import sys

from unicorn import UC_HOOK_CODE, arm_const as arm

PLUGINS = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(PLUGINS / "great-sword-extreme/tests"))
from verify_assets import Machine, MEM, STOP
from verify_style import CHANGE_ACTION, api_for, boot, hunters, load_registered_style, setup_controller


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
    g.stub(loader, lambda _, kind, *__: bank if kind in (0x2125210, 0x1857810) else 0)
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

    def tick(frame):
        g.cpu.mem_write(actor + 1280, struct.pack("<f", frame))
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
        attack_input, command, followup = inputs, 30, 17
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
        contact(0.1, 12504)
        assert tick(16) == 0
        power_and_trails(g, actor, eq, vt, trails, begin, tick)
    else:
        for state in (29, 54):
            begin(state)
            assert 12320 in submitted
            contact(0.1)
            assert tick(38) == state
    print(f"{plugin}: contact threshold, recoil clip, single response, exceptions and interruption passed")


def power_and_trails(g, actor, equipment, vtable, trails, begin, tick):
    g.cpu.mem_write(equipment + 800, struct.pack("<h", 200))
    separate = g.alloc(184)
    for hit, expected in ((actor + 12320, 2.1), (actor + 12504, 2.1), (separate, 2.0)):
        g.put(hit + 12, actor)
        g.put(hit + 48, 0x20)
        g.run(0x169548, hit)
        power, = struct.unpack("<f", struct.pack("<I", g.cpu.reg_read(arm.UC_ARM_REG_S0)))
        assert abs(power - expected) < 1e-5, (hit, power, expected)

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


if __name__ == "__main__":
    source()
    replay(0, 1, "great-sword-extreme", 3, 1404)
    replay(11, 2, "dual-blades-extreme", 11, 1408)
