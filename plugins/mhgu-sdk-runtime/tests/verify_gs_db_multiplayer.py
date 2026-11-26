"""Run W00/W11 netPlAction and the receiving game's weapon and damage dispatchers."""
import itertools
import math
from unicorn import arm_const as arm
import struct
import sys
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "plugins/great-sword-extreme/tests"))
from damage_target import damage_target
from verify_style import STYLE_IDS, boot, guard_body_scene, load_registered_style, quest_entries


def network_scene(weapon, name, vtable):
    g = boot((ROOT / "mhgu/main.elf").read_bytes(), f"[Weapons]\n{weapon}={STYLE_IDS[name]}\n".encode(), ("registry",))
    nx = ROOT / "plugins" / name / "dist" / name / "romfs/nativeNX"
    if weapon == 1:
        g.inline_hooks.add(0x116D464)
    desc = load_registered_style(g, nx / "plugins" / (name.replace("-", "_") + ".nro"))
    def half(addr, value):
        g.cpu.mem_write(addr, struct.pack("<H", value))
    def number(addr, value):
        g.cpu.mem_write(addr, struct.pack("<f", value))
    def zero(*_):
        return 0
    manager, session, packet = [g.alloc(n) for n in (128, 128, 200)]
    g.put(0x1885B58, manager)
    g.put(0x188CA48, session)
    for pc in (0x2AC690, 0x2E9B7C, 0x27C8F8, 0xE6890, 0x2827B4, 0x227B3C, 0x280548,
               0x2FFC1C, 0x2FF984, 0xE174C, 0x282FD8, 0x2A2560, 0x887648, 0x887660,
               0x2B76CC, 0x2864C4, 0x2A264C):
        g.stub(pc, zero)
    for vt in (164, 884, 1132, 348, 376, 508, 2020, 2024, 612, 1828, 1356, 1992):
        g.stub(g.get(vtable + vt), zero)
    for addr in (0x2A2444, g.get(vtable + 136)):
        g.stub(addr, lambda *_: 1)
    g.stub(0x13EFDC4, lambda p, n, *_: g.cpu.mem_write(p, bytes(n)))
    g.stub(0x53A06C, lambda p, _, req, __: half(p + 1204, req & 65535) or 1)
    seq = itertools.count(1)
    g.stub(0x16B920, lambda *_: next(seq))
    for pc in (0x7AF3A0, 0x7AF37C):
        g.stub(pc, lambda size, *_: g.alloc(size))

    # Imported trigonometry for GU's oriented blade shapes.
    for pc, fn in ((0x13EFDE8, math.sin), (0x13EFDF4, math.cos)):
        def trig(*_, fn=fn):
            angle, = struct.unpack("<f", struct.pack("<I", g.cpu.reg_read(arm.UC_ARM_REG_S0)))
            bits, = struct.unpack("<I", struct.pack("<f", fn(angle)))
            g.cpu.reg_write(arm.UC_ARM_REG_S0, bits)
        g.stub(pc, trig)

    def resource(data):
        obj, raw, vt = [g.alloc(n) for n in (112, len(data), 80)]
        g.cpu.mem_write(raw, data)
        g.put(obj, vt)
        g.put(obj + 80, 1)
        g.put(obj + 100, raw)
        return obj, raw, vt

    files = {kind: zlib.decompress(blob)
             for _, kind, blob in quest_entries(f"w{weapon:02}.arc")
             if kind in (0x4E2EF008, 0x5653C1B0)}
    hdp, raw, vt = resource(files[0x4E2EF008])
    g.put(vt + 64, 0x270D8C)
    g.put(vt + 68, 0x270D98)
    g.put(hdp + 104, raw + 16)
    def shape(data):
        obj, _, vt = resource(data)
        assert g.run(0x16A1D8, obj) == 1
        getter = g.alloc(4)
        g.stub(getter, lambda obj, i, *_: g.get(g.get(obj + 104) + 4*i))
        g.put(vt + 68, getter)
        return obj
    stock = shape(files[0x5653C1B0])
    rm, rvt, loader = [g.alloc(n) for n in (16, 64, 4)]
    g.put(0x2126A64, rm)
    g.put(rm, rvt)
    g.put(rvt + 48, loader)
    shapes = {path: shape(path.read_bytes()) for path in (nx / "player/hit").rglob("*.hts")}
    def load(_, kind, name, flags):
        path = nx / g.string(name).replace("\\", "/")
        if kind == 0x2125210:
            raw = path.with_suffix(".lmt").read_bytes()
            count = struct.unpack_from("<H", raw, 6)[0]
            return resource(raw[:8 + 4*count])[0]
        if kind == 0x1857810:
            return shapes[path.with_suffix(".hts")]
        return 0
    g.stub(loader, load)
    def hunter(slot):
        actor, eq = g.alloc(14000), g.alloc(1600)
        g.put(actor, vtable)
        g.put(actor + 5060, eq)
        half(eq + 800, 100)
        g.cpu.mem_write(eq + 1236, bytes([weapon]))
        g.cpu.mem_write(actor + 4177, bytes([slot]))
        g.put(actor + 240, g.alloc(128))
        number(actor + 28, 2)
        number(actor + 1284, 240)
        half(actor + 5078, 100)
        g.put(actor + 12696, hdp)
        g.put(actor + 12700, stock)
        for hand in (12320, 12504):
            g.run(0x1687A8, actor + hand)
        return actor
    local, guest, second, sender = [hunter(i) for i in (0, 1, 2, 1)]
    g.cpu.mem_write(g.get(local + 5060) + 1236, b"\x08")
    for index, actor in enumerate((local, guest, second)):
        g.put(manager + 28 + index*4, actor)
    monster, status, _, _, collision, credit = damage_target(g, vtable)

    return g, nx, desc, manager, session, packet, local, guest, second, sender, (monster, status, collision, credit)


def replay(weapon, name, vtable):
    g, nx, desc, manager, _, packet, local, guest, second, sender, target = network_scene(
        weapon, name, vtable
    )
    monster, status, collision, credit = target

    def half(addr, value):
        g.cpu.mem_write(addr, struct.pack("<H", value))

    def number(addr, value):
        g.cpu.mem_write(addr, struct.pack("<f", value))

    def zero(*_):
        return 0

    def receive(actor, move, bits=None, flags=0):
        half(sender + 9478, 16)
        half(sender + 9482, 0x8000 + move)
        g.run(0x1FED90, packet, sender, flags)
        assert g.half(packet + 56) == 0x8000 + move
        if bits is not None:
            g.put(packet + 16, g.get(packet + 16) | ((8 | bits) << 16))
        g.run(0x1FF308, packet, actor)
        assert g.half(actor + 9482) == 0x8000 + move
        number(actor + 1280, 0)
        g.run(0x282DA8, actor)
        assert g.cpu.mem_read(actor + 5, 1) == b"\1"
    def hit_frame(actor, frame, hand):
        number(actor + 1280, frame)
        g.run(0x282DA8, actor)
        hit = actor + hand
        assert g.get(hit + 12) == actor and g.half(hit + 56) > 0, (name, frame, hand)
        serial = g.half(hit + 52)
        g.run(0x282DA8, actor)
        assert g.half(hit + 52) == serial
        return hit

    for actor in (guest, second):
        receive(actor, 0, 0)
        assert g.half(actor + 1204) == (1406 if weapon == 0 else 1401)
    for index, actor in enumerate((guest, second), 1):
        hit = hit_frame(actor, 0, 12320)
        mapping, matrices = g.alloc(256), g.alloc(512)
        g.put(actor + 1176, mapping)
        g.put(actor + 1172, matrices)
        g.cpu.mem_write(mapping, bytes([1])*256)
        g.cpu.mem_write(matrices + 176, struct.pack("<16f",1,0,0,0,0,1,0,0,0,0,1,0,10,90,100*index,1))
        g.put(collision + 10288, 0)
        assert g.run(0x1696FC, hit) > 0
        xyz = g.cpu.mem_read(collision + 8248, 12)
        g.cpu.mem_write(collision + 28, struct.pack("<B3xf", 1, 2) + bytes(xyz))
        before = g.get(status + 880)
        g.run(0x16C0C0, collision, hit, monster)
        damage = g.half(status + 1160)
        assert damage > 0 and credit[-1] == (0, index, damage), credit
        g.run(0x97DDC, monster)
        assert g.get(status + 880) == before - damage
        g.run(0x9D67C, monster)
    if weapon == 0:
        for flags in (0x80, 0):
            receive(guest, 58, 4, flags)
            assert g.run(0x2A263C, guest) == int(flags != 0)
        for actor, level in ((guest, 3), (second, 0)):
            receive(actor, 29, level)
            row = (3, 91, 92, 93)[level]
            data = (ROOT / "plugins" / name / "target/assets/great_sword.fgsa").read_bytes()
            offset, = struct.unpack_from("<I", data, 12)
            expected, = struct.unpack_from("<H", data, offset + 40*row + 4)
            assert g.half(hit_frame(actor, 32, 12320) + 56) == expected
            if level == 0:
                expected_base = expected
    else:
        for actor, mode in ((guest, 7), (second, 1)):
            receive(actor, 0, mode)
            for hand in (12320, 12504):
                hit = hit_frame(actor, 8, hand)
                base = g.half(hit + 56)
                # The fixture omits the HDP adjustment trampoline in 2B76CC.
                # Execute its built w11 detour for each emitted contact.
                row = g.alloc(56)
                g.run(g.hooks[0x11BAD38], actor, hit, row, 1)
                assert g.half(hit + 56) == int(base * (1.45 if mode == 7 else 1.35))
    if weapon == 0:
        # Two concurrent Radiance emitters must expand at their own world
        # origins even after both peers have submitted their first wave.
        for i, actor in enumerate((guest, second), 1):
            receive(actor, 59)
            number(actor + 64, 300 * i)
            number(actor + 1280, 82)
            g.run(0x282DA8, actor)
            assert g.half(actor + 1204) == 1465
            number(actor + 1280, 84)
            g.run(0x282DA8, actor)
        for i, actor in enumerate((guest, second), 1):
            g.put(collision + 10288, 0)
            assert g.run(0x1696FC, actor + 12320) == 1
            x, = struct.unpack("<f", g.cpu.mem_read(collision + 8248, 4))
            assert x == 300*i, x
    else:
        # The five strip render callbacks use distinct model paths per hunter.
        fx = nx / "effect/frontier_db"
        for slot, prefix in enumerate("abcd", 1):
            pel = (fx / f"peer{slot}.pel").read_bytes()
            for channel, effect in enumerate((14, 18, 21, 189, 190), 1):
                layer = f"{prefix}{effect}"
                model = f"m5{slot}{channel}"
                assert (layer.encode() + b"\0") in pel
                assert (model.encode() + b"\0") in (fx / (layer + ".efl")).read_bytes()
                assert all((fx / (model + ext)).is_file() for ext in (".mod", ".mrl"))

    # Execute the sending NRO as a local player, then deliver its packet to
    # a receiver using w08. Neither the charge tier nor demon mode is inferred.
    g.put(manager + 28, sender)
    move = 57 if weapon == 0 else 65
    half(sender + 9478, 16)
    half(sender + 9482, 0x8000 + move)
    g.cpu.mem_write(sender + 5, b"\0")
    g.cpu.mem_write(sender + 10057, b"\1")
    g.put(sender + 1188, 1)
    g.run(g.get(desc + 20), sender, g.alloc(40))
    outgoing = 29 if weapon == 0 else 0
    half(sender + 9482, 0x8000 + outgoing)
    g.run(g.hooks[0x1FED90], packet, sender, 0)
    expected = 3 if weapon == 0 else 1
    assert g.get(packet + 16) >> 16 & 15 == 8 + expected
    g.put(manager + 28, local)
    g.run(g.hooks[0x1FF308], packet, guest)
    g.run(0x282DA8, guest)
    if weapon == 11:
        assert g.cpu.mem_read(guest + 13168, 1)[0] & 3 == 1
    # Reusing a departed slot with the same actor address must start clean.
    g.put(manager + 32, 0)
    g.stub(g.originals[0x282DA8], zero)
    g.run(g.hooks[0x282DA8], local)
    g.cpu.hook_del(g.traps.pop(g.originals[0x282DA8]))
    g.put(manager + 32, guest)
    receive(guest, outgoing)
    if weapon == 0:
        assert g.half(hit_frame(guest, 32, 12320) + 56) == expected_base
        verify_guard_body_network(g, manager, packet, local, guest, second, sender)
    assert g.get(local + 12320 + 12) != guest
    print(name, "two peers in a mixed-weapon room, native packet, motion, hit ownership and monster HP passed")


def verify_guard_body_network(g, manager, packet, local, guest, second, sender):
    monster, _, _, step = guard_body_scene(g)
    # Let GU apply the player's packet position, and isolate the original
    # local action dispatcher while the registry still observes both peers.
    g.cpu.hook_del(g.traps.pop(0x280548))
    g.cpu.hook_del(g.traps.pop(0x710B0))
    def observe_local(count=1):
        dispatch = g.originals[0x282DA8]
        g.stub(dispatch, lambda *_: 0)
        for _ in range(count):
            g.run(g.hooks[0x282DA8], local)
        g.cpu.hook_del(g.traps.pop(dispatch))

    def receive_guard(actor, z=0., marked=True, move=66):
        if g.half(actor + 9482) == 0x8000 + move:
            # A second success follows the new native guard, even if the
            # private success slot is the same as it was on the previous hit.
            g.put(manager + 28, sender)
            g.cpu.mem_write(sender + 9478, struct.pack("<H2xH", 32, 0))
            g.run(g.hooks[0x1FED90], packet, sender, 0)
            g.put(manager + 28, local)
            g.run(g.hooks[0x1FF308], packet, actor)
            assert g.half(actor + 9478) == 32
        g.put(manager + 28, sender)
        g.cpu.mem_write(sender + 9478, struct.pack("<H2xH", 16, 0x8000 + move))
        g.cpu.mem_write(sender + 64, struct.pack("<3f", 0, 0, z))
        g.run(g.hooks[0x1FED90], packet, sender, 0)
        assert g.get(packet + 16) >> 16 & 15 == 13
        if not marked:
            g.put(packet + 16, g.get(packet + 16) & ~(15 << 16))
        g.put(manager + 28, local)
        g.run(g.hooks[0x1FF308], packet, actor)
        assert g.half(actor + 9482) == 0x8000 + move
        assert g.cpu.mem_read(actor + 5, 1) == b"\0"
        g.put(actor + 1280, 0)
        g.run(0x282DA8, actor)
        assert g.half(actor + 1204) == 1474

    receive_guard(guest, marked=False)
    assert step(0, -75) == (0, -75)
    receive_guard(guest)
    stopped = step(0, -75)
    assert -15.1 < stopped[1] < -14.5, (stopped, g.get(guest + 11072), g.get(guest + 64), g.get(guest + 4076))
    away = (0, 50, 400)
    assert step(15, -40, away) == (12, 0)
    receive_guard(guest)
    assert step(0, -40, away) == (0, -40)  # a new precise guard needs fresh contact
    x, z = step(0, -75, (60, 50, 150))
    assert x == 0 and -19 < z < -18, (x, z)
    assert step(0, 30, away) == (0, 30)
    # The authoritative monster stops at the first of the two guard bodies,
    # even when the closer guard was added later and occupies a higher slot.
    receive_guard(second, z=10., move=72)
    assert -5.1 < step(0, -75)[1] < -4.5
    g.cpu.mem_write(monster + 46583, b"\1")
    assert step(0, -75) == (0, -75)
    g.cpu.mem_write(monster + 46583, b"\0")

    push, normal = g.alloc(16), g.alloc(16)
    g.cpu.mem_write(push, struct.pack("<4f", 0, 0, -10, 0))
    g.cpu.mem_write(normal, struct.pack("<4f", 0, 0, 10, 0))
    g.stub(0xAA278, lambda *_: 5)
    g.run(g.hooks[0x17F188], 0, guest, push, monster, normal)
    assert g.get(guest + 4168) == 0
    side_push, side_normal = g.alloc(16), g.alloc(16)
    g.cpu.mem_write(side_push, struct.pack("<4f", 10, 0, 0, 0))
    g.cpu.mem_write(side_normal, struct.pack("<4f", -10, 0, 0, 0))
    assert step(0, -10, away) == (0, 0)
    g.run(g.hooks[0x17F188], 0, second, side_push, monster, side_normal)
    assert g.get(second + 4160) == 0

    # A replica can establish protection from the hunter's own native push,
    # even when GU has no combined separation vector for the monster.
    g.cpu.mem_write(monster + 46583, b"\1")
    zero_normal = g.alloc(16)
    g.run(g.hooks[0x17F188], 0, guest, push, monster, zero_normal)
    assert g.get(guest + 4168) == 0
    g.run(g.hooks[0x17F188], 0, guest, side_push, monster, side_normal)
    assert g.get(guest + 4160) == 0
    g.cpu.mem_write(monster + 46583, b"\0")
    receive_guard(guest)

    # Native 30/60 Hz update sizes both expire at 14 clock units, independent
    # of the remote motion frame (kept at zero here).
    for elapsed in (1, 2):
        if elapsed == 2:
            receive_guard(guest)
            assert step(0, -75)[1] > -75
        g.cpu.mem_write(local + 28, struct.pack("<f", elapsed))
        observe_local(14 // elapsed - 1)
        assert step(0, -75, away) == (0, 0)
        observe_local()
        assert step(0, -75, away) == (0, -75)
    g.run(g.hooks[0x17F188], 0, guest, push, monster, normal)
    assert struct.unpack("<f", g.cpu.mem_read(guest + 4168, 4)) == (-10.,)

    receive_guard(guest)
    g.cpu.mem_write(guest + 9482, struct.pack("<H", 0x8044))
    assert step(0, -75) == (0, -75)
    receive_guard(guest)
    g.cpu.mem_write(guest + 4180, b"\1")
    assert step(0, -75) == (0, -75)
    g.cpu.mem_write(guest + 4180, b"\0")
    g.put(manager + 32, 0)
    observe_local()
    g.put(manager + 32, guest)
    assert step(0, -75) == (0, -75)
    print("GU great sword guard body: packet marker, simultaneous guards, contact hold, timeout, counter, area and roster passed")


if __name__ == "__main__":
    replay(0, "great-sword-extreme", 0x1749A88)
    replay(11, "dual-blades-extreme", 0x174F2B4)
