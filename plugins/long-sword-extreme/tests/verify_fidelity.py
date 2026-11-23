"""Boundary replays for the w07 native branches using the built NRO and GU 1.4."""

import math
import struct
from pathlib import Path

from unicorn import arm_const as arm
from verify_style import BLOCK, CHANGE_ACTION, api_for

ROLL_ATTACKS = ((0, 0), (2, 20))  # GU weapon command and Frontier destination.


def replay(g, actor, descriptor, set_action, allow_transition):
    controller = g.get(descriptor + 20)
    registry = api_for(g, b"mhgu::StyleRegistryV1", b"SRG1")
    inputs = g.alloc(40)
    motions = []

    def stub(pc, fn):
        if pc in g.traps:
            g.cpu.hook_del(g.traps.pop(pc))
        g.stub(pc, fn)

    def put_float(addr, value):
        g.cpu.mem_write(addr, struct.pack("<f", value))

    def half(addr, value):
        g.cpu.mem_write(addr, struct.pack("<H", value & 65535))

    def number(addr):
        return struct.unpack("<f", g.cpu.mem_read(addr, 4))[0]

    def playhead(who, frame, advance=1.):
        g.cpu.mem_write(who + 1276, struct.pack("<2f", frame + advance, frame))

    def motion(_, layer, request, flags):
        blend, frame = (
            struct.unpack("<f", struct.pack("<I", g.cpu.reg_read(r)))[0]
            for r in (arm.UC_ARM_REG_S0, arm.UC_ARM_REG_S1)
        )
        clip = request & 65535
        motions.append((clip, frame, blend))
        put_float(actor + 1292, 1.0)
        half(actor + 1204, clip)
        playhead(actor, frame, 0.)
        return 1

    def begin(state):
        g.run(controller, 0, 0)
        set_action(16, 0x8000 + state)
        g.cpu.mem_write(inputs, bytes(40))
        g.put(actor + 4076, 0)
        g.put(actor + 5280, 0)
        put_float(actor + 5296, 1.0)
        g.cpu.mem_write(actor + 10156, bytes(8))
        g.put(actor + 12016, 0)
        g.put(actor + 10112, 100)
        g.put(actor + 1280, 0)
        put_float(actor + 28, 2)
        put_float(actor + 1284, 240)
        put_float(actor + 5088, 900)
        g.run(controller, actor, inputs)

    def tick(frame, yaw=0, weapon=0, held=0, common=0, auxiliary=0, strength=120):
        half(actor + 5132, 0x4000 - yaw)
        half(actor + 5136, strength)
        half(actor + 5108, held)
        half(inputs + 34, strength)
        g.cpu.mem_write(inputs, common.to_bytes(16, "little"))
        g.put(inputs + 16, weapon)
        g.put(inputs + 36, auxiliary)
        playhead(actor, frame)
        g.run(controller, actor, inputs)
        return g.half(actor + 9482) - 0x8000

    stub(0x53A06C, motion)
    g.cpu.hook_del(g.traps.pop(0x27E608))
    stub(0x27F678, lambda p, *_: (g.half(p + 5132) - 0x4000) & 65535)
    for addr in (0x27D5C4, 0x3A5ED0, 0x5584A0, 0x27F728):
        stub(addr, lambda *_: 0)
    shoulder_layout = g.get(0x1897F78) + 17518
    old_layout = bytes(g.cpu.mem_read(shoulder_layout, 1))
    def mapped(frame, held, previous):
        g.cpu.mem_write(actor + 5108, struct.pack("<3H", held, previous & ~held, held & ~previous))
        g.cpu.mem_write(actor + 5140, bytes(32))
        g.run(0x27CA28, actor)
        g.run(0x1199114, actor)
        common = int.from_bytes(g.cpu.mem_read(actor + 5140, 16), "little")
        return tick(frame, weapon=g.get(actor + 5156), held=held, common=common, strength=0)

    for layout, shoulder in ((0, 0x40), (1, 0x800)):
        g.cpu.mem_write(shoulder_layout, bytes([layout]))
        chord = shoulder | 0x120
        for presses in (
            (0x120,), (0x20, 0x120), (0x100, 0x120), (chord,), (shoulder, chord),
            (shoulder,) * 4 + (chord,),
        ):
            begin(0)
            g.cpu.mem_write(actor + 5188, bytes(8))
            previous = 0
            for frame, held in (*zip(range(44, 54, 2), presses), (78, 0), (80, 0)):
                active = mapped(frame, held, previous)
                previous = held
            assert active == 32, (layout, presses, active)
        for use_chord, dest in ((False, 7), (True, 0)):
            begin(0)
            put_float(actor + 28, 1)
            g.cpu.mem_write(actor + 5188, bytes(16))
            previous = 0
            # X+A arrives just before the evade-slash window. It also consumes
            # the held shoulder so releasing it cannot later queue Spirit I.
            for frame in range(18, 46):
                held = (chord if use_chord else shoulder) if 43 <= frame < 45 else (
                    shoulder if frame < 43 else 0)
                assert mapped(frame, held, previous) == 0
                previous = held
            assert mapped(80, 0, 0) == dest, (layout, use_chord)
    g.cpu.mem_write(shoulder_layout, old_layout)
    print("GU LS evade slash: X+A survives overlapping weapon shoulder input in both layouts")

    stub(0x2A8458, lambda *_: (_ for _ in ()).throw(
        AssertionError("sprint used the forward ledge probe")))
    begin(85)
    assert tick(10, strength=0) == 85
    assert tick(28) == 85
    assert tick(30, strength=0) == 86
    g.cpu.hook_del(g.traps.pop(0x2A8458))
    for weapon, dest in ((1, 0), (4, 20)):
        begin(85)
        assert tick(29.5, weapon=weapon) == 85
        assert tick(30) == 85
        put_float(actor + 1268, 1)
        assert tick(30, weapon=weapon) == 85
        put_float(actor + 1268, 0)
        assert tick(30, weapon=weapon) == dest
        assert motions[-1] == ((1401, 10, 2) if dest == 0 else (1416, 0, 1))
    for yaw, expected in (
        (0x4000, 35),
        (-0x4000, 33),
        (0x8000, 7),
        (0x2000, 7),
        (0x6000, 35),
        (-0x6000, 33),
        (-0x6001, 7),
    ):
        begin(7)
        tick(46, yaw, weapon=4)
        assert tick(56, yaw) == expected, (yaw, expected, g.half(actor + 9482))
    for yaw, expected in ((0x4000, 0xE000), (-0x4000, 0x2000), (0x8000, 0)):
        begin(33)
        put_float(actor + 1284, 40)
        tick(4, -yaw)
        tick(20, yaw)
        assert tick(40, 0) == 34 and g.half(actor + 4076) == expected
    begin(7)
    assert tick(46, weapon=1) == 7
    assert tick(56) == 8
    put_float(actor + 1284, 24)
    assert tick(24) == 8 and motions[-1] == (1405, 16, 0)
    put_float(actor + 1284, 130)
    assert tick(28, 0x4000, weapon=4) == 8
    assert tick(38, 0x4000) == 8
    assert tick(40, common=(1 << 70) | (1 << 27)) == 8
    assert tick(42) == 27
    begin(7)
    assert tick(46, 0x4000, weapon=4) == 7
    assert tick(56, 0x4000) == 35
    put_float(actor + 1284, 40)
    assert tick(40, 0x4000) == 34
    put_float(actor + 1284, 160)
    assert tick(66, common=(1 << 70) | (1 << 27)) == 34
    assert tick(70) == 45
    for source, first_frame, execute, expected in (
        (72, 32, 58, 7), (74, 22, 64, 7), (75, 22, 64, 7), (0, 24, 80, 56),
    ):
        begin(source)
        for frame in range(first_frame, execute, 2):
            common = (1 << 27) | (1 << 70 if frame == first_frame else 0)
            assert tick(frame, common=common) == source
        actual = tick(execute, common=1 << 27)
        assert actual == expected, (source, actual, expected)
    begin(80)
    assert tick(10, weapon=1) == 80
    allow_transition(False)
    assert tick(20, 0x4000) == 80
    assert g.half(actor + 4076) == 0
    allow_transition(True)
    assert tick(22, -0x4000) == 77
    assert g.half(actor + 4076) == 0x4000
    # The A counter thrust turns one 22.5 degree sector toward the stick when
    # it executes; neutral, straight ahead and straight behind keep the facing.
    for address, trig in ((0x13EFDE8, math.sin), (0x13EFDF4, math.cos)):
        def imported_trig(*_, trig=trig):
            reg = arm.UC_ARM_REG_S0
            angle = struct.unpack("<f", struct.pack("<I", g.cpu.reg_read(reg)))[0]
            g.cpu.reg_write(reg, struct.unpack("<I", struct.pack("<f", trig(angle)))[0])

        stub(address, imported_trig)
    g.cpu.mem_write(actor + 96, struct.pack("<3f", 1., 1., 1.))

    def check_counter_pose(facing):
        g.run(0x8A8384, actor)
        g.run(0x890E20, actor)
        angle = facing * math.tau / 65536
        forward = struct.unpack("<3f", g.cpu.mem_read(actor + 208, 12))
        assert math.dist(forward, (math.sin(angle), 0., math.cos(angle))) < 1e-5, forward

    for source, yaw, strength, facing in (
        (79, 0x4000, 120, 0xF000),
        (80, -0x4000, 120, 0x1000),
        (80, 0x8000, 120, 0),
        (80, 0, 120, 0),
        (80, 0x2000, 0, 0),
        (80, 0x2000, 120, 0xF000),
    ):
        begin(source)
        g.run(0x53A9FC, actor)
        assert tick(10, 0x4000, weapon=4) == source
        allow_transition(False)
        assert tick(20, 0x4000) == source
        assert g.half(actor + 4076) == 0
        allow_transition(True)
        assert tick(22, yaw, strength=strength) == 81
        assert g.half(actor + 4076) == facing, (yaw, g.half(actor + 4076))
        assert g.half(actor + 5280) == facing
        # The new clip must already have its yaw before GU evaluates the pose.
        check_counter_pose(facing)
        # Simulate a native yaw write between ticks. The counter must retain
        # its committed aim before the first blade effect at frame 36.
        g.put(actor + 4076, 0)
        g.put(actor + 5280, 0)
        assert tick(36, -yaw) == 81
        assert g.half(actor + 4076) == g.half(actor + 5280) == facing
        g.run(0x53A9FC, actor)
        check_counter_pose(facing)
    for yaw, facing in ((0x8000, 0), (0x4000, 0xF000), (-0x4000, 0x1000)):
        begin(0)
        # Buffer a neutral A first, then steer only on the execution frame.
        put_float(actor + 1280, 44)
        g.put(inputs + 16, 4)
        g.run(controller, actor, inputs)
        assert tick(80, yaw) == 47 and g.half(actor + 4076) == facing, (
            yaw,
            g.half(actor + 4076),
        )
    begin(47)
    for frame in range(2, 32, 2):
        state = tick(frame, held=0 if frame == 10 else 0x100)
        assert state == (47 if frame < 30 else 49)
    for state, frame, common in ((85, 30, 8), (71, 4, (1 << 27) | 8)):
        begin(state)
        tick(frame, 0x4000, common=common)
        assert g.half(actor + 9482) == 6 and g.run(0x27C6A0, actor, 28) == 28

    # Let GU maintain its actual weapon-drawn and guard flags for draw-parry.
    for address in (0x2A2444, 0x2A2560, 0x283238):
        g.cpu.hook_del(g.traps.pop(address))
    dispatch = 0x282DA8
    stub(g.originals[dispatch], lambda *_: 0)
    shoulder_layout = g.get(0x1897F78) + 17518
    old_layout = bytes(g.cpu.mem_read(shoulder_layout, 1))
    for layout, dash_pressed, parry_pressed in ((0, 0x40, 0x800), (1, 0x800, 0x40)):
        g.cpu.mem_write(shoulder_layout, bytes([layout]))
        actions = (((1, 0), 78), ((2, 0), 78), ((2, 1), 78), ((2, 2), 78),
                   ((2, 3), 78), ((2, 5), 78), ((2, 13), 78), ((2, 14), 78), ((2, 15), 78),
                   ((1, 4), 4 - 0x8000), ((4, 14), 14 - 0x8000))
        for action, expected in actions:
            begin(0)
            set_action(*action)
            g.cpu.mem_write(actor + 10057, b"\x00")
            half(actor + 5136, 120)
            if action == (2, 2):
                started = []
                stub(0x281B4C, lambda _, clip, *__: started.append(clip) or half(actor + 1204, clip))
                half(actor + 1204, 0)
                g.run(0x2E5810, actor)
                g.cpu.hook_del(g.traps.pop(0x281B4C))
                assert started == [3] and g.get(actor + 9488) & 256
            if action[0] == 2 and action[1] in (13, 14, 15):
                g.put(actor + 9488, 0x181 if action[1] == 14 else 0x8181)
                g.cpu.mem_write(actor + 10057, b"\x02")
            # Exercise 2A79CC's real sprint input filter before the style
            # dispatch. +5124 is the GU snapshot from before the filter.
            held = dash_pressed | parry_pressed
            half(actor + 5108, held)
            half(actor + 5110, held)
            half(actor + 5112, parry_pressed)
            g.put(actor + 5124, parry_pressed)
            g.put(actor + 10096, 1)
            for register, value in ((arm.UC_ARM_REG_R4, actor),
                                    (arm.UC_ARM_REG_R6, actor + 5112),
                                    (arm.UC_ARM_REG_R8, 5114),
                                    (arm.UC_ARM_REG_R11, actor + 5108),
                                    (arm.UC_ARM_REG_R9, 0),
                                    (arm.UC_ARM_REG_R10, actor + 5110)):
                g.cpu.reg_write(register, value)
            g.run(0x2A7D68, until=0x2A7DC0)
            assert g.half(actor + 5112) == 0
            assert g.half(actor + 5108) == dash_pressed
            g.put(actor + 10096, 0)
            # Idle still uses the ordinary, filtered-input route.
            if action == (1, 0):
                half(actor + 5112, parry_pressed)
            g.run(g.hooks[dispatch], actor)
            assert g.half(actor + 9482) == 0x8000 + expected, (action, layout)
            g.put(actor + 5124, 0)
            half(actor + 5112, 0)
            if expected == 78:
                assert motions[-1] == (1450, 0, 0)
                assert g.get(actor + 10057) & 1 and g.get(actor + 9488) & 16
                g.put(actor + 12820, 0)
                half(actor + 12828, 0)
                assert g.run(g.hooks[0x2F2BA0], actor, 0) == 1
                tick(32)
                assert g.get(actor + 9488) & 16 == 0
                assert g.run(g.hooks[0x2F2BA0], actor, 0) == 0
    g.cpu.hook_del(g.traps.pop(g.originals[dispatch]))
    g.cpu.mem_write(shoulder_layout, old_layout)
    print("GU draw-parry: both shoulder layouts, regular and panic sprint filtering, and airborne rejection passed")

    # The retail w07 guard rejects Guard Up attacks even with the skill.
    skills = set()
    stub(0x2864C4, lambda _, skill, *__: int(skill in skills))
    g.put(actor + 5068, 1)
    for skill, frame, delta, flags, expected in (
        (0, 8, 0, 0x20, 0),
        (171, 8, -0x4000, 0x20, 1),
        (317, 8, 0x4000, 0x20, 1),
        (171, 8, 0x4001, 0x20, 0),
        (171, 8, 0x8000, 0x20, 0),
        (171, 8, 0, 0x120, 0),
        (171, 28, 0, 0x20, 0),
    ):
        skills.clear()
        skills.add(skill)
        begin(0)
        assert tick(68, -0x4000, auxiliary=1) == 78
        assert g.half(actor + 5280) == 0x1000
        tick(frame)
        g.put(actor + 12820, flags)
        half(actor + 12828, 0)
        angle = (0x1000 + delta) & 0xffff
        assert g.run(g.originals[BLOCK], actor, angle) == 0
        assert g.run(g.hooks[BLOCK], actor, angle) == expected, (skill, frame, delta, flags)

    skills.clear()
    skills.add(171)
    begin(78)
    tick(8)
    g.put(actor + 12820, 0x20)
    half(actor + 12824, 0)
    half(actor + 12826, 123)
    damage, absorbed = g.alloc(4), g.alloc(4)
    g.put(damage, 35)
    stub(0x27BCD0, lambda *_: 1)
    stub(g.originals[CHANGE_ACTION], lambda _, group, slot, __: set_action(group, slot) or 1)
    stub(0x2C4EEC, lambda *_: 0)
    assert g.run(0x2F0670, actor, 0, damage, absorbed) == 1
    assert (g.half(actor + 9478), g.half(actor + 9482)) == (32, 3)
    assert tick(0) == 80
    assert tick(20, weapon=1) == 77
    assert g.run(g.hooks[BLOCK], actor, 0) == 0
    g.put(actor + 12820, 0)
    for address in (0x27BCD0, g.originals[CHANGE_ACTION], 0x2C4EEC):
        g.cpu.hook_del(g.traps.pop(address))
    stub(0x2864C4, lambda *_: 0)
    print("GU LS Guard Up: native skill tiers, guard boundaries, recoil and instant slash passed")

    g.stub(0x2A2444, lambda *_: 1)
    for address in (0x2A2560, 0x283238):
        g.stub(address, lambda *_: 0)

    # Replay an accepted GU guard: the result arrives in a different native
    # action whose animation clock has already restarted.
    result, out = [0], g.alloc(12)
    stub(g.get(registry + 12), lambda *_: result.pop() if result else 0)
    for contact, expected in ((1, 79), (2, 80)):
        begin(78)
        tick(2)
        g.put(actor + 9488, 16)
        assert g.run(g.get(descriptor + 60), actor, out) == 2
        result.append(contact)
        set_action(32, 3)
        for elapsed in (2, 4, 6, 8):
            dst = tick(100 + elapsed)
            assert dst == (3 - 0x8000 if elapsed < 8 else expected), (elapsed, dst)

    # Exercise the real registry binding for the native drawn roll table.
    stock, entries, terrain, attack, climb, slash = (g.alloc(n) for n in (24, 8, 24, 24, 124, 124))
    g.cpu.mem_write(stock, struct.pack("<6I", 0, 0, 2, 2, 0, entries))
    g.cpu.mem_write(entries, struct.pack("<2I", terrain, attack))
    for entry, resource, group, slot in ((terrain, climb, 1, 137), (attack, slash, 16, 41)):
        g.put(entry + 8, resource)
        g.cpu.mem_write(resource + 100, struct.pack("<2H", group, slot))
    stub(g.originals[0x2E9B7C], lambda *_: g.put(actor + 9384, stock))
    set_action(1, 6)
    g.put(actor + 5068, g.get(actor + 5068) | 1)
    g.run(g.hooks[0x2E9B7C], actor, 2023, 0)
    table = g.get(actor + 9384)
    assert table != stock and g.get(table + 8) == 1
    assert g.get(g.get(table + 20)) == terrain
    # Restore the fixture's native transition backend for the remaining moves.
    g.put(actor + 9384, 0)

    # The resource loader returns a loaded blade group. Keep the style's
    # event selection, both native deflection hooks and +148 contact dispatch live.
    shape, vt, getter, group, rows = [g.alloc(n) for n in (112, 72, 4, 12, 12)]
    g.put(shape, vt)
    g.put(shape + 80, 1)
    g.put(vt + 68, getter)
    g.put(group + 4, 3)
    g.put(group + 8, rows)
    g.stub(getter, lambda *_: group)
    loader = g.get(g.get(g.get(0x2126A64)) + 48)
    stub(loader, lambda _, kind, name, __: shape if kind == 0x1857810 else 0)
    submits = []
    def submit(_, atk, *__):
        submits.append((g.half(atk + 2), number(atk + 4), number(atk + 8)))
        # Match 168960's copy of the equipped w07 HDP row 1 into the hit.
        g.cpu.mem_write(actor + 12320 + 87, bytes([26]))
        return 1
    stub(g.get(registry + 8), submit)
    for frame, expected in ((59, [(40, 0, 7)]), (66, [])):
        begin(0)
        count = len(submits)
        tick(frame)
        tick(frame)
        assert submits[count:] == expected
    begin(20)
    count = len(submits)
    put_float(actor + 1284, 24)
    tick(24)
    assert motions[-1] == (1407, 20, 0) and submits[count:] == [(28, 0, 10)]
    # A second R tap after the running slash enters Spirit I. Its contact
    # must be armed on the active clip, after any pause or skipped frame.
    for gauge, state, power in ((100, 7, 37), (0, 18, 17)):
        begin(71)
        for frame in range(2, 94, 2):
            assert tick(frame, common=1 << 27) == 71
        assert tick(94) == 72
        count = len(submits)
        assert tick(18) == 72
        assert submits[count:] == [(70, 0, 14)]
        g.put(actor + 10112, gauge)
        assert tick(48, common=(1 << 70) | (1 << 27)) == 72
        count = len(submits)
        assert tick(50) == state and motions[-1][:2] == (1404, 8)
        for frame in (8, 8, 46, 47):
            assert tick(frame) == state
        assert submits[count:] == []
        assert tick(49) == state
        assert tick(49) == state
        assert submits[count:] == [(power, 0, 7)]
    target, part, factor, element = [g.alloc(n) for n in (47000, 88, 4, 4)]
    hit = actor + 12320
    stub(0x119F574, lambda *_: 0)
    for resolver in (0x178030, 0x1748A8):
        stub(resolver, lambda *_: 0)

    def contact(ratio, resolver=0x178030):
        put_float(factor, ratio)
        g.run(g.hooks[resolver], 0, hit, target, part, part, actor, factor, element)
        g.run(g.hooks[0x119F574], actor, hit, target, 1)

    # Keep the real registry submission and deflection service in this replay.
    # The multiplayer fixture separately runs the original attack constructor.
    submit_attack = g.get(registry + 8)
    g.cpu.hook_del(g.traps.pop(submit_attack))
    g.put(actor + 12700, shape)
    hdp = g.alloc(56)
    row_lookup = g.originals[0x2A8A88]
    stub(row_lookup, lambda *_: hdp)
    stub(0xDF814, lambda *_: g.get(actor + 5060))
    stub(0xDD4C4, lambda *_: 0)
    hits = []

    def construct(*_):
        address = g.get(g.cpu.reg_read(arm.UC_ARM_REG_SP))
        hits.append(address)
        half(address + 52, len(hits))
        return 1

    stub(0x2B72D4, construct)
    for ratio, dest in ((0.2299, 3), (0.23, 0), (0.45, 0), (1.0, 0)):
        count = len(hits)
        begin(0)
        tick(54)
        assert len(hits) == count
        tick(56)
        assert hits[count:] == [hit]
        motions.clear()
        contact(ratio)
        contact(0.1, 0x1748A8)  # Later targets must not override the first contact.
        native_timers = (9., 0.) if ratio == 1.0 else (0., 6.) if ratio == 0.45 or dest == 3 else (0., 0.)
        g.cpu.mem_write(actor + 10156, struct.pack("<2f", *native_timers))
        assert tick(56) == dest
        if dest == 3:
            assert motions[-1][:2] == (1413, 0)
            assert (number(actor + 10156), number(actor + 10160)) == (0., 0.)
            tick(2, weapon=1)
            assert tick(22) == 17
        else:
            for frame in (56, 56, 58):
                assert tick(frame) == 0
            assert not motions
            assert len(hits) == count + 1
            assert (number(actor + 10156), number(actor + 10160)) == native_timers
            assert number(actor + 5296) == 1.0
    for fn in (row_lookup, 0x2B72D4, 0xDF814, 0xDD4C4):
        g.cpu.hook_del(g.traps.pop(fn))
    g.stub(submit_attack, submit)

    # Keep the full release tier after sheathed updates at zero and normal delta.
    drawn = [1]
    stub(0x2A2444, lambda *_: drawn[0])
    for sheath_delta in (0., 2.):
        begin(0)
        tick(56)
        for _ in range(32):
            g.run(g.hooks[0x119F574], actor, hit, target, 1)
        set_action(1, 0)
        drawn[0] = 0
        put_float(actor + 28, sheath_delta)
        tick(0)
        drawn[0] = 1
        state = tick(0, common=1 << 8)
        assert state == 84, (sheath_delta, state)

    from verify_hud import replay as replay_hud
    replay_hud(g, actor, begin, tick, set_action, drawn, target, shape)

    # Mark a live monster part through the actual primary hit callback. At
    # collision-pass time a GU damage action must discard the mark. A voluntary
    # roll or sheath must reach the launcher exactly once.
    part_vt, body, bone, getter, idx = [g.alloc(n) for n in (480, 460, 64, 4, 16)]
    g.put(target, part_vt)
    g.put(target + 5160, body)
    g.put(part_vt + 208, getter)
    g.cpu.mem_write(bone, struct.pack("<16f", *(float(i % 5 == 0) for i in range(16))))
    g.stub(getter, lambda *_: bone)
    stub(g.get(g.get(actor) + 208), lambda *_: bone)
    g.put(0x1851FE0, idx)
    stub(0x5CEB4, lambda *_: target)
    g.put(hit + 172, part)
    launched = []
    hdp = g.alloc(56)
    stub(0x2A8A88, lambda *_: hdp)
    row_group, row_ptr, shape_row = [g.alloc(n) for n in (12, 4, 48)]
    g.put(row_group + 4, 1)
    g.put(row_group + 8, row_ptr)
    g.put(row_ptr, shape_row)
    release_shape = g.alloc(112)
    g.put(release_shape, vt)
    g.put(release_shape + 80, 1)
    stub(
        g.get(vt + 68), lambda which, *_: row_group if which == release_shape else group
    )
    stub(
        loader,
        lambda _, kind, name, __: (
            (release_shape if g.string(name).endswith("release") else shape)
            if kind == 0x1857810
            else 0
        ),
    )

    def construct(obj, _, owner, resource):
        launched.append(g.half(resource + 4))
        return 0

    stub(0x168960, construct)
    manager = g.get(0x1857830)
    for tier, first, power in ((76, 14, 10), (83, 12, 60), (84, 12, 180)):
        for group_id in (64, 1):
            begin(tier)
            tick(first)
            g.run(g.hooks[0x119F574], actor, hit, target, 1)
            set_action(group_id, 6)
            half(actor + 1204, 28)
            launched.clear()
            g.run(g.hooks[0x16ABAC], manager)
            set_action(1, 0)
            g.run(g.hooks[0x16ABAC], manager)
            assert launched == ([] if group_id == 64 else [power]), (
                tier,
                group_id,
                launched,
            )

    # Observe the final camera transform inside the engine's view callback.
    view = g.alloc(2400)
    g.put(view, 0x1730D0C)
    g.put(view + 2140, actor)
    g.cpu.mem_write(actor + 64, bytes(12))
    seen = []
    stub(g.originals[0x898AC0], lambda camera, *_: seen.append(number(camera + 68)) or 0)
    for index, amp, duration in ((54, 4, 10), (60, 10, 20), (65, 12, 30)):
        begin(index)
        put_float(actor + 1284, 10)
        tick(10)
        put_float(actor + 1284, 240)
        tick(122)
        g.put(view, 0x1730C28)
        g.run(g.hooks[0x898AC0], view)
        assert seen[-1] == 0
        g.put(view, 0x1730D0C)
        g.run(g.hooks[0x898AC0], view)
        assert abs(abs(seen[-1]) - amp) < 0.001, (
            index,
            seen[-1],
            motions[-5:],
            g.half(actor + 9482),
        )
        put_float(view + 28, 2 * duration)
        assert number(view + 68) == number(view + 100) == 0
        g.run(g.hooks[0x898AC0], view)
        assert seen[-1] == 0
        put_float(view + 28, 0)
    assert any(clip == 1443 and blend == 1 for clip, _, blend in motions)
    # Run the charge controller over a dry and submerged sample of the same
    # rotated node offset. Capture GU particle selectors and positional cues.
    routes = list(
        struct.iter_unpack(
            "<HH",
            (
                Path(__file__).resolve().parents[1] / "target/assets/effects/routes.bin"
            ).read_bytes(),
        )
    )
    terrain, particles, audio, resource = [g.alloc(n) for n in (100, 160, 512, 128)]
    g.put(resource + 80, 1)
    g.put(0x21270E0, terrain)
    g.put(0x188A0D4, particles)
    g.put(0x1896F5C, audio)
    stub(loader, lambda _, kind, *__: resource if kind == 0x2125BEC else 0)
    emitted, cues, probes = [], [], []
    water = [-100001.0]
    floor_available = True
    probe_floor = 12.

    def floor(_, owner, point, end):
        probes.append(struct.unpack("<3f", g.cpu.mem_read(point, 12)))
        if not floor_available:
            return 0
        sp = g.cpu.reg_read(arm.UC_ARM_REG_SP)
        put_float(g.get(sp + 4), probe_floor)
        put_float(g.get(sp + 8) + 20, water[0])
        return 1

    stub(0x18192C, floor)
    stub(
        0x3267D0,
        lambda *_: emitted.append(g.get(g.cpu.reg_read(arm.UC_ARM_REG_SP))) or 1,
    )
    stub(0xB68280, lambda *_: 1)
    stub(0x4EEAA4, lambda _, kind, bank, cue: cues.append(cue - 4096) or 1)
    for surface in (-100001.0, 20.0):
        water[0] = surface
        begin(71)
        g.cpu.mem_write(actor + 64, struct.pack("<3f", 300.0, 12.0, 400.0))
        g.cpu.mem_write(actor + 224, struct.pack("<3f", 500.0, 70.0, 1000.0))
        emitted.clear()
        cues.clear()
        probes.clear()
        for frame in range(2, 131, 2):
            tick(frame, common=1 << 27)
        dry = surface < 0
        for route in (15, 16):
            first, count = routes[route]
            assert all((sel in emitted) == dry for sel in range(first, first + count))
        assert (8 in cues) == dry and (9 in cues) == dry, cues
        assert (380.0, 12.0, 960.0) in probes
    effects = []

    def capture(_, unit, *__):
        selector = g.get(g.cpu.reg_read(arm.UC_ARM_REG_SP))
        effects.append((selector, struct.unpack("<3f", g.cpu.mem_read(unit + 304, 12)),
                        struct.unpack("<3f", g.cpu.mem_read(unit + 160, 12))))
        emitted.append(selector)
        return 1

    stub(0x3267D0, capture)
    # GU's terrain under the trailing blade can be lower than the hunter.
    probe_floor = -60.
    water[0] = -100001.
    begin(71)
    g.cpu.mem_write(actor + 64, struct.pack("<3f", 300., 12., 400.))
    effects.clear()
    for frame in range(2, 65, 2):
        tick(frame, common=1 << 27)
    first, count = routes[15]
    heights = [pos[1] for sel, pos, _ in effects if first <= sel < first + count]
    assert heights and set(heights) == {12.}, heights
    probe_floor = 12.
    # Whiff all three cut-gauge tiers with a rotated and translated blade.
    stub(0xC1628, lambda *_: bone)
    g.put(actor + 1172, bone)
    g.put(actor + 1176, bone)
    # A rotated joint translated independently of the hunter root.
    g.cpu.mem_write(bone, struct.pack("<16f", 0, 1, 0, 0, -1, 0, 0, 0,
                                     0, 0, 1, 0, 700, 120, 300, 1))
    swing = [(4, 17), (14, 24), (28, 60), (34, 33), (36, 23),
             (54, 59), (56, 23), (60, 33)]
    footsteps = {i for first, count in routes[75:78] for i in range(first, first + count)}
    for tier, entry in ((76, 52), (83, 30), (84, 0)):
        for wet in (False, True):
            water[0] = 20 if wet else -100001
            begin(tier)
            if entry:
                put_float(actor + 1284, 24)
                tick(24)
                put_float(actor + 1284, 260)
            assert motions[-1][:2] == (1448, entry)
            g.put(actor + 4076, 0x4000)
            g.cpu.mem_write(actor + 64, struct.pack("<3f", 300, 12, 400))
            for at, route in swing:
                if at < entry:
                    continue
                effects.clear()
                if at > entry:
                    tick(at - 1)
                # History also emits short-lived strips while a window is active.
                blades = (routes[35][0], routes[67][0])
                assert all(sel in blades or sel in footsteps for sel, _, _ in effects), (tier, at, effects)
                effects.clear()
                tick(at)
                if wet and route in (59, 60):
                    route += 2
                first, count = routes[route]
                actual = [sel for sel, _, _ in effects if sel not in blades and sel not in footsteps]
                assert actual == list(range(first, first + count)), (tier, at, actual, route)
                if at in (14, 28, 54):
                    assert blades[int(at == 28)] in [sel for sel, _, _ in effects]
                _, pos, angles = next(item for item in effects if item[0] == first)
                expected = {4: (700, 120, 300), 14: (300, 12, 400),
                            28: (300, 20 if wet else 12, 400), 34: (700, 120, 300),
                            36: (730, 120, 300), 54: (300, 20 if wet else 12, 400),
                            56: (300, 12, 400), 60: (200, 12, 400)}[at]
                assert all(abs(a-b) < 0.01 for a,b in zip(pos, expected)), (at, pos)
                degrees = (0, 0, 90) if at in (34, 36) else (0, {4: 0, 28: 180}.get(at, 90), 0)
                assert all(abs(a-b) < 0.01 for a,b in zip(angles, degrees)), (at, angles)
                effects.clear()
                tick(at)
                assert all(sel in blades or sel in footsteps for sel, _, _ in effects), (at, effects)
    # FZ X/Y/Z angles of 20/35/50 degrees. The GU world proof must
    # reconstruct this blade basis with the native Ry * Rx * Rz order.
    g.cpu.mem_write(bone, struct.pack(
        "<16f", .5265408, .6275069, -.5735764, 0.,
        -.5937476, .7543013, .2801665, 0.,
        .6084559, .1930406, .7697511, 0.,
        700., 120., 300., 1.,
    ))
    begin(84)
    effects.clear()
    tick(34)
    first, _ = routes[33]
    angles = next(a for sel, _, a in effects if sel == first)
    expected = (360. - 11.1303, 38.3249, 39.7572)
    assert math.dist(angles, expected) < .02, angles
    print("GU cut-gauge release: three whiff entries, dry/wet selectors, compound blade rotation and frozen edges passed")
    # The clip dispatcher plays independently of monster contact.
    def layers(route):
        first, count = routes[route]
        return list(range(first, first + count))

    for state, samples in (
        (77, ((20, (59, 18)), (28, (21,)), (34, (21,)), (36, (76, 74)),
              (42, (23,)), (44, (76, 72)), (46, (33,)))),
        (79, ((2, (59, 20)),)),
        (80, ((2, (59, 20)),)),
        (85, ((2, (59,)), (22, (76,)))),
        (71, ((2, (32,)), (6, (76,)))),
    ):
        water[0] = -100001
        begin(state)
        hold_charge = (1 << 27) if state == 71 else 0
        for at, expected in samples:
            emitted.clear()
            tick(at, common=hold_charge)
            assert emitted == [i for route in expected for i in layers(route)], (state, at, emitted)
            emitted.clear()
            tick(at, common=hold_charge)
            assert not emitted, (state, at, emitted)

    water[0] = 20
    begin(77)
    emitted.clear()
    tick(20)
    assert emitted == layers(61) + layers(18)
    emitted.clear()
    tick(32)
    assert emitted == layers(75)
    emitted.clear()
    tick(36)
    assert emitted == layers(75) + layers(82)
    emitted.clear()
    tick(44)
    assert emitted == layers(75) + layers(80)
    begin(72)
    tick(24)
    emitted.clear()
    tick(32)
    assert emitted == layers(81)

    # FZ keeps the event's original height if there is no floor at its point.
    floor_available = False
    begin(84)
    emitted.clear()
    tick(28)
    assert set(layers(60)).issubset(emitted)
    floor_available = True
    water[0] = -100001

    units = []
    def capture_unit(_, unit, *__):
        selector = g.get(g.cpu.reg_read(arm.UC_ARM_REG_SP))
        units.append((selector, unit))
        return 1
    stub(0x3267D0, capture_unit)
    begin(71)
    units.clear()
    tick(2, common=1 << 27)
    opening = [unit for selector, unit in units if selector in layers(32)]
    assert len(opening) == len(layers(32))
    assert all(g.get(unit + 132) == 0xA9 and g.half(unit + 194) == 2 for unit in opening)
    for surface, ground_route in ((-100001., 59), (20., 61)):
        water[0] = surface
        begin(71)
        for frame in range(2, 102, 2):
            assert tick(frame, common=1 << 27) == 71
        assert tick(102) == 72
        units.clear()
        for frame in (18, 20, 22, 24):
            tick(frame)
        expected = layers(ground_route) + layers(23) + layers(21)
        blast = [unit for selector, unit in units if selector in expected]
        opening = [unit for selector, unit in units if selector in layers(18)]
        assert len(opening) == len(layers(18))
        assert [selector for selector, _ in units if selector in expected] == expected, (
            surface, ground_route, [selector for selector, _ in units]
        )
        assert all(g.cpu.mem_read(unit + 48, 1) == b"\0" for unit in blast)
        retired = blast.pop()
        g.run(g.hooks[0x32A104], retired)
        set_action(16, 0x8000 + 85)
        assert tick(0) == 85
        assert all(g.cpu.mem_read(unit + 48, 1) == b"\x02" for unit in blast)
        assert all(g.cpu.mem_read(unit + 48, 1) == b"\x02" for unit in opening)
        assert g.cpu.mem_read(retired + 48, 1) == b"\0"
        units.clear()
        tick(2)
        assert [selector for selector, _ in units] == layers(59)
        x, y, z = struct.unpack("<3f", g.cpu.mem_read(actor + 64, 12))
        assert all(struct.unpack("<3f", g.cpu.mem_read(unit + 304, 12)) == (x, y, z + 200.)
                   for _, unit in units)
    # The weak slash also clears on damage.
    water[0] = -100001.
    begin(72)
    units.clear()
    for frame in (18, 20, 22, 24):
        tick(frame)
    expected = layers(59) + layers(22)
    weak_blast = [unit for selector, unit in units if selector in expected]
    weak_opening = [unit for selector, unit in units if selector in layers(17)]
    assert len(weak_blast) == len(expected)
    assert len(weak_opening) == len(layers(17))
    set_action(64, 1)
    g.run(controller, actor, inputs)
    assert all(g.cpu.mem_read(unit + 48, 1) == b"\x02" for unit in weak_blast)
    assert all(g.cpu.mem_read(unit + 48, 1) == b"\x02" for unit in weak_opening)
    print("GU dash slash: dry/wet cleanup on sprint restart and weak slash damage interruption passed")

    # The parent-relative thrust overlay is retired when damage interrupts it.
    for state, frame, route in ((49, 16, 1), (52, 36, 2), (81, 36, 2)):
        begin(state)
        units.clear()
        tick(frame)
        thrust = [unit for selector, unit in units if selector in layers(route)]
        assert len(thrust) == len(layers(route))
        flags, joint = (0xA9, 12) if state == 81 else (0x29, 8)
        assert all(g.get(unit + 132) == flags and g.half(unit + 194) == joint for unit in thrust)
        set_action(64, 1)
        g.run(controller, actor, inputs)
        assert all(g.cpu.mem_read(unit + 48, 1) == b"\x02" for unit in thrust)
    stub(0x3267D0, capture)
    print("GU motion FX: instant, parry, sprint, charge, dry/wet frame edges and thrust interruption passed")

    begin(0)
    emitted.clear()
    g.run(g.hooks[0x119EE7C], actor)
    assert emitted == layers(78) + layers(79)
    emitted.clear()
    g.run(g.hooks[0x119EE7C], actor)
    assert not emitted  # Refresh the timer without replaying the full-gauge flash.

    # A guest can have a zero LS gauge in the receiver. Exercise received
    # motions with that value and with no local input or monster contact.
    peer = g.alloc(14000)
    g.cpu.mem_write(peer, bytes(g.cpu.mem_read(actor, 14000)))
    g.cpu.mem_write(peer + 4177, b"\x01")
    g.put(peer + 10112, 0)
    g.put(g.get(0x1885B58) + 32, peer)

    def peer_motion(who, _, request, flags):
        if who == actor:
            return motion(who, _, request, flags)
        half(who + 1204, request)
        playhead(who, 0., 0.)
        put_float(who + 1268, 0.)
        return 1

    stub(0x53A06C, peer_motion)
    for state, frame, route in ((0, 48, 64), (49, 16, 1), (52, 36, 2),
                                (77, 20, 18), (79, 2, 20), (81, 36, 2)):
        half(peer + 9478, 16)
        half(peer + 9482, 0x8000 + state)
        g.cpu.mem_write(peer + 5, b"\0")
        put_float(peer + 1284, 240.)
        g.run(0x282DA8, peer)
        emitted.clear()
        playhead(peer, frame)
        g.run(0x282DA8, peer)
        assert set(layers(route)).issubset(emitted), (state, emitted)

    stub(0x3267D0, capture_unit)
    half(peer + 9482, 0x8000 + 71)
    g.cpu.mem_write(peer + 5, b"\0")
    g.run(0x282DA8, peer)
    units.clear()
    for frame in range(2, 130, 2):
        playhead(peer, frame)
        g.run(0x282DA8, peer)
    auras = [unit for selector, unit in units if selector in layers(19)]
    assert len(auras) == 3
    for route in (15, 16):
        assert set(layers(route)).issubset(selector for selector, _ in units)
    half(peer + 9478, 64)
    half(peer + 9482, 1)
    g.run(g.hooks[0x16ABAC], g.alloc(8300))
    assert all(g.cpu.mem_read(unit + 48, 1) == b"\x02" for unit in auras)
    g.put(g.get(0x1885B58) + 32, 0)
    stub(0x53A06C, motion)
    stub(0x3267D0, capture)
    print("GU guest motion FX: zero-gauge trail, thrust, counter, instant, parry and charge interruption passed")

    # The discharge startup uses the existing ordinary spirit flash and model cue.
    emitted.clear()
    cues.clear()
    begin(54)
    first, count = routes[36]
    assert all(sel in emitted for sel in range(first, first + count)) and 12 in cues
    stub(0x3267D0, capture_unit)
    begin(57)
    put_float(actor + 1284, 40.)
    tick(40)
    assert motions[-1][:2] == (1437, 40)
    put_float(actor + 1284, 260.)
    for at in (40, 54, 84):
        units.clear()
        tick(at)
        charge_layers = [unit for selector, unit in units if selector in layers(8)]
        assert len(charge_layers) == len(layers(8))
        assert all(g.half(unit + 194) == 0xffff for unit in charge_layers)
    stub(0x3267D0, capture)
    # Execute GU's retail command evaluator before ticking the style's input buffer.
    # The scene state transition is the same fixture boundary used for GS rolls.
    g.run(controller, 0, 0)
    g.cpu.hook_del(g.traps.pop(0x2E9D54))

    def change(_, group, slot, flags):
        half(actor + 9480, g.half(actor + 9478))
        half(actor + 9484, g.half(actor + 9482))
        set_action(group, slot)
        g.cpu.mem_write(actor + 9384, bytes(64))
        return 1

    stub(g.originals[0x2AB670], change)
    for offset in (752, 756, 792, 796, 808, 812):
        stub(g.get(g.get(actor) + offset), lambda *_: 0)
    def roll():
        g.run(controller, 0, 0)
        set_action(1, 6)
        half(actor + 1204, 28)
        g.cpu.mem_write(actor + 5, b"\x02")
        g.cpu.mem_write(actor + 10057, b"\x01")
        g.cpu.mem_write(actor + 5188, bytes(8))
        g.cpu.mem_write(actor + 9392, bytes(48))
        g.put(actor + 4076, 0)
        g.put(actor + 5280, 0)
        put_float(actor + 28, 2)
        put_float(actor + 5088, 900)
        put_float(actor + 5328, 0)
        put_float(actor + 1268, 0)
        put_float(actor + 1284, 80)
        g.run(g.hooks[0x2E9B7C], actor, 2023, 0)

    for key, dest in ROLL_ATTACKS:
        roll()
        for frame in (4, 5, 6, 34, 35, 36):
            tick(frame, 0x4000, weapon=1 << key if frame == 6 else 0)
            if frame < 36:
                assert g.half(actor + 9482) == 6
        assert g.half(actor + 9482) == 0x8000 + dest, (key, g.half(actor + 9482))
        assert g.half(actor + 4076) == 0xf000
        roll()
        tick(5, weapon=1 << key)
        tick(36, strength=0)
        assert g.half(actor + 9482) == 6  # Pre-input opens at frame 6.

    for button, expected in ((0x20, 0), (0x100, 20)):
        for first_press, destination in ((6, expected), (22, 85)):
            roll()
            for frame in range(0, 38, 2):
                held = button if frame >= first_press else 0
                half(actor + 5108, held)
                half(actor + 5112, button if frame == first_press else 0)
                g.cpu.mem_write(actor + 5140, bytes(32))
                g.run(0x27CA28, actor)
                g.run(0x1199114, actor)
                playhead(actor, frame)
                put_float(actor + 5332, frame)
                g.run(0x2E9D54, actor)
                strength = 100 if frame == 10 or frame >= 14 else 0
                tick(frame, weapon=g.get(actor + 5156), strength=strength)
                if frame < 36:
                    assert g.half(actor + 9482) == 6
            assert g.half(actor + 9482) == 0x8000 + destination, (button, first_press)
    g.run(controller, 0, 0)
    print(
        "GU w07 directions, release latch, roll gates, parry, stock hitstop, deflection, sheathed gauge, running slash to Spirit I, delayed hits, terrain effects and camera passed"
    )
