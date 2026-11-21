"""Compare the FZ weapon update with GU effect-unit submissions and releases."""

import struct
from unicorn import UC_HOOK_CODE, arm_const as arm
from verify_assets import Machine, MEM


def original_pulses(elf):
    m = Machine(elf, True)
    actor, motion = MEM + 0x1000, MEM + 0x5000
    seen = []
    # End this probe immediately after the global light; the motion dispatcher
    # has its own clip-specific bursts and is exercised by the GU replay below.
    m.c.hook_add(
        UC_HOOK_CODE,
        lambda c, *_: c.reg_write(arm.UC_ARM_REG_PC, 0x81217021),
        begin=0x81216F8C,
        end=0x81216F8C,
    )
    m.trap(0x818173BE, lambda *_: 12)
    m.trap(0x8183B4F2, lambda *_: 1)
    m.trap(0x818173C8, lambda *_: group)
    m.trap(0x8120F588, lambda *_: form)
    m.trap(0x8120F4E8, lambda _, mask, *__: int(bool(flags & mask)))
    m.trap(0x812123F8, lambda *_: unlocked)
    m.trap(0x8121097C, lambda *_: light)
    m.trap(0x812118C6, lambda *_: phial)
    m.trap(0x8181727C, lambda *_: clip)

    def submit(_, node, bank, selector):
        offset = int.from_bytes(
            m.c.mem_read(m.c.reg_read(arm.UC_ARM_REG_SP), 4), "little"
        )
        seen.append(
            (tick, node, bank, selector, struct.unpack("<3f", m.c.mem_read(offset, 12)))
        )
        return 0

    m.trap(0x8181818A, submit)
    expected = {}
    group, form, flags, unlocked, light, clip = 1, 2, 0, 1, 1, 1001
    for phial, bank in enumerate([4019, 4018, 4016, 4017], 1):
        seen.clear()
        for tick in range(1, 21):
            m.put(0x83A8F6E4, tick)
            m.call(0x81216E36, actor, motion)
        assert seen == [(t, 21, bank, 8, (-10.0, 0.0, 20.0)) for t in (5, 10, 15, 20)]
        expected[bank] = tuple(t for t, *_ in seen)
    for group, form, flags, unlocked, light, clip, frame, count in (
        (1, 2, 0, 1, 1, 1456, 137, 1),
        (1, 2, 0, 1, 1, 1456, 138, 0),
        (1, 1, 0, 1, 1, 1001, 0, 0),
        (1, 2, 0, 0, 1, 1001, 0, 0),
        (1, 2, 0, 1, 0, 1001, 0, 0),
        (2, 2, 0, 1, 1, 1001, 0, 0),
        (3, 2, 0, 1, 1, 1001, 0, 0),
        (7, 2, 0, 1, 1, 1001, 0, 0),
        (1, 2, 0x800, 1, 1, 1001, 0, 0),
        (1, 2, 0x1000, 1, 1, 1001, 0, 0),
    ):
        seen.clear()
        m.c.mem_write(actor + 476, struct.pack("<f", frame))
        m.call(0x81216E36, actor, motion)
        assert len(seen) == count, (group, form, flags, clip, frame, seen)
    return expected


def replay(g, root, actor, eq, receive, routes, stub, sync_light):
    expected = original_pulses(root.parents[1] / "mhfz/eboot.bin.elf")
    parent, model, lookup = [g.alloc(n) for n in (32, 256, 256)]
    g.put(actor + 1172, model)
    g.put(actor + 1176, lookup)
    equipment, equip_vt, socket, get_socket = [g.alloc(n) for n in (5100, 308, 32, 4)]
    g.put(actor + 9120, equipment)
    g.put(equipment, equip_vt)
    g.put(equip_vt + 304, get_socket)
    g.put(equipment + 1172, model)
    g.put(equipment + 1176, lookup)
    g.stub(get_socket, lambda *_: socket)
    stub(g.get(g.get(actor) + 304), lambda *_: parent)
    stub(0xC1628, lambda *_: model)
    # The engine can recycle released pointers. Keep the fixture's *live*
    # pointers distinct, then deliver the native notification before reuse.
    # Suballocate a particle pool independent of Forge and resource allocations.
    pool = 0x48000000
    g.cpu.mem_map(pool, 0x200000)
    cursor = 0

    def allocate(*_):
        nonlocal cursor
        cursor += 1
        assert cursor <= 4096
        return pool + (cursor - 1) * 416

    stub(0x3288C4, allocate)
    stub(0x328900, lambda u, *_: g.cpu.mem_write(u, bytes(416)))
    stopped, spawned = [], []
    stub(0x32A400, lambda u, *_: stopped.append(u) or 0)
    stub(g.originals[0x32A104], lambda *_: 0)

    def spawn(_, unit, *__):
        i = g.get(g.cpu.reg_read(arm.UC_ARM_REG_SP))
        p = unit + 128
        spawned.append(
            (
                i,
                unit,
                g.half(p + 66),
                g.get(p + 192),
                struct.unpack("<3f", g.cpu.mem_read(p + 16, 12)),
            )
        )
        return 1

    stub(0x3267D0, spawn)
    local = g.get(g.get(0x1885B58) + 28)
    stub(g.originals[0x282DA8], lambda *_: 0)

    def frame(n):
        g.run(g.hooks[0x282DA8], local)
        g.cpu.mem_write(actor + 1276, struct.pack("<2f", n + 1, n))
        g.run(g.hooks[0x2B8178], actor)

    def tick_size(n):
        for hunter in (actor, local):
            g.cpu.mem_write(hunter + 28, struct.pack("<f", n))

    def selected(bank, sel):
        start, count = routes[bank, sel]
        return [r for r in spawned if start <= r[0] < start + count]

    def release(units):
        for _, unit, *_ in units:
            g.run(g.hooks[0x32A104], unit)

    tick_size(0)
    receive(actor, 94)
    receive(actor, 110)
    receive(actor, 78)
    frame(20)
    tick_size(2)
    glow_proofs = {}
    for phi, bank in enumerate((4019, 4018, 4016, 4018, 4017, 4016)):
        g.cpu.mem_write(eq + 854, struct.pack("<h", phi))
        observed = []
        for tick in range(1, 21):
            spawned.clear()
            frame(20)
            batch = selected(bank, 8)
            if batch:
                observed.append(tick)
                assert len(batch) == routes[bank, 8][1]
                assert all(
                    (j, p, o) == (0, socket, (-10.0, 0.0, 20.0))
                    for _, _, j, p, o in batch
                )
                for layer, unit, *_ in batch:
                    if layer not in glow_proofs:
                        glow_proofs[layer] = bytes(g.cpu.mem_read(unit + 128, 200))
                release(batch)
        assert tuple(observed) == expected[bank], (
            phi,
            observed,
            cursor,
            g.get(actor + 13096),
            g.half(actor + 1204),
            g.half(actor + 9482),
            g.cpu.mem_read(actor + 5, 1),
        )
    # Source 812165AC's five-tick periodic jet is separate from the sword glow.
    tick_size(0)
    receive(actor, 71)
    frame(120)
    tick_size(2)
    spawned.clear()
    for _ in range(5):
        frame(120)
    jets = selected(4016, 4)
    assert len(jets) == routes[4016, 4][1] and all(
        r[4] == (-20.0, 0.0, 40.0) for r in jets
    )
    # Stop exactly the still-live units when the clip changes.
    release(jets[:1])
    stopped.clear()
    receive(actor, 75)
    assert {r[1] for r in jets[1:]}.issubset(stopped) and jets[0][1] not in stopped
    spawned.clear()
    frame(138)
    for _ in range(5):
        frame(138)
    assert not selected(4016, 8)
    # The release variants use different phial layers at the same strike frame.
    # The cartridge uses effect.bin mesh 151 and the left hand during 100..142.
    release(spawned)
    cartridge, count = struct.unpack_from(
        "<HH",
        (root / "target/assets/effects/routes.bin").read_bytes(),
        (len(routes) + 3) * 4,
    )
    assert count == 1
    tick_size(0)
    receive(actor, 86)
    for n, enabled in ((79, False), (80, True), (80, False)):
        spawned.clear()
        frame(n)
        batch = selected(4014, 16)
        assert len(batch) == (routes[4014, 16][1] if enabled else 0), (n, batch)
        release(spawned)
    for state, tier in ((75, 1), (85, 2), (86, 3)):
        receive(actor, state)
        spawned.clear()
        frame(25)
        assert bool(selected(4016, 10)) == (tier == 3)
        release(spawned)
        spawned.clear()
        frame(101)
        cartridges = [r for r in spawned if r[0] == cartridge]
        assert len(cartridges) == 1 and cartridges[0][2:] == (
            12,
            parent,
            (-9.0, -3.0, 0.0),
        )
        handle = cartridges[0][1]
        release([r for r in spawned if r[1] != handle])
        stopped.clear()
        spawned.clear()
        frame(143)
        assert handle in stopped
        assert bool(selected(4016, 2)) == (tier == 1)
        assert bool(selected(4016, 3)) == (tier >= 2)
        assert bool(selected(4016, 18)) == (tier == 3)
        release(spawned)
    # Drop the recorded participant before receiving the authoritative snapshot.
    manager = g.get(0x1885B58)
    peer_slot = next(
        manager + 28 + 4 * i for i in range(4) if g.get(manager + 28 + 4 * i) == actor
    )
    g.put(peer_slot, 0)
    frame(143)
    g.put(peer_slot, actor)
    sync_light()
    tick_size(2)
    spawned.clear()
    for _ in range(5):
        frame(20)
    assert len(selected(4016, 8)) == routes[4016, 8][1]
    release(spawned)

    # The native sheathe action can take ownership before it starts its clip.
    tick_size(0)
    sync_light(148)
    spawned.clear()
    frame(23)
    frame(39)
    opener = selected(4016, 14) + selected(4016, 16)
    assert len(opener) == routes[4016, 14][1] + 3 * routes[4016, 16][1], opener
    sync_light(179)
    tick_size(2)
    spawned.clear()
    for _ in range(27):
        frame(90)
    glow = selected(4016, 8)
    sprint = selected(4016, 25) + selected(4013, 0)
    assert glow and sprint
    release(glow[:1])
    stopped.clear()
    spawned.clear()
    g.cpu.mem_write(actor + 9478, struct.pack("<H2xH", 1, 2))
    g.cpu.mem_write(actor + 5, b"\0")
    frame(90)
    assert set(stopped) == {r[1] for r in glow[1:] + sprint + opener}, stopped
    for _ in range(5):
        frame(90)
    assert not (selected(4016, 8) or selected(4016, 25) or selected(4013, 0))

    tick_size(0)
    receive(actor, 105)
    spawned.clear()
    frame(2)
    frame(6)
    brake = selected(4015, 13) + selected(4014, 3)
    assert len(brake) == 2 * routes[4015, 13][1] + routes[4014, 3][1]
    stopped.clear()
    g.cpu.mem_write(actor + 9478, struct.pack("<H2xH", 1, 2))
    frame(6)
    assert set(stopped) == {r[1] for r in brake}

    # Bind the private native locomotion bank so sheathe reaches real clip IDs.
    manager = g.get(0x2126A64)
    load = g.get(g.get(manager) + 48)
    lmt = {}
    for bank in ("m_0", "m_4"):
        lmt[bank] = g.alloc(128)
        g.put(lmt[bank] + 80, 1)
    stub(load, lambda _, kind, path, __: (
        lmt[g.string(path).rsplit("\\", 1)[-1]] if kind == 0x2125210 else 0
    ))
    stub(0xB49E54, lambda *_: 1)
    stub(0x957400, lambda p, r, bank, *_: g.put(p + 3844 + 4 * bank, r))

    def native_frame(n):
        g.cpu.mem_write(actor + 1280, struct.pack("<f", n))
        g.run(g.hooks[0x282DA8], local)

    for previous, clip, cue in ((78, 1003, 8), (179, 1009, 6)):
        tick_size(0)
        sync_light(previous)
        packed = g.run(g.hooks[0x27C6A0], actor, 0x10000 | clip)
        assert packed & 0xff == clip - 1000 and 8 <= packed >> 8 < 16
        if previous == 78:
            # A stationary, drawn hunter enters sheathe through GU itself.
            stub(0x2A2444, lambda *_: 1)
            g.cpu.mem_write(actor + 9478, struct.pack("<H2xH", 1, 0))
            native_frame(20)
        tick_size(2)
        spawned.clear()
        for _ in range(5):
            if previous == 179:
                frame(90)
            else:
                native_frame(20)
        glow = selected(4016, 8)
        assert glow, (previous, spawned, g.get(actor + 13096), g.half(actor + 9478))
        stopped.clear()
        g.cpu.mem_write(actor + 9478, struct.pack("<H2xH", 1, 2))
        g.cpu.mem_write(actor + 1204, struct.pack("<H", packed))
        g.cpu.mem_write(actor + 1276, struct.pack("<f", cue - 1))
        spawned.clear()
        native_frame(cue + 1)
        assert {r[1] for r in glow} <= set(stopped)
        assert not (selected(4015, 4) or selected(4014, 2) or selected(4016, 8)), spawned

    # Start the highest discharge in the same room, then interrupt it with
    # damage while the old motion is still bound and its old clock is frozen.
    tick_size(0)
    spawned.clear()
    sync_light(86)
    flash = selected(4014, 21)
    assert len(flash) == routes[4014, 21][1]
    packed = g.run(g.hooks[0x27C6A0], actor, 0x10000 | 1456)
    assert packed & 0xff == 56 and 8 <= packed >> 8 < 16
    frame(95)
    live = flash + selected(4014, 5)
    assert len(live) > len(flash)
    g.cpu.mem_write(actor + 9478, struct.pack("<H2xH", 64, 6))
    g.cpu.mem_write(actor + 1204, struct.pack("<H", packed))
    g.cpu.mem_write(actor + 1276, struct.pack("<f", 95))
    stopped.clear()
    spawned.clear()
    native_frame(97)
    native_frame(97)
    assert not spawned
    assert {r[1] for r in live} <= set(stopped)

    area = bytes(g.cpu.mem_read(actor + 4180, 1))
    g.cpu.mem_write(actor + 4180, bytes((area[0] ^ 1,)))
    receive(actor, 86)
    frame(97)
    receive(actor, 105)
    frame(7)  # The axe brake emits through the separate motion FX table.
    assert not spawned
    g.cpu.mem_write(actor + 4180, area)
    frame(8)
    assert not spawned  # Effects skipped in the other room do not play on re-entry.

    tick_size(0)
    g.put(actor + 9120, 0)
    g.put(actor + 1172, 0)
    g.put(actor + 1176, 0)
    from verify_phial_follow import moving_blade
    moving_blade(root, glow_proofs)
    print(
        "FZ/GU phials: six equipment values, five-tick cadence, weapon-model anchor, discharge cutoff, late-join state, three release tiers, cartridge, sprint/sheathe retirement, damage interruption and cross-area effects passed."
    )
