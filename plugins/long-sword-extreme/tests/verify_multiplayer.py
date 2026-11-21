"""GU netPlAction -> peer hunter -> Frontier blade contacts in the receiving game."""

import itertools
import struct
import zlib

from verify_port import NX, REPO, boot, load_registered_style
from verify_style import quest_arc


def replay():
    g = boot((REPO / "mhgu/main.elf").read_bytes(), b"[Weapons]\n", ("registry",))
    load_registered_style(g, NX / "plugins/long_sword_extreme.nro")
    w07 = 0x174CF64

    def half(addr, value):
        g.cpu.mem_write(addr, struct.pack("<H", value))

    def byte(addr, value):
        g.cpu.mem_write(addr, bytes([value]))

    def number(addr, value):
        g.cpu.mem_write(addr, struct.pack("<f", value))

    def zero(*_):
        return 0

    # Real sender and receiver packet builders. Host services are replaced
    # only for the scene, renderer, resource manager and equipment refresh.
    manager, session, packet = [g.alloc(n) for n in (128, 128, 200)]
    g.put(0x1885B58, manager)
    g.put(0x188CA48, session)
    for address in (
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
        0x282FD8,
        0x2A2560,
        0x887648,
        0x887660,
        0x2B76CC,
        0x2864C4,
    ):
        g.stub(address, zero)
    for vt in (164, 884, 1132, 348, 376, 508, 2056, 612, 1828, 1356, 1992):
        g.stub(g.get(w07 + vt), zero)
    for address in (0x2A2444, g.get(w07 + 136)):
        g.stub(address, lambda *_: 1)
    g.stub(0x13EFDC4, lambda p, n, *_: g.cpu.mem_write(p, bytes(n)))
    g.stub(0x53A06C, lambda p, _, req, __: half(p + 1204, req & 65535) or 1)
    seq = itertools.count(1)
    g.stub(0x16B920, lambda *_: next(seq))

    # Use the shipped GU w07 HDP and the mod's actual HTS rows.
    arc = quest_arc("w07.arc").read_bytes()
    entries = (
        struct.unpack_from("<64s4I", arc, 12 + 80 * i)
        for i in range(struct.unpack_from("<H", arc, 6)[0])
    )
    _, _, size, _, off = next(
        e
        for e in entries
        if e[1] == 0x4E2EF008 and e[0].rstrip(b"\0").endswith(b"pl_we07_hitdata")
    )
    hdp = zlib.decompress(arc[off : off + size])
    assert hdp[16 + 56 + 38] == 26  # Stock w07 HDP row 1 awards native spirit.
    blade_hitstop = hdp[16 + 56 + 37]
    raw, bank, vt = g.alloc(len(hdp)), g.alloc(112), g.alloc(80)
    g.cpu.mem_write(raw, hdp)
    g.put(bank, vt)
    g.put(vt + 64, 0x270D8C)
    g.put(vt + 68, 0x270D98)
    g.put(bank + 100, raw)
    g.put(bank + 104, raw + 16)
    resources, resource_vt, load, getter = (g.alloc(n) for n in (16, 64, 4, 4))
    g.put(0x2126A64, resources)
    g.put(resources, resource_vt)
    g.put(resource_vt + 48, load)
    assets, groups = {}, {}
    svt = g.alloc(72)
    g.put(svt + 68, getter)
    for pc in (0x7AF3A0, 0x7AF37C):
        g.stub(pc, lambda size, *_: g.alloc(size))
    for path in (NX / "player/hit/frontier_ls").glob("*.hts"):
        data = path.read_bytes()
        asset, raw_shape = g.alloc(112), g.alloc(len(data))
        g.cpu.mem_write(raw_shape, data)
        g.put(asset, svt)
        g.put(asset + 80, 1)
        g.put(asset + 100, raw_shape)
        assert g.run(0x16A1D8, asset) == 1
        groups[asset] = g.get(g.get(asset + 104))
        assets["player\\hit\\frontier_ls\\" + path.stem] = asset
    g.stub(getter, lambda asset, i, *_: groups[asset] if i == 0 else 0)
    g.stub(
        load,
        lambda _, kind, name, __: (
            assets.get(g.string(name), 0) if kind == 0x1857810 else 0
        ),
    )
    model = g.alloc(100)

    def hunter(i):
        p, eq = g.alloc(14000), g.alloc(1600)
        g.put(p, w07)
        g.put(p + 5060, eq)
        byte(eq + 1236, 7)
        byte(p + 4177, i)
        byte(p + 4180, 1)
        g.put(p + 240, model)
        number(p + 28, 2)
        number(p + 1284, 240)
        number(p + 1292, 1)
        g.run(0x1687A8, p + 12320)
        g.put(p + 12696, bank)
        g.put(p + 12700, next(iter(groups)))
        half(p + 5078, 100)
        return p

    roster = [hunter(i) for i in range(4)]
    for i, actor in enumerate(roster):
        g.put(manager + 28 + 4 * i, actor)
    sender = hunter(1)

    def receive(actor, state):
        half(sender + 9478, 16)
        half(sender + 9482, 0x8000 + state)
        g.run(0x1FED90, packet, sender, 0)
        assert g.half(packet + 56) == 0x8000 + state
        g.run(0x1FF308, packet, actor)

    from damage_target import damage_target
    monster, status, monster_vt, part, collision, credit = damage_target(g, w07)

    # Expand the shipped blade against a moving wrist. A different joint
    # index and a translated, rotated pose expose wrong-bone and axis errors.
    probe = itertools.count()

    def verify_hitstop(hit, expected):
        assert g.cpu.mem_read(hit + 108, 1) == bytes([expected])
        actor = g.get(hit + 112)
        original_grade = bytes(g.cpu.mem_read(hit + 6, 1))
        g.cpu.hook_del(g.traps.pop(0x16DB68))
        for grade, timers in ((3, (0., 2. * expected)), (4, (3. * expected, 0.))):
            byte(hit + 6, grade)
            g.run(0x16DB68, 0, hit)
            assert struct.unpack("<2f", g.cpu.mem_read(actor + 10156, 8)) == timers
        g.cpu.mem_write(hit + 6, original_grade)
        g.cpu.mem_write(actor + 10156, bytes(8))
        g.stub(0x16DB68, zero)

    def resolve_blade(actor, power=40):
        hit = actor + 12320
        half(g.get(actor + 5060) + 800, 100)
        before = g.get(status + 880)
        sample = next(probe)
        joints, mapping = g.alloc(320), g.alloc(256)
        g.put(actor + 1172, joints)
        g.put(actor + 1176, mapping)
        byte(mapping + 8, 1)
        x, z = ((0, 1), (1, 0))[sample % 2]
        origin = (480 + sample * 100, 90, -300)
        g.cpu.mem_write(joints + 176, struct.pack(
            "<16f", z, 0, -x, 0, 0, 1, 0, 0, x, 0, z, 0, *origin, 1))
        def point(along, side=0):
            return (origin[0] + x * along + z * side, origin[1],
                    origin[2] + z * along - x * side)
        g.put(collision + 10288, 0)
        assert g.run(0x1696FC, hit) == 3
        for i, (start, end) in enumerate(((100, 220), (60, 100), (220, 260))):
            shape = struct.unpack("<B3x7f", g.cpu.mem_read(collision + 8240 + 32*i, 32))
            assert shape == (2, 52, *point(start), *point(end)), shape
        # A two-unit hurtbox just outside and just inside each capsule boundary.
        for along, side in ((5, 0), (315, 0), (160, 55),
                            ((7, 0), (313, 0), (160, 53))[sample % 3]):
            g.cpu.mem_write(collision + 28, struct.pack("<B3x4f", 1, 2, *point(along, side)))
            g.run(0x16C0C0, collision, hit, monster)
            if along in (5, 315) or side == 55:
                assert g.half(status + 1160) == 0
        assert credit[-1] == (0, roster.index(actor), power), credit
        assert g.half(status + 1160) == power
        g.run(0x97DDC, monster)
        assert g.get(status + 880) == before - power
        g.run(0x9D67C, monster)

    for local in (0, 2):
        g.put(session + 48, local)
        guests = [p for i, p in enumerate(roster) if i != local]
        for p in guests:
            receive(p, 0)
            assert g.half(p + 9482) == 0x8000 and g.cpu.mem_read(p + 5, 1) == b"\0"
            number(p + 1280, 0)
            g.run(0x282DA8, p)
            hit = p + 12320
            previous = g.half(hit + 52)
            for frame in (10, 55, 55):
                number(p + 1280, frame)
                g.run(0x282DA8, p)
                assert g.half(hit + 52) == previous
            number(p + 1280, 56)
            g.run(0x282DA8, p)
            assert g.half(hit + 52) != previous
            assert g.cpu.mem_read(hit + 5, 1) == b"\1"
            assert (g.get(hit + 12), g.half(hit + 56), g.get(hit + 132)) == (
                p,
                40,
                assets[r"player\hit\frontier_ls\blade_0"],
            )
            assert struct.unpack("<2f", g.cpu.mem_read(hit + 92, 8)) == (0.0, 10.0)
            assert g.cpu.mem_read(hit + 87, 1) == b"\0", "Frontier blade inherited GU spirit gain"
            verify_hitstop(hit, blade_hitstop)
            resolve_blade(p)
        for p in guests:
            serial = g.half(p + 12320 + 52)
            g.run(0x282DA8, p)
            assert g.half(p + 12320 + 52) == serial
            receive(p, 255)
            assert g.half(p + 9482) == 0x8000
            # The three genuine source hit frames of a full release, including
            # repeated dispatch at each frame and a skipped intermediate frame.
            receive(p, 84)
            number(p + 1280, 0)
            g.run(0x282DA8, p)
            for frame in (12, 29, 50):
                number(p + 1280, frame)
                g.run(0x282DA8, p)
                n = g.half(p + 12320 + 52)
                assert n != serial and g.half(p + 12320 + 56) == 5
                g.run(0x282DA8, p)
                assert g.half(p + 12320 + 52) == n
                serial = n
        p = guests[0]
        hit = p + 12320
        # The hunter's playhead can advance by less than the collision clock.
        # Keep the native contact aging and verify that the swing frame
        # still reaches the monster as a new attack.
        for state, power in ((7, 37), (18, 17)):
            receive(p, 72)
            number(p + 1280, 0)
            g.run(0x282DA8, p)
            number(p + 1280, 18)
            g.run(0x282DA8, p)
            serial = g.half(hit + 52)
            receive(p, state)
            for frame in range(8, 49):
                number(p + 1280, frame)
                g.run(0x282DA8, p)
                g.run(0x16919C, hit)
            assert g.run(0x169328, hit) == 1, (state, "Spirit I contact expired before frame 48")
            assert g.half(hit + 52) != serial
            resolve_blade(p, power)
    # Two peers sweep different world paths at once, then one leaves the room.
    for address in (0x276748, 0x5CEB4, 0x258FFC):
        g.stub(address, zero)

    def collect():
        g.put(collision + 8224, 0)
        g.run(g.hooks[0x16ABAC], collision)
        hit = g.get(collision + 8224)
        contacts = []
        while hit:
            assert hit not in contacts
            contacts.append(hit)
            hit = g.get(hit + 44)
        return contacts

    # Independent hits must keep their own values even if the borrowed
    # template's hitstop differs from the installed w07 row.
    byte(raw + 16 + 56 + 37, blade_hitstop + 1)
    peers = [roster[1], roster[3]]
    for p in peers:
        receive(p, 77)
        number(p + 1280, 0)
        g.run(0x282DA8, p)
    for frame in (26, 30, 42, 46):
        for i, p in enumerate(peers):
            number(p + 1280, frame)
            g.cpu.mem_write(p + 64, struct.pack("<3f", frame * 10, 0, i * 1000))
        contacts = collect()
    assert {g.get(hit + 12) for hit in contacts} == set(peers) and len(contacts) == 2
    for hit in contacts:
        assert g.cpu.mem_read(hit + 87, 1) == b"\0"
        # Family 108 row 4 carries its own value instead of a blanket override.
        verify_hitstop(hit, 3)
        z = 1000 * peers.index(g.get(hit + 12))
        offset = 8240 + 32 * g.get(hit + 28)
        assert struct.unpack("<B3x7f", g.cpu.mem_read(collision + offset, 32)) == (
            2,
            300.0,
            300.0,
            0.0,
            z,
            420.0,
            0.0,
            z,
        )
    g.put(manager + 28 + 4, 0)
    assert [g.get(hit + 12) for hit in collect()] == [peers[1]]
    g.put(manager + 28 + 4, peers[0])
    remote_equipment = g.get(peers[1] + 5060)
    byte(remote_equipment + 1236, 0)  # The same live roster slot can change weapons.
    assert collect() == []
    byte(remote_equipment + 1236, 7)
    for p in peers:
        receive(p, 65)
        number(p + 1280, 0)
        g.run(0x282DA8, p)
        if g.half(p + 1204) != 1437:
            number(p + 1280, 240)
            g.run(0x282DA8, p)
        assert g.half(p + 1204) == 1437
        number(p + 1280, 128)
        g.run(0x282DA8, p)
    waves = collect()
    assert sorted(g.get(hit + 12) for hit in waves) == sorted(peers * 2)
    for hit in waves:
        assert g.get(hit + 132) == assets[r"player\hit\frontier_ls\discharge"]
        assert g.cpu.mem_read(hit + 87, 1) == b"\0"
        assert g.cpu.mem_read(hit + 108, 1) == b"\0"
    # Both hunters attach a release finisher to their own target. The shared
    # HTS must be positioned for each contact during GU's expansion pass.
    other, other_status, bone, enemy_mgr = (g.alloc(n) for n in (48000, 26000, 64, 64))
    g.put(other, monster_vt)
    g.put(other + 5160, other_status)
    byte(other_status + 437, 1)
    g.put(0x1851FE0, enemy_mgr)
    targets = (monster, other)
    g.cpu.hook_del(g.traps.pop(0x5CEB4))
    g.stub(0x5CEB4, lambda _, index, *__: targets[index] if index < 2 else 0)
    g.cpu.mem_write(bone, struct.pack("<16f", *(float(i % 5 == 0) for i in range(16))))
    get_bone = g.alloc(4)
    g.put(monster_vt + 208, get_bone)
    g.stub(get_bone, lambda *_: bone)
    g.stub(g.get(w07 + 208), lambda *_: bone)
    for i, p in enumerate(peers):
        receive(p, 76)
        number(p + 1280, 0)
        g.run(0x282DA8, p)
        number(p + 1280, 14)
        g.run(0x282DA8, p)
        hit = p + 12320
        g.put(hit + 172, part)
        g.cpu.mem_write(hit + 32, struct.pack("<3f", 30 + i * 600, 0, 0))
        g.run(g.hooks[0x119F574], p, hit, targets[i], 1)
        number(p + 1280, 148)
    followups = [
        h
        for h in collect()
        if g.get(h + 132) == assets[r"player\hit\frontier_ls\release"]
    ]
    assert len(followups) == 2
    for h in followups:
        assert g.cpu.mem_read(h + 87, 1) == b"\0"
        assert g.cpu.mem_read(h + 108, 1) == b"\0"
        i = peers.index(g.get(h + 12))
        assert (
            g.run(0x169878, h, targets[i]) == 1
            and g.run(0x169878, h, targets[1 - i]) == 0
        )
        offset = 8240 + 32 * g.get(h + 28)
        assert struct.unpack("<B3x4f", g.cpu.mem_read(collision + offset, 20)) == (
            1,
            10.0,
            30.0 + 600 * i,
            0.0,
            0.0,
        )
    print(
        "Concurrent delayed release hits kept their separate target monster and impact position"
    )
    print(
        "Concurrent peer instant slashes, departure and weapon-switch cleanup, two discharge waves per peer passed"
    )
    print("GU native blade hitstop and individual path, wave and follow-up values passed")
    print(
        "GU netPlAction send/receive, all guest slots, host without selected style, blade ownership, X miss/hit and HP settlement, three release hits passed"
    )


if __name__ == "__main__":
    replay()
