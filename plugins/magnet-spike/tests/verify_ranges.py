"""Compare GU's packaged Magnet Spike shapes with the Vita collision evaluator."""
import math
import struct

from unicorn.arm_const import UC_ARM_REG_S0, UC_ARM_REG_S1
from verify_assets import Machine, MEM


def vita_contact_clock(elf, delay, duration):
    fz = Machine(elf, True)
    native = MEM + 0x1000
    fz.put(0x83AC1AA8, native)
    fz.trap(0x8166D520, lambda *_: 0)
    fz.c.mem_write(native, b"\1")
    fz.c.mem_write(native + 15, b"\1")
    fz.c.mem_write(native + 104, struct.pack("<2H", delay // 2, duration // 2))
    for _ in range((delay + duration) // 2 + 2):
        fz.call(0x817006A6)
        yield fz.c.mem_read(native + 15, 1)[0] == 2


def verify_contact_clock(g, hit, elf, delay, duration):
    """Advance a copy of the live NRO strike alongside Vita 817006A6."""
    probe = g.alloc(184)
    g.cpu.mem_write(probe, bytes(g.cpu.mem_read(hit, 184)))
    clock = g.get(0x2126764) + 104
    saved = bytes(g.cpu.mem_read(clock, 4))
    g.cpu.mem_write(clock, struct.pack("<f", 2.))
    try:
        for tick, active in enumerate(vita_contact_clock(elf, delay, duration)):
            g.run(0x16919C, probe)
            assert bool(g.run(0x169328, probe)) == active, (delay, duration, tick)
    finally:
        g.cpu.mem_write(clock, saved)


def verify_ranges(g, actor, part, hit, collision, shapes, program, elf):
    fz = Machine(elf, True)
    # These SceLibm imports in the Vita ELF are unresolved loader stubs.
    fz.trap(0x823D2450, lambda *_: fz.c.reg_write(UC_ARM_REG_S0, fz.c.reg_read(UC_ARM_REG_S0) & 0x7fffffff))
    def sqrtf(*_):
        value, = struct.unpack("<f", struct.pack("<I", fz.c.reg_read(UC_ARM_REG_S0)))
        fz.c.reg_write(UC_ARM_REG_S0, struct.unpack("<I", struct.pack("<f", math.sqrt(value)))[0])
    fz.trap(0x823D2490, sqrtf)
    player, body, weapon, joints, row, capsule, sphere = (
        MEM + i for i in (0x1000, 0x3000, 0x4000, 0x5000, 0x7000, 0x7100, 0x7200)
    )
    fz.c.mem_write(player + 3, bytes([13]))
    fz.put(player + 184, 0x3f800000)
    fz.put(player + 1656, body)
    fz.put(body + 36, body + 384)
    fz.put(player + 1664, weapon)
    fz.put(weapon + 68, joints)
    # wk451's depth-first indices 4, 5 and 7 are the head and its two blades.
    for index, (parent, child, sibling) in enumerate((
        (None, 1, None), (0, 2, None), (1, 3, None), (2, 4, None),
        (3, 5, 9), (4, 6, 7), (5, None, None), (4, 8, None),
        (7, None, None), (3, None, 10), (3, None, None),
    )):
        for offset, link in ((200, parent), (204, sibling), (208, child)):
            if link is not None:
                fz.put(joints + index * 384 + offset, joints + link * 384)

    offset = program.index(b"MSHT") + 4
    groups = []
    for family in (120, 121):
        kind, count = struct.unpack_from("<HH", program, offset)
        assert kind == family
        offset += 4
        for selector in range(count):
            num, = struct.unpack_from("<H", program, offset)
            offset += 2
            if (family == 120 and selector != 0) or (family == 121 and selector == 8):
                name = f"melee_{selector}" if family == 120 else "bind_finisher"
                groups.append((name, program[offset:offset + 40 * num]))
            offset += 40 * num

    gu_volume, gu_probe, gu_point = g.alloc(48), g.alloc(16), g.alloc(16)
    copy = g.alloc(184)
    g.cpu.mem_write(copy, bytes(g.cpu.mem_read(hit, 184)))
    original = [(p, bytes(g.cpu.mem_read(p, size))) for p, size in (
        (actor + 64, 12), (actor + 4072, 12), (part + 1172, 8), (collision + 10288, 4),
    )]
    gu_bones, gu_map = g.alloc(160 * 3), g.alloc(256)
    g.put(part + 1172, gu_bones)
    g.put(part + 1176, gu_map)
    indices = bytearray([255] * 256)
    indices[4], indices[5], indices[7] = 0, 1, 2
    g.cpu.mem_write(gu_map, bytes(indices))
    for yaw, sin, cos in ((0, 0., 1.), (0x4000, 1., 0.)):
        pos = (375., 80., -245.)
        g.cpu.mem_write(actor + 64, struct.pack("<3f", *pos))
        g.cpu.mem_write(actor + 4072, struct.pack("<3I", 0, yaw, 0))
        basis = (cos, 0., -sin, 0., 0., 1., 0., 0., sin, 0., cos, 0.)
        fz.c.mem_write(body + 384, struct.pack("<16f", *basis, *pos, 1.))
        for i, joint in enumerate((4, 5, 7)):
            matrix = struct.pack("<16f", *basis, 160. * i - 70., 35. * i, 650., 1.)
            fz.c.mem_write(joints + joint * 384, matrix)
            g.cpu.mem_write(gu_bones + 16 + i * 160, matrix)
        for name, rows in groups:
            g.put(copy + 132, shapes["player\\hit\\frontier_ms\\" + name])
            g.put(collision + 10288, 0)
            assert g.run(0x1696FC, copy) == len(rows) // 40
            for i in range(len(rows) // 40):
                fz.c.mem_write(row, rows[i * 40:(i + 1) * 40])
                kind = fz.call(0x8183AAAE, player, row, capsule, sphere)
                gu = struct.unpack("<B3x7f", g.cpu.mem_read(collision + 8240 + i * 32, 32))
                assert gu[0] == kind + 1
                if kind == 1:
                    *points, radius = struct.unpack("<7f", fz.c.mem_read(capsule, 28))
                else:
                    *points, radius = struct.unpack("<4f", fz.c.mem_read(sphere, 16))
                expected = (radius, *points)
                assert all(math.isclose(a, b, abs_tol=.01) for a, b in zip(gu[1:], expected)), (
                    name, yaw, gu, expected)
                # Check the native monster damage dispatcher's sphere target at either side
                # of the surface, including capsules whose two ends coincide.
                probe = sphere + 128
                if kind == 1:
                    fz.call(0x816E8630, capsule, probe + 96)
                    g.cpu.mem_write(gu_volume, struct.pack("<9f", *gu[2:5], 0., *gu[5:], 0., gu[1]))
                else:
                    g.cpu.mem_write(gu_volume, struct.pack("<4f", *gu[2:5], gu[1]))
                # Each test is centered on the endpoint with the highest Y.
                end = points[3:] if kind and points[4] > points[1] else points[:3]
                for distance in (radius + 4., radius + 6.):
                    target = struct.pack("<4f", end[0], end[1] + distance, end[2], 5.)
                    fz.c.mem_write(probe, target)
                    g.cpu.mem_write(gu_probe, target)
                    if kind == 1:
                        fz.c.reg_write(UC_ARM_REG_S0, 0x40a00000)
                        source = fz.call(0x816EBA2C, probe + 96, probe, probe + 32)
                        observed = g.run(0x167F64, gu_volume, gu_probe, gu_point)
                    else:
                        fz.c.reg_write(UC_ARM_REG_S0, struct.unpack("<I", struct.pack("<f", radius))[0])
                        fz.c.reg_write(UC_ARM_REG_S1, 0x40a00000)
                        source = fz.call(0x816E86F8, sphere, probe, probe + 32)
                        observed = g.run(0x167118, gu_volume, gu_probe, gu_point)
                    assert source == observed == int(distance < radius + 5.), (name, yaw, distance, source, observed)
    for addr, data in original:
        g.cpu.mem_write(addr, data)


def verify_marked_row(g, strike, monster, hurt, collision):
    # Two overlapping hurt rows can have the same breakable-part ID.
    from unicorn.arm_const import UC_ARM_REG_SP

    saved_rows = bytes(g.cpu.mem_read(hurt + 44, 46))
    saved_shapes = bytes(g.cpu.mem_read(collision + 28, 64))
    first = bytes(g.cpu.mem_read(hurt, 44))
    g.cpu.mem_write(hurt + 44, first + b"\xff\xff")
    sphere = struct.pack("<B3x7f", 1, 10., 100., 50., 150., 0., 0., 0.)
    g.cpu.mem_write(collision + 28, 2 * sphere)
    seen = []
    def marked(result, *_):
        # ARM32 returns the registered enum through the caller's output pointer.
        g.cpu.mem_write(result, struct.pack("<II", 2, 1))
    def settlement(*_):
        seen.append(g.get(g.cpu.reg_read(UC_ARM_REG_SP) + 8))
    g.stub(g.restrained_query, marked)
    g.stub(0x170968, settlement)
    try:
        g.run(0x16C0C0, collision, strike, monster)
        assert seen == [hurt + 44], seen
        assert bytes(g.cpu.mem_read(collision + 28, 64)) == 2 * sphere
        g.cpu.mem_write(hurt + 44 + 10, struct.pack("<H", 2))
        g.run(0x16C0C0, collision, strike, monster)
        assert seen == [hurt + 44]
    finally:
        for address in (0x170968, g.restrained_query):
            g.cpu.hook_del(g.traps.pop(address))
        g.cpu.mem_write(hurt + 44, saved_rows)
        g.cpu.mem_write(collision + 28, saved_shapes)
