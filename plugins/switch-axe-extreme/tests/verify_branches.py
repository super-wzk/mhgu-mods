"""Compare GU release decisions to the original FZ branch dispatcher."""

import struct
from unicorn.arm_const import UC_ARM_REG_S1

from verify_assets import Machine, MEM


class Frontier:
    def __init__(self, elf):
        self.m = m = Machine(elf, True)
        self.actor = MEM + 0x10000
        m.trap(0x81818BF8, lambda *_: MEM + 0x20000)
        m.trap(0x8120F46A, lambda *_: MEM + 0x201C0)
        # 206..209 occur exclusively behind condition 9 in the shipped table.
        for style, expected in ((0, 2), (3, 0)):
            m.c.mem_write(self.actor + 3394, bytes([style]))
            assert m.call(0x8121B618, self.actor, 9) == expected
        m.trap(0x817C5C40, lambda *_: 0)
        m.trap(0x8181784E, lambda *_: 0)
        m.trap(0x8182B4AA, lambda *_: 0)
        m.trap(0x81212140, lambda *_: self.level)
        m.trap(0x8120F53E, lambda *_: 2 if self.sword else 1)
        m.trap(0x81212380, lambda *_: self.tier)

        def stick(_, out, *__):
            m.c.mem_write(out, struct.pack("<H", self.yaw & 65535))
            return int(self.yaw != 0)

        m.trap(0x8172C2D4, stick)
        m.trap(
            0x8182D1D8, lambda _, group, action, weapon: self.selected.append(action)
        )

    def command(self, commands, yaw, *, stick=True, button=214, stamina=450):
        """Run the actual FZ input selector with the selected button pressed."""
        m = self.m
        self.yaw = yaw
        for address, value in (
            (0x8172C1A8, 100 if stick else 0), (0x8184373A, 75),
            (0x81818AAC, stamina), (0x8172C370, 1), (0x817C45EC, 1), (0x81F04590, 0),
        ):
            m.trap(address, lambda *_, value=value: value)
        m.trap(0x8172B7D8, lambda candidate, *_: int(candidate == button))
        m.trap(0x8172B7E8, lambda candidate, *_: int(candidate == button))
        mask = sum(1 << command for command in commands)
        return m.call(0x8121B914, self.actor, mask, mask)

    def release(self, branch, *, sword=False, level=0, tier=1, yaw=0, facing=0):
        self.sword, self.level, self.tier, self.yaw = sword, level, tier, yaw
        self.selected = []
        self.m.c.mem_write(self.actor + 14, struct.pack("<H", facing))
        self.m.call(0x81214070, self.actor, branch)
        (action,) = self.selected
        return action, int.from_bytes(self.m.c.mem_read(self.actor + 14, 2), "little")

    def turn(self, previous, branch, yaw, facing):
        self.yaw = yaw
        self.m.put(0x82BB48C0, MEM + 0x30000)
        self.m.put(MEM + 0x30000 + 2692, MEM + 0x31000)
        # These moving releases are eligible for the pre-transition turn.
        self.m.trap(0x81FFE920, lambda *_: 0xFFFFFFFF)
        self.m.c.mem_write(self.actor, b"\x01")
        self.m.c.mem_write(self.actor + 22, bytes([previous]))
        self.m.c.mem_write(self.actor + 14, struct.pack("<H", facing))
        self.m.call(0x81213F18, self.actor, branch)
        return int.from_bytes(self.m.c.mem_read(self.actor + 14, 2), "little")


def replay(g, repo, actor, remote, tick, inputs, set_action, reg):
    reference = Frontier(repo / "mhfz/eboot.bin.elf")

    def half(p, v):
        g.cpu.mem_write(p, struct.pack("<H", v & 65535))

    def num(p, v):
        g.cpu.mem_write(p, struct.pack("<f", v))

    def clear_input():
        g.cpu.mem_write(inputs, bytes(40))
        half(actor + 5136, 0)

    def frame(n):
        num(actor + 1280, n)
        g.run(tick, actor, inputs)

    def enter(id):
        set_action(16, 0x8000 + id)
        frame(0)

    def current():
        return g.half(actor + 9482) - 0x8000

    def stub(addr, fn):
        if addr in g.traps:
            g.cpu.hook_del(g.traps.pop(addr))
        g.stub(addr, fn)

    clear_input()
    g.cpu.hook_del(g.traps.pop(0x27E608))
    skill = 0
    stub(0x2864C4, lambda _, candidate, *__: int(skill == candidate))
    num(actor + 5088, 900)
    for skill, lower, upper in ((0, 60, 270), (152, 48, 216), (153, 72, 324)):
        for sword in (False, True):
            for elapsed, level in (
                (lower - 1, 1),
                (lower, 2),
                (upper - 1, 2),
                (upper, 3),
            ):
                for branch in (211, 212, 214):
                    clear_input()
                    num(actor + 28, 0)
                    if sword:
                        enter(110)  # Complete the light-sword transformation first.
                    enter(162 if sword else 111)
                    walking = branch == 212
                    holding = (
                        (164 if walking else 163)
                        if sword
                        else (113 if walking else 112)
                    )
                    enter(holding)
                    if walking:
                        half(inputs + 34, 80)
                        half(actor + 5136, 80)
                    if branch == 214:
                        g.put(inputs, 1 << 27)
                        g.put(inputs + 16, 1 << 2)
                    num(actor + 28, elapsed)
                    frame(70)
                    expected, _ = reference.release(branch, sword=sword, level=level)
                    assert current() == expected, (
                        branch,
                        sword,
                        skill,
                        elapsed,
                        current(),
                        expected,
                    )

    # Record the row submitted by the NRO, and feed actual GU contact callbacks
    # separately from animation time. Only authored blade rows 24/25 count.
    clear_input()
    shape, manager, vt, load = [g.alloc(n) for n in (128, 32, 64, 4)]
    g.put(shape + 80, 1)
    g.put(manager, vt)
    g.put(vt + 48, load)
    g.put(0x2126A64, manager)
    g.stub(load, lambda _, kind, name, __: shape if kind == 0x1857810 else 0)
    strikes = []
    stub(
        g.get(reg + 8),
        lambda p, a, *_: strikes.append((p, g.half(a + 2), g.get(a + 16))) or 1,
    )
    stub(g.originals[0x178030], lambda *_: 0)
    part, target, factor = [g.alloc(32) for _ in range(3)]
    num(factor, 1)
    num(actor + 28, 0)

    def hit():
        g.run(g.hooks[0x178030], 0, actor + 12320, target, part, 0, actor, factor, 0)

    facing = 0x0300
    yaw = 0
    stub(0x27E608, lambda *_: int(yaw != 0))
    stub(0x27F5D0, lambda *_: (facing - yaw) & 65535)
    for hits in (0, 2, 4, 1):
        for yaw in (0, 0x1FFF, 0x2000, 0x6000, 0x6001, -0x2000, -0x6000, -0x6001):
            clear_input()
            g.run(tick, 0, 0)  # Fresh quest gauges for each independent release.
            enter(70)
            frame(70)
            assert strikes and strikes[-1][0] == actor
            for _ in range(hits):
                hit()
            # The controller consumes confirmed contacts on its next tick.
            frame(72)
            enter(72)
            # State 72 can release early with back + X; plain X at 208 works
            # for every yaw without relying on a direction-specific opener.
            half(actor + 4076, facing)
            half(actor + 5280, facing)
            half(inputs + 34, 80 if yaw else 0)
            g.put(inputs + 16, 1 << 14)
            frame(208)
            expected = reference.release(
                210,
                tier=3 if hits >= 4 else 2 if hits >= 2 else 1,
                yaw=yaw,
                facing=facing,
            )
            assert (current(), g.half(actor + 4076)) == expected, (
                hits,
                yaw,
                current(),
                expected,
            )
            assert g.half(actor + 5280) == expected[1]
    clear_input()

    # The receiving registry dispatches the accepted network action for a peer
    # independently of the local player's weapon. Motion and contacts stay on
    # that actor even if the local player never selected this style.
    roster = g.get(0x1885B58)
    g.put(roster + 32, remote)
    g.put(remote, 0x174D838)
    g.put(remote + 240, g.alloc(128))
    eq = g.alloc(1600)
    g.put(remote + 5060, eq)
    g.cpu.mem_write(eq + 1236, b"\x08")
    num(remote + 1284, 180)
    num(remote + 28, 2)
    # GU 1FED90 encodes the action sent by a hunter. 1FF308 applies the
    # packet to the corresponding hunter on the receiving machine.
    session, packet = g.alloc(128), g.alloc(200)
    g.put(0x188CA48, session)
    for addr in (
        0x2AC690,
        0x2E9B7C,
        0x27C8F8,
        0xE6890,
        0x2827B4,
        0x227B3C,
        0x280548,
        0x2FFC1C,
        0x2FF984,
        0xE174C,
    ):
        stub(addr, lambda *_: 0)
    for offset in (164, 884, 1132, 348, 376, 508, 2056, 612, 1828, 1356, 1992):
        stub(g.get(0x174D838 + offset), lambda *_: 0)
    stub(0x13EFDC4, lambda p, n, *_: g.cpu.mem_write(p, bytes(n)))
    sender = g.alloc(14000)
    g.put(sender, 0x174D838)
    g.put(sender + 5060, eq)
    for p in (actor, sender, remote):
        g.cpu.mem_write(p + 4177, b"\x01")
        g.cpu.mem_write(p + 4180, b"\x01")
    stub(g.get(0x174D838 + 136), lambda *_: 1)
    # The local slot equips another weapon; the receiving style comes from the peer.
    g.cpu.mem_write(g.get(actor + 5060) + 1236, b"\x00")

    def receive(p, action):
        half(sender + 9478, 16)
        half(sender + 9482, 0x8000 + action)
        g.run(0x1FED90, packet, sender, 0)
        assert g.half(packet + 56) == 0x8000 + action
        g.run(0x1FF308, packet, p)
        assert (
            g.half(p + 9482) == 0x8000 + action and g.cpu.mem_read(p + 5, 1) == b"\0"
        ), (g.half(p + 9478), g.half(p + 9482), g.cpu.mem_read(p + 5, 1))
        g.run(g.hooks[0x2B8178], p)

    def play(p, _, request, __):
        half(p + 1204, request)
        g.put(p + 1280, g.cpu.reg_read(UC_ARM_REG_S1))
        return 1

    stub(0x53A06C, play)
    for action, clip in ((179, 1055), (178, 1479)):
        receive(remote, action)
        start, = struct.unpack("<f", g.cpu.mem_read(remote + 1280, 4))
        assert (g.half(remote + 1204), start) == (clip, 90.)
        assert g.get(remote + 13096) == 1
    count = len(strikes)
    receive(remote, 0)
    assert g.half(remote + 1204) == 1401 and g.half(remote + 9482) == 0x8000
    num(remote + 1280, 56)
    g.run(g.hooks[0x2B8178], remote)
    assert len(strikes) == count + 1 and strikes[-1][0] == remote
    g.run(g.hooks[0x2B8178], remote)
    assert len(strikes) == count + 1
    print(
        "FZ/GU: release thresholds, both forms, side turns, per-discharge contacts and peer sprint resumes passed."
    )

    def sync_light():
        # Construct the packet from the selected local style, then join the peer
        # directly in a sword attack without replaying either unlock action.
        local_eq = g.get(actor + 5060)
        g.cpu.mem_write(local_eq + 1236, bytes([8]))
        g.put(actor, 0x174D838)
        g.run(tick, 0, 0)
        num(actor + 28, 0)
        clear_input()
        enter(94)
        enter(110)
        enter(78)
        g.run(g.hooks[0x1FED90], packet, actor, 0)
        value = g.get(packet + 16)
        assert (value >> 16) & 15 == 12, hex(value)
        assert g.half(packet + 56) == 0x8000 + 78
        g.run(g.hooks[0x1FF308], packet, remote)
        g.run(g.hooks[0x2B8178], remote)
        g.cpu.mem_write(local_eq + 1236, b"\0")

    verify_swing_fx(g, remote, eq, receive, load, stub, shape, sync_light)
    verify_peer_damage(g, remote, reg, receive, load, stub, eq)


def verify_swing_fx(g, actor, eq, receive, load, stub, shape, sync_light):
    from unicorn import arm_const
    from pathlib import Path

    root = Path(__file__).resolve().parents[1]
    assets = root / "target/assets/effects"
    keys = struct.iter_unpack("<HH", (assets / "selectors.bin").read_bytes())
    routes = dict(
        zip(keys, struct.iter_unpack("<HH", (assets / "routes.bin").read_bytes()))
    )
    sizes = {}
    for phi, bank in enumerate((4019, 4018, 4016, 4018, 4017, 4016)):
        for sel in (13, 26, 27, 31):
            first, count = routes[bank, sel]
            assert count and first + count < 65536
            sizes[phi, sel] = range(first, first + count)
    # GU proof records at the native spawn boundary, including the 1.1x
    # scale for sword mode. No renderer or audio engine is required.
    handle, unit, pel = [g.alloc(n) for n in (32, 416, 128)]
    g.put(pel + 80, 1)
    stub(
        load,
        lambda _, kind, *__: (
            pel if kind == 0x1889FDC else shape if kind == 0x1857810 else 0
        ),
    )
    g.put(0x188A0D4, g.alloc(128))
    g.put(0x2126F48, g.alloc(128))
    stub(0x3268D8, lambda *_: handle)
    stub(0x3288C4, lambda *_: unit)
    stub(0x328900, lambda *_: g.cpu.mem_write(unit, bytes(416)))
    stub(0x326C78, lambda *_: 0)
    # The network fixture stubs actor virtuals; restore the equipped-property query.
    g.cpu.hook_del(g.traps.pop(0x27C708))
    spawned = []

    def spawn(_, instance, *__):
        selector = g.get(g.cpu.reg_read(arm_const.UC_ARM_REG_SP))
        proof = instance + 128
        spawned.append(
            (
                selector,
                g.get(proof + 4),
                *[
                    struct.unpack("<3f", g.cpu.mem_read(proof + off, 12))
                    for off in (176, 32, 48)
                ],
            )
        )
        return 1

    stub(0x3267D0, spawn)

    def frame(n):
        g.cpu.mem_write(actor + 1280, struct.pack("<f", n))
        g.run(g.hooks[0x2B8178], actor)

    for phi in range(6):
        g.cpu.mem_write(eq + 854, struct.pack("<h", phi))
        for state, loop_state, sel in ((116, 117, 13), (155, 156, 31)):
            receive(actor, state)
            for phase in (0, 1):
                if phase:
                    receive(actor, loop_state)
                    frame(71)
                for i, n in enumerate((25, 55)):
                    spawned.clear()
                    frame(n)
                    take = [s for s in spawned if s[0] in sizes[phi, sel]]
                    assert [s[0] for s in take] == list(sizes[phi, sel]), (
                        phi,
                        state,
                        phase,
                        n,
                        take,
                    )
                    _, flags, pos, angles, scale = take[0]
                    assert flags == 0x4000000A
                    if sel == 13:
                        expected = (
                            ((220, 370, 50), (-310, 320, 50)),
                            ((240, 320, 80), (-320, 230, 120)),
                        )[phase][i]
                        roll = ((140, 240), (130, 250))[phase][i]
                        factor = 1.0
                    else:
                        expected = (0, 120, 20)
                        roll = ((140, 250), (130, 240))[phase][i]
                        factor = 1.1
                    assert pos == expected and abs(angles[2] - roll) < 0.01, (
                        pos,
                        angles,
                        expected,
                        roll,
                    )
                    assert all(abs(v - factor) < 0.00001 for v in scale)
    from verify_phial import replay as verify_phials

    verify_phials(g, root, actor, eq, receive, routes, stub, sync_light)
    mesh = (
        root / "dist/switch-axe-extreme/romfs/nativeNX/effect/frontier_sa/m265.mrl"
    ).read_bytes()
    _, _, nm, nt, _, tex, mat = struct.unpack_from("<7I", mesh)
    assert (nm, nt) == (2, 2)
    for i in range(2):
        row = mat + 60 * i
        size = struct.unpack_from("<I", mesh, row + 8)[0]
        block = struct.unpack_from("<I", mesh, row + 52)[0]
        count = struct.unpack_from("<I", mesh, row + 24)[0] & 4095
        assert (
            block + size <= len(mesh)
            and b"effect\\frontier_sa\\" in mesh[tex + 76 * i : tex + 76 * (i + 1)]
        )
        slots = [
            v
            for tag, v, _ in struct.iter_unpack(
                "<III", mesh[block : block + 12 * count]
            )
            if tag & 15 == 3 and v
        ]
        assert slots == [1 + i], slots
    print(
        "GU particles: both loop phases, all six equipped phial values and two-material mesh passed."
    )


def verify_peer_damage(g, actor, registry, receive, load, stub, equip):
    import zlib
    from pathlib import Path
    from damage_target import damage_target
    from verify_style import quest_arc

    # Restore the registry's real attack constructor and use its stock w08 HDP.
    g.cpu.hook_del(g.traps.pop(g.get(registry + 8)))
    arc = quest_arc("w08.arc").read_bytes()
    entries = (
        struct.unpack_from("<64s4I", arc, 12 + 80 * i)
        for i in range(struct.unpack_from("<H", arc, 6)[0])
    )
    _, _, packed, _, start = next(
        row
        for row in entries
        if row[1] == 0x4E2EF008 and row[0].rstrip(b"\0").endswith(b"pl_we08_hitdata")
    )
    hdp = zlib.decompress(arc[start : start + packed])
    bank, vt, data = [g.alloc(n) for n in (112, 76, len(hdp))]
    g.cpu.mem_write(data, hdp)
    g.put(bank, vt)
    g.put(vt + 64, 0x270D8C)
    g.put(vt + 68, 0x270D98)
    g.put(bank + 100, data)
    g.put(bank + 104, data + 16)
    g.put(actor + 12696, bank)
    root = Path(__file__).resolve().parents[1]
    raw = (
        root
        / "dist/switch-axe-extreme/romfs/nativeNX/player/hit/frontier_sa/hit_106_0.hts"
    ).read_bytes()
    shape, src, vt, getter = [g.alloc(n) for n in (112, len(raw), 76, 4)]
    g.cpu.mem_write(src, raw)
    g.put(shape, vt)
    g.put(vt + 68, getter)
    g.put(shape + 80, 1)
    g.put(shape + 100, src)
    for addr in (0x7AF3A0, 0x7AF37C):
        stub(addr, lambda n, *_: g.alloc(n))
    assert g.run(0x16A1D8, shape) == 1
    stub(getter, lambda p, i, *_: g.get(g.get(p + 104)) if i == 0 else 0)
    stub(
        load,
        lambda _, kind, path, __: (
            shape
            if kind == 0x1857810 and g.string(path).endswith(
                (r"frontier_sa\hit_106_0", r"frontier_sa\hit_106_2")
            )
            else 0
        ),
    )
    g.put(actor + 12700, shape)
    for addr in (0x887648, 0x2B76CC):
        stub(addr, lambda *_: 0)
    stub(0x16B920, lambda *_: 17)
    hit = actor + 12320
    g.run(0x1687A8, hit)
    g.cpu.mem_write(equip + 800, struct.pack("<h", 100))
    g.cpu.mem_write(actor + 5078, struct.pack("<h", 100))

    def contact(state, frame):
        receive(actor, state)
        g.cpu.mem_write(actor + 1280, struct.pack("<f", frame))
        g.run(g.hooks[0x2B8178], actor)
        return g.cpu.mem_read(hit + 108, 1)[0]

    hitstop = 16 + 56 + 37  # Native w08 HDP row 1.
    assert hdp[hitstop] == 5
    for state in (106, 107, 108, 152, 153, 154):
        assert contact(state, 10.) == 2, state
        for grade, timers in ((3, (0., 4.)), (4, (6., 0.))):
            g.cpu.mem_write(hit + 6, bytes([grade]))
            g.run(0x16DB68, 0, hit)
            assert struct.unpack("<2f", g.cpu.mem_read(actor + 10156, 8)) == timers
    for template, expected in ((8, 3), (1, 0)):
        g.cpu.mem_write(data + hitstop, bytes([template]))
        assert contact(106, 10.) == expected
    g.cpu.mem_write(data + hitstop, bytes([5]))
    g.cpu.mem_write(actor + 10156, bytes(8))
    print("GU peer: all six slash dodges scale the native contact hitstop.")

    monster, status, _, _, collision, credit = damage_target(g, g.get(actor))
    assert contact(0, 56.) == 5
    assert (g.get(hit + 12), g.get(hit + 132), g.half(hit + 52)) == (actor, shape, 17)
    # Supply the live bone mapping used by GU to expand the shipped HTS.
    mapping, matrices = g.alloc(256), g.alloc(512)
    g.put(actor + 1172, matrices)
    g.put(actor + 1176, mapping)
    g.cpu.mem_write(mapping, bytes([1]) * 256)
    g.cpu.mem_write(
        matrices + 176,
        struct.pack("<16f", 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 100, 90, 300, 1),
    )
    assert g.run(0x1696FC, hit) > 0
    _, radius, x, y, z = struct.unpack("<B3x4f", g.cpu.mem_read(collision + 8240, 20))
    assert radius > 0
    g.cpu.mem_write(collision + 28, struct.pack("<B3x4f", 1, 2, x, y, z))
    before = g.get(status + 880)
    g.run(0x16C0C0, collision, hit, monster)
    damage = g.half(status + 1160)
    assert damage > 0 and credit[-1] == (0, 1, damage), (credit, damage)
    g.run(0x97DDC, monster)
    assert g.get(status + 880) == before - damage
    print(f"GU peer: slot 1's axe hit reduced target HP by {damage}.")
