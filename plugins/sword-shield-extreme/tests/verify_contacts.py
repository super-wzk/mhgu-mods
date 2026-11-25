"""Construct w01 sword, shield and world contacts with the packaged HTS and retail HDP."""

import itertools
import struct
import zlib

from verify_port import ROOT, Scene, float_bits
from verify_style import CHANGE_ACTION, ENCODE, SNS_DRAW_ROUTE, quest_entries
from damage_target import damage_target


def replay():
    vtable = 0x174A354
    s = Scene(1, 5, "sword-shield", vtable)
    g, a = s.g, s.actor
    hdp = next(
        zlib.decompress(data)
        for name, kind, data in quest_entries("w01.arc")
        if kind == 0x4E2EF008 and name.endswith(b"pl_we01_hitdata")
    )
    data, bank, vt = g.alloc(len(hdp)), g.alloc(112), g.alloc(72)
    g.cpu.mem_write(data, hdp)
    g.put(bank, vt)
    g.put(vt + 64, 0x270D8C)
    g.put(vt + 68, 0x270D98)
    g.put(bank + 100, data)
    g.put(bank + 104, data + 16)
    g.put(a + 12696, bank)
    g.put(a + 5078, 100)
    s.stub(0x13EFDC4, lambda dest, size, *_: g.cpu.mem_write(dest, bytes(size)))
    for offset in (12320, 12504):
        g.run(0x1687A8, a + offset)
    serial = itertools.count(1)
    s.stub(0x16B920, lambda *_: next(serial))
    s.stub(0x887648, lambda *_: 0)
    s.stub(0x887660, lambda *_: 0)
    local = g.alloc(128)
    g.put(0x188CA48, local)
    resources, manager_vt, load, shape_vt, getter = [
        g.alloc(n) for n in (16, 64, 4, 72, 4)
    ]
    g.put(0x2126A64, resources)
    g.put(resources, manager_vt)
    g.put(manager_vt + 48, load)
    g.put(shape_vt + 68, getter)
    for address in (0x7AF3A0, 0x7AF37C):
        s.stub(address, lambda size, *_: g.alloc(size))
    groups, shapes = {}, {}
    nx = ROOT / "dist/sword-shield-extreme/romfs/nativeNX"
    for path in (nx / "player/hit/frontier_sns").glob("*.hts"):
        content = path.read_bytes()
        obj, raw = g.alloc(112), g.alloc(len(content))
        g.cpu.mem_write(raw, content)
        g.put(obj, shape_vt)
        g.put(obj + 80, 1)
        g.put(obj + 100, raw)
        assert g.run(0x16A1D8, obj) == 1
        groups[obj] = g.get(g.get(obj + 104))
        shapes[path.stem] = obj
    g.stub(getter, lambda obj, i, *_: groups[obj] if i == 0 else 0)
    g.stub(
        load,
        lambda mgr, kind, path, flags: (
            shapes[g.string(path).split("\\")[-1]] if kind == 0x1857810 else 0
        ),
    )
    g.put(a + 12700, shapes["blade_6"])
    monster, status, _, _, collision, credit = damage_target(g, vtable)
    # Neutral equipment with base attack 100. GU resolves the collision and HP;
    # the shared target supplies a neutral hitzone and suppresses visual reactions.
    g.cpu.mem_write(g.get(a + 5060) + 800, struct.pack("<H", 100))
    joints, mapping = g.alloc(512), g.alloc(256)
    g.put(a + 1172, joints)
    g.put(a + 1176, mapping)
    g.cpu.mem_write(mapping + 8, bytes([1]))
    g.cpu.mem_write(mapping + 12, bytes([2]))
    pose = struct.pack("<16f", 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)
    for joint in (1, 2):
        g.cpu.mem_write(joints + 176 * joint, pose)

    def tick(frame):
        g.put(a + 1280, float_bits(frame))
        s.tick()

    def resolve(hit, power, center):
        g.put(collision + 10288, 0)
        assert g.run(g.hooks[0x1696FC], hit) == 1
        before = g.get(status + 880)
        g.cpu.mem_write(collision + 28, struct.pack("<B3x4f", 1, 2, *center))
        g.run(0x16C0C0, collision, hit, monster)
        assert g.half(status + 1160) == power, (power, credit)
        assert credit[-1] == (0, 0, power)
        g.run(0x97DDC, monster)
        assert g.get(status + 880) == before - power
        g.run(0x9D67C, monster)

    sword = a + 12320
    s.action(16, 0x8037)
    g.put(a + 13096, 4)
    g.put(a + 13104, float_bits(80))
    g.run(0x1168EEC, a)
    tick(0)
    assert (g.half(sword + 56), g.get(sword + 132)) == (31, shapes["blade_6"])
    assert struct.unpack("<2f", g.cpu.mem_read(sword + 92, 8)) == (24, 8)
    tick(24)
    resolve(sword, 31, (0, 0, 50))
    waves = []
    s.stub(0x1696E4, lambda hit, *_: waves.append(hit))
    s.stub(g.originals[0x16ABAC], lambda *_: 0)
    tick(30)
    g.run(g.hooks[0x16ABAC], collision)
    (wave,) = waves
    assert g.half(wave + 52) != g.half(sword + 52)
    resolve(wave, 8, (0, 100, 0))
    assert g.get(status + 880) == 961

    # Let GU construct both hits from the same equipped stats and live oils.
    eq = g.get(a + 5060)
    s.stub(g.get(vtable + 508), lambda *_: 3)
    g.cpu.mem_write(eq + 802, struct.pack("<h", 35))  # effective affinity
    g.cpu.mem_write(eq + 804, struct.pack("<h", 21))  # element value
    g.cpu.mem_write(eq + 834, struct.pack("<h", 1))   # fire
    g.put(a + 13100, 7)
    g.put(a + 13108, float_bits(80))
    for oil, ko, exhaust, part_factor, wave_power in [
        (0, 7, 8, 1.15, 8), (3, 12, 14, 1.15, 8),
        (4, 7, 8, 1.15, 16), (2, 7, 8, 1.30, 8),
    ]:
        g.run(s.controller, 0, 0)
        s.action(16, 0x8037)
        g.put(a + 13096, oil)
        g.put(a + 13104, float_bits(80))
        g.put(a + 10336, 0)
        g.run(0x1168EEC, a)
        tick(0)
        tick(30)
        waves.clear()
        g.run(g.hooks[0x16ABAC], collision)
        (wave,) = waves
        for hit, divisor, power in ((sword, 1, 31), (wave, 2, wave_power)):
            assert g.half(hit + 56) == power
            assert g.half(hit + 144) == 35 and g.get(hit + 48) & 0x20000
            assert g.get(hit + 72) == 16 and g.half(hit + 80) == 21 // divisor
            assert (g.cpu.mem_read(hit + 86, 1)[0], g.cpu.mem_read(hit + 89, 1)[0]) == (ko // divisor, exhaust // divisor)
            assert g.get(hit + 152) == float_bits(part_factor)
    print("Blade and wave: independent native affinity, fire, KO, exhaust and part modifiers passed")
    # Control only GU's status roll. Either hit can proc while its partner misses.
    for kind, mask, procs in ((6, 2, (1, 0)), (6, 2, (0, 1)), (7, 4, (1, 1)),
                              (8, 1, (1, 1)), (9, 0x20000, (1, 1))):
        g.cpu.mem_write(eq + 834, struct.pack("<h", kind))
        rolls = iter(procs)
        s.stub(0x302208, lambda *_: next(rolls))
        g.run(s.controller, 0, 0)
        s.action(16, 0x8037)
        tick(0)
        tick(30)
        waves.clear()
        g.run(g.hooks[0x16ABAC], collision)
        (wave,) = waves
        for hit, proc, amount in zip((sword, wave), procs, (21, 10)):
            assert g.get(hit + 72) == mask * proc and g.half(hit + 80) == amount * proc
    s.stub(0x302208, lambda *_: 1)

    # Cancel into Round Force with the forward wave still in flight. The
    # previous replay checks the native PMA. Here isolate its first cue and
    # keep GU's real contact constructors, HTS resources and list expansion.
    g.cpu.mem_write(eq + 834, struct.pack("<h", 1))  # fire
    s.stub(g.originals[0x116D464], lambda owner, *_: g.put(owner + 12716, 1))
    s.action(16, 43)
    g.cpu.mem_write(a + 5, bytes([1]))
    g.put(a + 12716, 0)
    g.run(0x1169A7C, a)
    waves.clear()
    g.run(g.hooks[0x16ABAC], collision)
    forward, ring = waves
    assert forward == wave and g.half(ring + 52) != g.half(forward + 52)
    assert g.get(ring + 132) == shapes["round_force"]
    assert g.get(groups[shapes["round_force"]] + 4) == 8
    assert g.get(ring + 152) == float_bits(1.3) and g.half(ring + 56) == 8
    assert g.get(forward + 72) == 0x20000 and g.half(forward + 80) == 10
    assert g.get(ring + 72) == 16 and g.half(ring + 80) == 10
    assert (g.cpu.mem_read(ring + 86, 1)[0], g.cpu.mem_read(ring + 89, 1)[0]) == (3, 4)
    g.put(collision + 10288, 0)
    assert g.run(g.hooks[0x1696FC], forward) == 1
    assert g.run(g.hooks[0x1696FC], ring) == 8
    print("Independent status rolls and overlapping forward/ring contacts passed")

    s.action(16, 0x802F)
    tick(0)
    tick(26)
    assert (g.half(sword + 56), g.get(sword + 132)) == (20, shapes["blade_29"])
    assert g.cpu.mem_read(sword + 86, 1) == bytes([13])
    assert g.get(sword + 48) & 0x08000000
    # The airborne attack reuses the two FZ sprint windows. A low ledge lands
    # between swings, while a tall drop may outlast the entire clip.
    returned = []
    s.stub(0x2B6E5C, lambda owner, *_: returned.append(owner))
    s.stub(0x53A058, lambda *_: 5001)
    g.put(a + 28, float_bits(1))

    def accept(owner, group, slot, flags):
        assert owner == a
        s.action(group, slot)
        return 1

    s.stub(g.originals[CHANGE_ACTION], accept)
    s.stub(g.get(vtable + 360), lambda *_: 0)
    lmt = (nx / "player/mot/frontier_sns/4.lmt").read_bytes()
    (offset,) = struct.unpack_from("<I", lmt, 8 + 4 * 47)
    length, loop = struct.unpack_from("<Ii", lmt, offset + 8)
    assert loop == -1 and length > 60
    end = length - 1
    g.put(a + 1284, float_bits(end))
    aerial_draw = g.run(g.hooks[SNS_DRAW_ROUTE], a, 5)
    draw_request = struct.unpack("<3H", g.cpu.mem_read(aerial_draw, 6))
    assert draw_request == (16, 16, 0x80)
    flight_start = (8.0, -4.0, -6.0, 0.0, 2.0, -0.5, -2.0)
    flight_limited = (4.0, -4.0, -3.0, 0.0, 1.0, -0.5, -1.0)
    for route, low_ledge in (
        (draw_request, True),
        ((16, 22, 0), False),
        ((16, 50, 0), False),
    ):
        s.action(4, 14)
        g.run(g.hooks[CHANGE_ACTION], a, *route)
        assert g.half(a + 9482) == 0x8038
        returned.clear()
        grounded = False
        s.stub(0x2A84D8, lambda *_: int(grounded))
        g.cpu.mem_write(a + 4112, struct.pack("<7f", *flight_start))
        tick(0)
        assert g.half(a + 1204) == 1447
        assert struct.unpack("<7f", g.cpu.mem_read(a + 4112, 28)) == flight_limited
        last = g.half(sword + 52)
        for frame, power, duration, hit_type, mount in (
            (16, 18, 16, 1, 0),
            (52, 34, 8, 36, 3),
        ):
            tick(frame - 1)
            assert g.half(sword + 52) == last
            tick(frame)
            assert g.half(sword + 52) != last
            last = g.half(sword + 52)
            assert (g.half(sword + 56), g.get(sword + 132)) == (
                power,
                shapes["blade_6"],
            )
            assert struct.unpack("<2f", g.cpu.mem_read(sword + 92, 8)) == (0, duration)
            assert (
                g.cpu.mem_read(sword + 58, 1)[0],
                g.cpu.mem_read(sword + 159, 1)[0],
            ) == (hit_type, mount)
            tick(frame)
            assert g.half(sword + 52) == last
            grounded = low_ledge
        tick(end)
        assert bool(returned) == low_ledge
        if not low_ledge:
            grounded = True
            tick(end)
        assert returned == [a]

    # Ground sprint X still selects the original w04 action and its melee contact.
    s.action(16, 0x8035)
    tick(0)
    g.put(s.inputs + 16, 1)
    tick(30)
    assert g.half(a + 9482) == 0x8024 and g.half(a + 1204) == 1447
    g.put(s.inputs + 16, 0)
    for frame, power in ((16, 18), (52, 34)):
        tick(frame)
        assert g.half(sword + 56) == power
        assert (
            g.cpu.mem_read(sword + 58, 1)[0],
            g.cpu.mem_read(sword + 159, 1)[0],
        ) == (1, 0)
    print(
        "Aerial draw and drawn jump slashes: clip 1447, mounting on the second hit, low/high landings and ground sprint control passed"
    )

    # Bind the motion bank and supply controlled deltas to GU's hunter motion
    # update through the regular layer update and the blend correction.
    motion_bank = g.alloc(112)
    g.put(motion_bank + 80, 1)
    s.stub(0xB49E54, lambda bank, slot, *_: int(bank == motion_bank and slot == 47))
    s.stub(load, lambda mgr, kind, *_: motion_bank if kind == 0x2125210 else 0)
    packed = g.run(g.hooks[ENCODE], a, 0x10000 | 1447)
    assert packed == 0xF2F
    s.stub(g.get(vtable + 204), lambda *_: 0)
    s.stub(0x280984, lambda *_: 0)
    root_updates = []

    def root_delta(owner, *_):
        root_updates.append(owner)
        g.cpu.mem_write(owner + 64, struct.pack("<3f", 112.0, -75.0, 82.0))

    s.stub(
        0x951168, lambda owner, *_: root_delta(owner) if g.get(owner + 1268) == 0 else 0
    )
    s.stub(0x53A5BC, root_delta)
    motion_update = g.hooks[0x281EF8]
    g.cpu.mem_write(a + 1204, struct.pack("<H", packed))
    g.put(a + 4344, 0x8000)
    for slot, phase, blend, expected in (
        (0x8038, 1, 0.0, (106.0, -75.0, 86.0)),
        (0x8038, 1, 3.0, (106.0, -75.0, 86.0)),
        (0x8038, 2, 0.0, (106.0, -75.0, 86.0)),
        (0x8038, 0, 0.0, (112.0, -75.0, 82.0)),
        (0x8024, 1, 0.0, (112.0, -75.0, 82.0)),
    ):
        s.action(16, slot)
        g.cpu.mem_write(a + 5, bytes([phase]))
        g.put(a + 1268, float_bits(blend))
        g.cpu.mem_write(a + 64, struct.pack("<3f", 100.0, -80.0, 90.0))
        g.run(motion_update, a)
        actual = struct.unpack("<3f", g.cpu.mem_read(a + 64, 12))
        assert actual == expected, (slot, phase, blend, actual, expected)
    assert root_updates == [a] * 5
    print(
        "Aerial travel: half horizontal root and inherited momentum, full vertical motion and ground sprint travel passed"
    )

    assert bytes(g.cpu.mem_read(data, len(hdp))) == hdp
    print(
        "w01 private HTS and native HDP: blade 31 + wave 8 HP, distinct serials, shield KO 13 and sharpness exemption passed"
    )


if __name__ == "__main__":
    replay()
