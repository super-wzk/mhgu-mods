"""Replay the packaged w01 controller, GU Chaos Oil application and independent wave."""

import struct
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
REPO = ROOT.parents[1]
sys.path.insert(0, str(REPO / "plugins/style-registry/tests"))
from verify_weapon_arts import Scene, float_bits
from verify_style import api_for


def source():
    from verify_assets import Machine, MEM, REG, STOP, arm

    m = Machine(REPO / "mhfz/eboot.bin.elf", True)
    blob = (ROOT / "target/assets/sword_shield.fsns").read_bytes()
    assert blob[:8] == b"FSNS\x02\0\x35\0"
    base, bank, table, actor = [MEM + i for i in (0x1000, 0x2000, 0x4000, 0x20000)]
    m.put(0x82BB48C0, base)
    m.put(base + 0x614, bank)
    m.put(bank + 4 * 8, 53)
    m.put(bank + 4 * 8 + 4, table + 16)
    m.c.mem_write(table, blob)
    for state in range(53):
        for field in range(3):
            offset = 16 + 24 * state + 8 * field
            count, start = struct.unpack_from("<II", blob, offset)
            if count:
                m.put(table + offset + 4, table + start)
    m.trap(0x81818BF8, lambda *_: 0)
    count = 0
    for state in range(53):
        n, off = struct.unpack_from("<II", blob, 16 + 24 * state)
        if not n:
            continue
        m.c.mem_write(actor, bytes(4176))
        for k, v in [(3, 4), (18, 1), (20, state), (21, 1), (3394, 3), (820, 1)]:
            m.c.mem_write(actor + k, bytes([v]))
        m.c.reg_write(REG[0], actor)
        m.c.reg_write(arm.UC_ARM_REG_SP, MEM + 0x3FF000)
        m.c.reg_write(arm.UC_ARM_REG_LR, STOP | 1)
        m.c.emu_start(0x811EB2A5, 0x811EB2EA, count=10000)
        assert m.c.reg_read(arm.UC_ARM_REG_PC) == 0x811EB2EA
        _, clip, start, blend, _, _ = struct.unpack_from("<6H", blob, off)
        assert struct.unpack("<2H", m.c.mem_read(actor + 812, 4)) == (clip, clip + 100)
        assert struct.unpack("<H", m.c.mem_read(actor + 796, 2))[0] == start
        assert struct.unpack("<H", m.c.mem_read(actor + 804, 2))[0] == blend // 2
        count += 1
    assert count == 48
    print(
        "FZ w04: all 48 programmed openings match both source layers, start frames and blends"
    )


def resources():
    assets = ROOT / "target/assets"
    nx = ROOT / "dist/sword-shield-extreme/romfs/nativeNX"
    blob = (assets / "sword_shield.fsns").read_bytes()
    clips = {
        1001,
        1002,
        1003,
        1004,
        1419,
        1420,
        1421,
        1448,
        1449,
        1450,
        *range(1465, 1469),
    }
    for state in range(53):
        steps, start = struct.unpack_from("<II", blob, 16 + state * 24)
        for i in range(steps):
            op, clip = struct.unpack_from("<HH", blob, start + i * 12)
            if 3 <= op <= 5:
                clips.add(clip)
    for sex in "mf":
        for group in (0, 2, 4):
            bank = (nx / f"player/mot/frontier_sns/{sex}_{group}.lmt").read_bytes()
            magic, version, count = struct.unpack_from("<4sHH", bank)
            assert (magic, version) == (b"LMT\0", 67)
            offsets = struct.unpack_from(f"<{count}I", bank, 8)
            for clip in clips:
                if (clip - 1000) // 100 == group:
                    assert 0 < offsets[clip % 100] < len(bank), (sex, clip)
    effects = nx / "effect/frontier_sns"
    routes = list(
        struct.iter_unpack("<HH", (assets / "effects/routes.bin").read_bytes())
    )
    assert len(routes) == 23
    for route, meshes in [
        (0, [335, 335, 334, 175]),
        *[(18 + i, [mesh]) for i, mesh in enumerate([501, 504, 505, 506, 507])],
    ]:
        first, count = routes[route]
        assert count == len(meshes)
        for i, mesh in enumerate(meshes):
            efl = (effects / f"e{first + i}.efl").read_bytes()
            assert rf"effect\frontier_sns\m{mesh}".encode() + b"\0" in efl
            assert (effects / f"m{mesh}.mod").is_file()
    print(
        "Packaged male/female motion slots, four vacuum emitters and five independent blade strips passed"
    )


def replay():
    s = Scene(1, 5, "sword-shield", 0x174A354)
    g, a = s.g, s.actor
    hits, waves = [], []
    shape, mgr, vt, load = [g.alloc(n) for n in (112, 16, 64, 4)]
    g.put(shape + 80, 1)
    g.put(0x2126A64, mgr)
    g.put(mgr, vt)
    g.put(vt + 48, load)
    g.stub(load, lambda _, kind, name, flags: shape if kind == 0x1857810 else 0)
    g.put(a + 12700, shape)
    g.put(a + 5078, 100)
    template = g.alloc(56)
    s.stub(g.originals[0x2A8A88], lambda *_: template)
    s.stub(0x887660, lambda *_: 0)
    s.stub(0x887648, lambda *_: 0)
    s.stub(0x1687A8, lambda *_: 0)
    s.stub(0x2B76CC, lambda *_: 0)

    def construct(hit, _, owner, row):
        assert owner in (a, s.remote)
        waves.append((hit, g.half(row + 4)))
        g.put(hit + 12, owner)
        return 1

    s.stub(0x168960, construct)
    api = api_for(g, b"mhgu::StyleRegistryV1", b"SRG1")
    s.stub(
        g.get(api + 8),
        lambda owner, attack, *_: (
            hits.append(struct.unpack("<HHffIII", g.cpu.mem_read(attack, 24))) or 1
        ),
    )
    stun = api_for(g, b"mhgu::StyleStunAttackV1", b"STA1")
    shield_stun = []

    def shield(owner, attack, buildup, _):
        hits.append(struct.unpack("<HHffIII", g.cpu.mem_read(attack, 24)))
        shield_stun.append(buildup)
        return 1

    s.stub(g.get(stun), shield)
    s.stub(0x27E608, lambda *_: 1)
    s.stub(0x27F678, lambda *_: 0)
    s.stub(0x27F5D0, lambda *_: 0)
    s.stub(0x2A2DB4, lambda *_: 0)
    s.stub(g.get(0x174A354 + 440), lambda *_: 0)
    s.stub(0x53A058, lambda *_: 5001)
    collision = g.alloc(10400)
    clock = g.alloc(112)
    g.put(0x1857830, collision)
    g.put(0x2126764, clock)
    g.put(clock + 104, float_bits(2))
    g.put(a + 28, float_bits(2))
    s.stub(g.originals[0x16ABAC], lambda *_: 0)
    registered = []
    s.stub(0x1696E4, lambda hit, *_: registered.append(hit))

    def tick(frame, weapon=0, common=0):
        g.put(a + 1280, float_bits(frame))
        g.put(s.inputs, common)
        g.put(s.inputs + 16, weapon)
        s.tick()
        return g.half(a + 9482)

    def start(oil, remaining):
        g.run(s.controller, 0, 0)
        s.action(1, 0)
        g.put(a + 13100, oil)
        g.put(a + 13108, float_bits(remaining))
        hits.clear()
        waves.clear()
        return tick(0, 1 << 22)

    # Ordinary oils, an expired Chaos Oil, and all three successful native applications.
    for oil, remaining in [(0, 0), (1, 60), (4, 60), (5, 0), (5, 80), (6, 80), (7, 80)]:
        active = oil >= 5 and remaining > 0
        if active:
            s.action(16, 52 + oil - 5)
            g.cpu.mem_write(a + 5, b"\1")
            g.put(a + 13100, 0)
            s.stub(0x280C1C, lambda *_: 0)
            g.run(0x116D608, a, oil - 5)
            assert g.get(a + 13100) == 0
            s.stub(0x280C1C, lambda *_: 1)
            g.run(0x116D608, a, oil - 5)
            assert (g.get(a + 13100), g.get(a + 13108)) == (
                oil,
                float_bits([8100, 10800, 7200][oil - 5]),
            )
        assert start(oil, remaining) == 0x8000 + (55 if active else 2)
        assert g.half(a + 1204) == 1405
        tick(29)
        assert not waves
        tick(31)
        tick(31)
        assert [h[1] for h in hits] == [31]
        assert [power for _, power in waves] == ([16] if active else [])
    # A remote w01 reconstructs Chaos Oil from the replicated status word.
    peer = s.remote
    g.put(peer, 0x174A354)
    g.put(g.get(0x1885B58) + 32, peer)
    g.put(peer + 240, g.alloc(128))
    g.put(peer + 1284, float_bits(240))
    g.put(peer + 28, float_bits(2))
    g.put(peer + 12700, shape)
    for level in (0, 5, 6, 7):
        g.put(a + 10340, 0)
        g.put(a + 13100, level)
        g.put(a + 13108, float_bits(100))
        g.run(0x1168EEC, a)
        status = g.get(a + 10340)
        assert status == (1 << (level - 4) if level else 0)
        g.put(peer + 10340, status)
        assert g.get(peer + 13108) == 0
        g.cpu.mem_write(peer + 9478, struct.pack("<H2xH", 16, 0x8037))
        g.cpu.mem_write(peer + 5, b"\0")
        waves.clear()
        g.run(g.hooks[0x2B8178], peer)
        g.put(peer + 1280, float_bits(30))
        g.run(g.hooks[0x2B8178], peer)
        assert [power for _, power in waves] == ([16] if level else [])
    g.put(g.get(0x1885B58) + 32, 0)
    assert start(5, 80) == 0x8037
    g.put(a + 13108, 0)
    tick(30)
    assert not waves
    assert start(7, 80) == 0x8037
    tick(30)
    (hit,) = [p for p, _ in waves]
    s.action(1, 0)
    registered.clear()
    for _ in range(17):
        g.run(g.hooks[0x16ABAC], collision)
    assert registered == [hit] * 17
    g.run(g.hooks[0x16ABAC], collision)
    assert len(registered) == 17
    # Route the registered contact through the real GU world-shape expander.
    s.action(1, 0)
    start(6, 80)
    g.cpu.mem_write(a + 64, struct.pack("<3f", 40.0, 20.0, 30.0))
    tick(30)
    hit = waves[-1][0]
    for i in range(3):
        g.run(g.hooks[0x16ABAC], collision)
        g.put(collision + 10288, 0)
        assert g.run(g.hooks[0x1696FC], hit) == 1
        kind, radius, x, y, z = struct.unpack(
            "<B3x4f", g.cpu.mem_read(collision + 8240, 20)
        )
        assert (kind, radius, x, y, z) == (1, 200.0, 40.0, 120.0, 30.0 + i * 10.0)
    g.cpu.mem_write(a + 4180, b"\x02")
    registered.clear()
    g.run(g.hooks[0x16ABAC], collision)
    assert not registered

    # Turn ninety degrees, then switch weapons while the wave is still in flight.
    assert start(5, 80) == 0x8037
    g.put(a + 4076, 0x4000)
    tick(30)
    hit = waves[-1][0]
    for i in range(3):
        g.run(g.hooks[0x16ABAC], collision)
        g.put(collision + 10288, 0)
        assert g.run(g.hooks[0x1696FC], hit) == 1
        x, y, z = struct.unpack("<3f", g.cpu.mem_read(collision + 8248, 12))
        assert abs(x - (40 + i * 10)) < 0.001 and y == 120 and abs(z - 30) < 0.001
    equipment = g.get(a + 5060)
    g.cpu.mem_write(equipment + 1236, b"\x07")
    registered.clear()
    g.run(g.hooks[0x16ABAC], collision)
    assert not registered
    g.cpu.mem_write(equipment + 1236, b"\x01")
    g.put(a + 4076, 0)

    # FZ combo pre-input frame 10, execution frame 24, then shield flurry and its follow-up.
    s.action(1, 0)
    assert tick(0, 1) == 0x8000
    assert tick(10, 1) == 0x8000 and tick(24) == 0x8006
    s.action(32, 0)
    assert tick(0, 1 << 22, 1 << 27) == 0x802F
    tick(26)
    assert hits[-1][0:2] == (3, 20) and hits[-1][-1] == 1 and shield_stun[-1] == 13
    tick(30, 1 << 22)
    assert tick(48) == 0x8030
    # Both sprint attacks go through the custom dispatch on flat ground.
    for command, dest in [(1, 36), (1 << 22, 42), (1 << 7, 1)]:
        s.action(16, 0x8000 + 53)
        tick(0)
        assert g.half(a + 1204) == 1448
        assert tick(30, command) == 0x8000 + dest
    print(
        "GU w01: all oil levels and interruption, 31+16 independent contacts, expiry, buffered slash and shield loop passed"
    )


if __name__ == "__main__":
    source()
    resources()
    replay()
