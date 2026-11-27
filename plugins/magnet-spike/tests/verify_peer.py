"""Use GU equipment and action packets to replay Magnet Spike on another hunter."""

import struct

from verify_style import CHANGE_ACTION, DECODE, DISPATCH, ENCODE, WEAPON_DISPATCH


def capture_action(g, actor):
    # Exercise the packet builder with only the stock stat callbacks stubbed.
    vtable = g.get(actor)
    packet = g.alloc(152)
    callbacks = (0x2FF984,) + tuple(g.get(vtable + offset) for offset in (348, 376, 508, 2024))
    temporary = {pc for pc in callbacks if pc and pc not in g.traps}
    for pc in temporary:
        g.stub(pc, lambda *_: 0)
    try:
        g.run(0x1FED90, packet, actor, 0)
        return bytes(g.cpu.mem_read(packet, 152))
    finally:
        for pc in temporary:
            g.cpu.hook_del(g.traps.pop(pc))


def capture_position(g, actor):
    packets = []
    process = g.alloc(48)
    g.put(process + 36, 1)
    transport = g.originals[0x9FC3D8]

    def capture(_, target, packet, priority):
        assert target == 1 and priority == 4 and g.get(packet) == 0x17423E4
        packets.append(bytes(g.cpu.mem_read(packet, 48)))

    g.stub(transport, capture)
    try:
        g.run(0x200438, process, actor)
    finally:
        g.cpu.hook_del(g.traps.pop(transport))
    packet, = packets
    return packet


def verify_peer(g, local, vtable, part, monster, status, collision, credit, set_action, tick, meter,
                flight_packets, boost_packets, guard_packets, shot_data, sounds, effects):
    manager, equipment = g.get(0x1885B58), g.get(local + 5060)
    g.put(0x18843F0, g.alloc(0x16280))
    actions = g.alloc(200)
    motions = []

    def stub(address, callback=lambda *_: 0):
        if address in g.traps:
            g.cpu.hook_del(g.traps.pop(address))
        g.stub(address, callback)

    def raw_action(actor, group, slot):
        g.cpu.mem_write(actor + 9478, struct.pack("<H2xH", group, slot))
        g.cpu.mem_write(actor + 5, b"\0")
        return 1

    def motion(actor, layer, request, _flags):
        if layer == 0:
            motions.append((actor, request & 0xffff))
        g.cpu.mem_write(actor + 1204 + 320 * layer, struct.pack("<H", request & 0xffff))
        return 1

    # Keep GU's packers, action requests, and both packet readers. Only the
    # live-scene callbacks and final writes have to be supplied to Unicorn.
    stub(g.originals[CHANGE_ACTION], lambda p, group, slot, _: raw_action(p, group, slot))
    stub(0x53A06C, motion)
    for address in (0x280548, 0x2FFC1C, 0x2FF984, 0xE174C, 0x4D31E8, 0x4D399C,
                    0x561F44, 0x22D8B4, 0x26E580, 0x26FDE8):
        stub(address)
    stub(0x26E600, lambda out, inp, *_: g.cpu.mem_write(out, bytes(g.cpu.mem_read(inp, 1376))))
    stub(0x13EFE3C, lambda out, inp, size, _: g.cpu.mem_write(out, bytes(g.cpu.mem_read(inp, size))))
    for offset in (164, 348, 508, 884, 1132, 2024):
        stub(g.get(vtable + offset))
    stub(g.get(vtable + 136), lambda *_: 1)
    stub(g.get(vtable + 376), lambda p, *_: g.get(p + 10057) & 1)
    stub(g.get(vtable + 460), lambda *_: 5)  # Stock w06 uses bits 16–19.

    def set_mode(buttons):
        set_action(1, 0)
        g.cpu.mem_write(local + 10057, b"\0")
        tick(buttons, native_pad=True)

    def capture(expected):
        g.run(0x1FED90, actions, local, 0)
        flags = g.get(actions + 16)
        assert (flags >> 16) & 15 == 5 and (flags >> 22) & 15 == (8 | expected), hex(flags)
        return bytes(g.cpu.mem_read(actions, 152))

    # A peer may have a different native fallback. Its extension is read from
    # the equipment packet rather than inferred from this raw weapon byte.
    g.cpu.mem_write(equipment + 1236, b"\x06")
    set_mode(0x120)
    impact = capture(1)
    set_action(1, 6)
    impact_roll = capture(1)
    set_mode(0x20)
    slash = capture(0)
    set_action(1, 6)
    slash_roll = capture(0)
    set_action(16, 0x8000 + 93)  # Packet construction before controller startup.
    recoil = capture(1)
    set_action(16, 0x8000 + 58)
    flight = capture(0)
    set_action(16, 0x8000 + 65)
    landing = capture(0)
    set_action(16, 0x8000 + 68)
    repeated_flight = capture(1)

    profile = g.alloc(1472)
    g.cpu.mem_write(profile + 8, bytes(g.cpu.mem_read(equipment, 1376)))
    g.put(profile + 1400, 0x98000000 | 0x1001 << 1)
    stock = g.alloc(1376)
    g.put(local + 5060, stock)
    set_action(1, 0)
    g.run(DISPATCH, local)

    def guest(index):
        actor, weapon = g.alloc(14000), g.alloc(5152)
        g.put(manager + 28 + 4 * index, actor)
        g.put(actor, vtable)
        g.put(actor + 240, g.alloc(128))
        g.put(actor + 1172, g.get(local + 1172))
        g.put(actor + 1176, g.get(local + 1176))
        g.put(actor + 5060, manager + 7004 + 1376 * index)
        g.cpu.mem_write(actor + 4177, bytes([index]))
        g.cpu.mem_write(actor + 5078, struct.pack("<h", 100))
        for offset in (12320, 12504):
            g.run(0x1687A8, actor + offset)
        g.put(actor + 9120, weapon)
        g.put(weapon + 48, actor)
        g.put(weapon + 240, g.alloc(128))
        g.put(weapon + 1172, g.get(part + 1172))
        g.put(weapon + 1176, g.get(part + 1176))
        g.put(weapon + 1204, 0xffff)
        g.run(0x202894, g.alloc(64), profile, index)
        return actor, weapon

    first, first_part = guest(1)
    second, second_part = guest(2)
    stub(g.get(vtable + 2020))
    stub(g.originals[WEAPON_DISPATCH], lambda *_: 0xBAD)

    def receive(actor, packet):
        g.cpu.mem_write(actions, packet)
        g.run(0x1FF308, actions, actor)

    def frame(actor, number, ended=False):
        g.cpu.mem_write(actor + 1280, struct.pack("<f", number))
        g.cpu.mem_write(actor + 1206, bytes([4 * ended]))
        assert g.run(WEAPON_DISPATCH, actor) != 0xBAD

    # Each peer starts its own cursor and emits the original hit once, after
    # entering the second clip. The contact is owned by the peer, not the host.
    for actor, packet, state, start, swing, contact in (
        (first, impact, 79, 1023, 1413, 30.), (second, slash, 78, 1440, 1049, 40.)
    ):
        receive(actor, packet)
        assert g.half(actor + 9482) == 0x8000 + state
        frame(actor, 0.)
        assert g.half(actor + 1204) == start
        packed = g.run(ENCODE, actor, 0x10000 | start)
        assert packed & 255 == start % 100 and g.run(DECODE, actor, packed) == 0x10000 | start
        frame(actor, 60., ended=True)
        assert g.half(actor + 1204) == swing
        hit = actor + 12320
        frame(actor, contact - 1)
        assert g.cpu.mem_read(hit + 5, 1) == b"\0"
        frame(actor, contact)
        assert g.get(hit + 12) == g.get(hit + 112) == actor
        serial = g.half(hit + 52)
        frame(actor, contact)
        assert g.half(hit + 52) == serial

    hit = second + 12320
    second_equipment = g.get(second + 5060)
    g.cpu.mem_write(second_equipment + 800, struct.pack("<h", 150))
    g.run(0x9D67C, monster)
    hp = g.get(status + 880)
    g.put(collision + 10288, 0)
    assert g.run(0x1696FC, hit) == 2
    xyz = bytes(g.cpu.mem_read(collision + 8248, 12))
    g.cpu.mem_write(collision + 28, struct.pack("<B3xf", 1, 2.) + xyz)
    g.run(0x16C0C0, collision, hit, monster)
    damage = g.half(status + 1160)
    assert damage > 0 and credit[-1] == (0, 2, damage), credit
    g.run(0x97DDC, monster)
    assert g.get(status + 880) == hp - damage

    receive(first, recoil)
    frame(first, 0.)
    assert g.half(first + 1204) == 1223
    frame(first, 60., ended=True)
    assert g.half(first + 9482) == 0x8000 + 93  # Only a packet can advance to another action.

    receive(first, flight)
    g.cpu.mem_write(first + 4180, b"\x01")
    frame(first, 0.)
    assert g.cpu.mem_read(first + 5, 1) == b"\0"
    g.cpu.mem_write(first + 4180, b"\0")
    frame(first, 8.)
    assert g.half(first + 1204) == 1472 and g.get(first + 10057) & 0x40 == 0
    frame(first, 34.)
    assert g.get(first + 10057) & 0x40
    receive(first, landing)
    frame(first, 0.)
    assert g.half(first + 1204) == 1473

    # Guided and coasting snapshots originate in the native position sender
    # and are applied by GU's ordinary interpolator, with no action restart.
    flight_action, guided, coast = flight_packets
    serial = struct.unpack_from("<I", flight_action, 16)[0] >> 26
    assert serial and struct.unpack_from("<H", flight_action, 56)[0] == 0x8000 + 68
    assert struct.unpack_from("<I", guided, 44)[0] >> 16 == 0x4844
    flags = struct.unpack_from("<I", coast, 44)[0]
    assert flags >> 16 == 0x4944 and (flags >> 8) & 63 == serial
    position_packet = g.alloc(48)
    g.cpu.hook_del(g.traps.pop(0x280548))

    def position(actor, packet):
        g.cpu.mem_write(position_packet, packet)
        g.run(0x1FFFF4, position_packet, actor)
        assert g.cpu.mem_read(actor + 5248, 12) == packet[16:28]
        assert g.get(actor + 5280) == struct.unpack_from("<H", packet, 32)[0]

    receive(first, flight_action)
    frame(first, 8.)
    assert g.half(first + 1204) == 1460
    position(first, guided)
    assert struct.unpack("<f", g.cpu.mem_read(first + 5244, 4)) == (20.,)
    before = struct.unpack("<3f", g.cpu.mem_read(first + 64, 12))
    destination = struct.unpack_from("<3f", guided, 16)
    g.cpu.mem_write(first + 28, struct.pack("<f", 2.))
    g.run(0x2806E0, first)
    after = struct.unpack("<3f", g.cpu.mem_read(first + 64, 12))
    expected = tuple(a + (b - a) / 10 for a, b in zip(before, destination))
    assert all(abs(a - b) < 0.001 for a, b in zip(after, expected)), (after, expected)
    frame(first, 34.)
    assert g.half(first + 1204) == 1460
    position(first, coast)
    frame(first, 40.)
    assert g.half(first + 1204) == 1461 and g.half(first + 9482) == 0x8000 + 68
    position(first, coast)
    frame(first, 2.)
    assert motions.count((first, 1461)) == 1
    assert all(owner != second for owner, clip in motions if clip == 1461)

    # Strengthened hits are constructed from the sender's active forms.
    boosted_slash, boosted_impact, idle_boost, expired_slash, expired_impact = boost_packets
    for actor, (packet, snapshot), state, contact, power in (
        (first, boosted_slash, 78, 40., 47), (second, boosted_impact, 79, 30., 48),
    ):
        receive(actor, packet)
        assert g.half(actor + 9482) == 0x8000 + state
        position(actor, snapshot)
        frame(actor, 0.)
        frame(actor, 60., ended=True)
        frame(actor, contact)
        assert g.half(actor + 12320 + 56) == power

    # Receiver-side sparks use the slower FZ interval and the actor's
    # weapon joint. The activation flash and larger pulses stay with the sender.
    clock = g.get(0x2126764) + 104
    stopped = []
    stub(0x32A400, lambda unit, immediate, *_: stopped.append((unit, immediate)))

    def observe(frames):
        g.cpu.mem_write(clock, struct.pack("<f", frames))
        g.run(DISPATCH, local)

    first_effect = len(effects)
    observe(50.)
    assert len(effects) == first_effect
    observe(2.)
    assert effects[first_effect:] == (
        [(i, 0, (105., 90., 140.)) for i in range(168, 174)]
        + [(i, 0, (60., 90., 260.)) for i in range(174, 180)]
    )
    for actor in (first, second):
        g.cpu.mem_write(actor + 4180, b"\1")
    observe(0.)
    assert len(stopped) == 12 and all(immediate == 1 for _, immediate in stopped)
    for actor in (first, second):
        g.cpu.mem_write(actor + 4180, b"\0")
        g.cpu.mem_write(actor + 10057, b"\0")
    observe(50.)
    assert len(effects) == first_effect + 12
    g.cpu.mem_write(clock, struct.pack("<f", 2.))

    # The sender stays in GU idle for both expirations. Only position
    # packets follow the idle action, including duplicates and a late one.
    def replay_swing(packet, state, start, swing, contact, power):
        for offset in (12320, 12504):
            g.run(0x1687A8, first + offset)
        receive(first, packet)
        assert g.half(first + 9482) == 0x8000 + state
        frame(first, 0.)
        assert g.half(first + 1204) == start
        frame(first, 60., ended=True)
        assert g.half(first + 1204) == swing
        frame(first, contact)
        actual = g.half(first + 12320 + 56)
        assert actual == power, (state, actual, power)

    receive(first, idle_boost[0])
    assert g.half(first + 9478) == 1 and g.half(first + 9482) == 0
    g.cpu.mem_write(first + 10057, b"\1")
    position(first, idle_boost[1])
    position(first, expired_slash)
    replay_swing(boosted_slash[0], 78, 1440, 1049, 40., 43)
    replay_swing(boosted_impact[0], 79, 1023, 1413, 30., 48)
    receive(first, idle_boost[0])
    g.cpu.mem_write(first + 10057, b"\1")
    position(first, expired_impact)
    position(first, idle_boost[1])
    position(first, expired_slash)
    position(first, expired_impact)
    assert g.half(first + 9478) == 1 and g.half(first + 9482) == 0
    first_effect = len(effects)
    observe(60.)
    assert len(effects) == first_effect
    g.cpu.mem_write(clock, struct.pack("<f", 2.))
    replay_swing(boosted_impact[0], 79, 1023, 1413, 30., 44)

    position(second, expired_impact)
    for actor, packet, state, motion, contact, power in (
        (first, guard_packets[0], 77, 1034, 32., 30),
        (second, guard_packets[1], 115, 1410, 24., 73),
    ):
        first_sound = len(sounds)
        first_effect = len(effects)
        receive(actor, packet)
        assert g.half(actor + 9482) == 0x8000 + state
        frame(actor, 0.)
        assert g.half(actor + 1204) == motion
        for hit in (actor + 12320, actor + 12504):
            g.run(0x1687A8, hit)
        frame(actor, contact)
        actual = g.half(actor + 12320 + 56)
        assert actual == power, (state, actual, power)
        if state == 77:
            assert [cue for owner, name, cue, _ in sounds[first_sound:]
                    if owner == actor and name == "blade"] == [13, 12]
            assert [(selector, parent) for selector, parent, _ in effects[first_effect:]] == [
                (i, actor + 600) for i in list(range(13, 37)) + list(range(93, 99))
            ]
        else:
            assert len(effects) == first_effect

    # A peer confirms the impact guard with state 113, before either assault.
    first_effect = len(effects)
    receive(second, guard_packets[2])
    frame(second, 0.)
    frame(second, 1.)
    assert [selector for selector, _, _ in effects[first_effect:]] == (
        list(range(37, 70)) + list(range(99, 105))
    )
    assert all(parent == second + 600 for _, parent, _ in effects[-6:])

    # Remote playback uses the sender's shot action and its source event.
    shot_packet, shot_events, (spawn_shot, stop_shot, visuals) = shot_data
    first_sound = len(sounds)
    receive(second, shot_packet)
    frame(second, 0.)
    frame(second, 11.)
    frame(second, 12.)
    frame(second, 12.)
    assert [cue for owner, _, cue, _ in sounds[first_sound:] if owner == second] == [8, 74, 9]

    stub(0x3267D0, spawn_shot)
    stub(0x32A400, stop_shot)
    # A native-weapon receiver still advances peer effects. Use the sender's
    # origin to compensate for the receiver's lagging position interpolation.
    g.cpu.mem_write(first + 64, struct.pack("<3f", 10., 0., 0.))
    g.cpu.mem_write(second + 64, bytes(12))
    hit_origin = struct.unpack_from("<3f", shot_events[0], 16)
    origin = tuple(a + b for a, b in zip(hit_origin, (15., 100., 50.)))
    hp_before = g.get(status + 880)
    position(first, shot_events[0])
    position(first, shot_events[0])
    position(second, shot_events[1])  # Reordered stop cannot be undone by the late launch.
    position(second, shot_events[0])
    assert visuals() == [(i, origin) for i in range(8)]
    g.cpu.mem_write(clock, struct.pack("<f", 0.))
    g.run(DISPATCH, local)
    assert visuals() == [(i, origin) for i in range(8)]
    g.cpu.mem_write(clock, struct.pack("<f", 2.))
    g.run(DISPATCH, local)
    assert visuals() == [(i, (origin[0], origin[1], origin[2] + 500.)) for i in range(8)]
    position(first, shot_events[1])
    position(first, shot_events[0])
    assert not visuals() and g.get(status + 880) == hp_before
    assert g.half(second + 9482) == 0x8000 + 84

    # Sloped shots use the sender's signed pitch. Changing area retires the
    # effect even if its final packet is missing.
    position(second, shot_events[6])
    start = visuals()[0][1]
    g.run(DISPATCH, local)
    moved = visuals()[0][1]
    assert abs(moved[0] - start[0]) < .001
    assert abs((moved[1] - start[1]) + 353.5534) < .001
    assert abs((moved[2] - start[2]) - 353.5534) < .001
    g.cpu.mem_write(second + 4180, b"\1")
    g.run(DISPATCH, local)
    assert not visuals()
    g.cpu.mem_write(second + 4180, b"\0")
    position(second, shot_events[4])
    assert len(visuals()) == 8
    g.cpu.mem_write(clock, struct.pack("<f", 150.))
    g.run(DISPATCH, local)
    assert not visuals()
    g.cpu.mem_write(clock, struct.pack("<f", 2.))

    receive(first, repeated_flight)
    frame(first, 8.)
    position(first, coast)  # Old serial, same GU action ID.
    frame(first, 34.)
    assert g.half(first + 1204) == 1460
    stub(0x280548)

    # GU's common roll reads the independent model's idle weapon pose. Swap
    # the received modes to prove the part update uses the packet for each peer.
    def track(owner, layer, clip, _):
        g.put(owner + 1204 + 320 * layer, clip)
        return 1

    stub(0x954640, track)
    for actor, weapon, packet, slot in (
        (first, first_part, slash_roll, 1), (second, second_part, impact_roll, 2),
    ):
        receive(actor, packet)
        g.cpu.mem_write(actor + 10057, b"\x01")
        g.cpu.mem_write(actor + 1204, b"\0\0")
        g.run(g.hooks[0x3078B8], weapon, actor, 6)
        assert g.half(weapon + 1204) & 255 == slot, (slot, hex(g.half(weapon + 1204)))

    # A replaced native record invalidates its SDK equipment identity even if
    # the raw fallback category and the GU actor pointer have stayed the same.
    g.run(0x274400, manager, 1, g.get(first + 5060))
    receive(first, impact)
    assert g.half(first + 9482) != 0x8000 + 79
    assert meter() == ([], 0)
    g.put(local + 5060, equipment)
    assert meter() == ([16.] * 5, 0)
