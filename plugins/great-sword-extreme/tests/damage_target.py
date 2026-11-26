"""A stationary GU target for replaying hunter contacts, HP and player credit."""

import struct


def damage_target(g, weapon_vtable, *, native_affinity=False):
    def half(addr, value):
        g.cpu.mem_write(addr, struct.pack("<H", value))

    def byte(addr, value):
        g.cpu.mem_write(addr, bytes([value]))

    def number(addr, value):
        g.cpu.mem_write(addr, struct.pack("<f", value))

    def zero(*_):
        return 0

    # A stationary monster with one breakable part and neutral equipment.
    # Keep GU's attack power, final damage arithmetic, part accumulation and
    # HP commit. Supply the scene's hitzone and skip audiovisual reactions.
    monster, status, monster_vt, part, zone, tune, breaks, empty = [
        g.alloc(n) for n in (48000, 26000, 1400, 96, 16, 512, 80, 16)
    ]
    g.put(monster, monster_vt)
    g.put(monster + 5160, status)
    g.put(monster + 30188, tune)
    g.put(tune + 104, breaks)
    half(breaks, 1000)
    half(status + 958, 1000)
    byte(monster + 29728, 1)
    g.put(status + 880, 1000)
    g.put(status + 884, 1000)
    number(status + 904, 1)
    noop = g.alloc(4)
    g.stub(noop, zero)
    for offset in range(0, 1400, 4):
        g.put(monster_vt + offset, noop)
    g.stub(0xBAB60, lambda *_: zone)
    g.stub(0xBAAAC, lambda *_: empty)
    g.stub(0x710F0, lambda *_: 0xFFFFFFFF)
    for pc in (
        0x812A8,
        0x1722C0,
        0x17240C,
        0x172474,
        0x172A90,
        0x17251C,
        0x1725D8,
        0x16DB68,
        0x172AC4,
        0x1798CC,
        0x18015C,
        0x16DA88,
        0x67254,
        0x81C6C,
        0xA1FC8,
        0xAAB9C,
        0x9A828,
        0xBCC04,
        0x173944,
        0x8172C,
    ):
        g.stub(pc, zero)
    if not native_affinity:
        g.stub(g.originals.get(0x17885C, 0x17885C), zero)
    # 178030's two output factors are initialized to 1.0 by the damage dispatcher.
    g.stub(0x178030, zero)
    credit = []
    g.stub(
        0x9DD40, lambda _, kind, player, amount: credit.append((kind, player, amount))
    )

    collision = g.alloc(10400)
    g.put(0x1857830, collision)
    clock = g.alloc(112)
    g.put(0x2126764, clock)
    number(clock + 104, 2)
    half(part + 44, 0xFFFF)
    g.stub(0x710B0, lambda *_: part)
    g.stub(g.get(weapon_vtable + 148), zero)

    return monster, status, monster_vt, part, collision, credit
