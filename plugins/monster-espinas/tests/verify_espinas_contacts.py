"""Run the built Forge NRO and native GU contact timers/list in Unicorn.

Loader services, resource references, and the surrounding quest are fixtures. No quest, previous player hits, terrain, or damage resolution.
"""

import argparse
import struct as s
from collections import Counter
from pathlib import Path

from unicorn import UC_ARCH_ARM, UC_HOOK_CODE, UC_MODE_ARM, Uc
from unicorn import arm_const as arm
from game_files import GU_ELF, VITA_ELF, assets, elf


def verify(mod):
    root = mod / "romfs/nativeNX"
    nro = (root / "plugins/monster_espinas.nro").read_bytes()
    archive = assets(root / "arc/enemy/em001_00.arc")
    rows = archive[r"enemy\hit_data\em001_00_attackdata", 0x70BB64BA]
    combined = rows[-32:-28] == b"ESH5"
    total = s.unpack_from("<I", rows, 8)[0]
    hts_data = archive[r"enemy\hit_data\em001_00_hitsize", 0x5653C1B0]
    if not combined:
        lmt = archive[r"enemy\em001\em001_00\mot\em001_00_0", 0x76820D81]
        frames = s.unpack_from("<I", lmt, s.unpack_from("<I", lmt, 24)[0] + 8)[0]
        # Independently observed species-80 native event dispatch for each test.
        events = {
            234: [
                (103, 26, 75),
                (74, 26, 75),
                (82, 77, 234),
                (101, 91, 234),
                (78, 95, 234),
            ],
            209: [(53, 2, 209)],
            189: [(69, 10, 189)],
            217: [(71, 87, 217), (71, 116, 217)],
            225: [
                (81, 36, 225),
                (80, 66, 225),
                (79, 70, 225),
                (83, 78, 225),
                (81, 92, 225),
            ],
        }[frames]
        src, srcsegs = elf(VITA_ELF)

        def va(a):
            return next(
                off + a - v for _, off, v, _, n, *_ in srcsegs if v <= a < v + n
            )

        schedule = []
        intact = []
        severed = []
        total = s.unpack_from("<I", rows, 8)[0]
        model = archive[r"enemy\em001\em001_00\mod\em001_00", 0x58A15856]
        nb = s.unpack_from("<H", model, 6)[0]
        bones = s.unpack_from("<I", model, 36)[0]
        remap = model[bones + 152 * nb : bones + 152 * nb + 256]
        for i, (effect, frame, end) in enumerate(events):
            off = 16 + 32 * (19 if i == 0 else 63 + i)
            delay, duration, power = s.unpack_from(
                "<3H", src, va(0x82707B5C + 40 * effect)
            )
            start = frame + delay
            duration = min(end - start, duration)
            assert s.unpack_from("<3H", rows, off) == (start, duration, power)
            g = rows[off + 25]
            intact.append(g)
            schedule.append((g, start, duration))
            sg = s.unpack_from("<H", src, va(0x82707B5C + 40 * effect) + 16)[0]
            pos = va(s.unpack_from("<I", src, va(0x82C779E8 + 4 * sg))[0])
            shapes = []
            cut = []
            skip = 0
            while True:
                bone, mode, n, _, _, _, *geometry = s.unpack_from("<6H7f", src, pos)
                pos += 40
                if bone == 65535:
                    break
                if bone == 125:
                    assert mode == 1
                    skip = n
                    continue
                shape = (nb - 48 + bone, mode, tuple(geometry))
                shapes.append(shape)
                if skip:
                    skip -= 1
                else:
                    cut.append(shape)
            for group, expected_shapes in [(g, shapes), (g + int(cut != shapes), cut)]:
                n, ofs = s.unpack_from("<II", hts_data, 16 + 8 * group)
                assert n == len(expected_shapes)
                for j, (physical, mode, geometry) in enumerate(expected_shapes):
                    a = ofs + 48 * j
                    logical, _, shape = s.unpack_from("<3H", hts_data, a)
                    assert (remap[logical], shape) == (physical, mode)
                    assert (
                        s.unpack_from("<f", hts_data, a + 12)
                        + s.unpack_from("<3f", hts_data, a + 16)
                        + s.unpack_from("<3f", hts_data, a + 32)
                        == geometry
                    )
            severed.append(g + int(cut != shapes))
    cpu = Uc(UC_ARCH_ARM, UC_MODE_ARM)
    cpu.ctl_set_cpu_model(arm.UC_CPU_ARM_MAX)
    game, segs = elf(GU_ELF)
    limit = max(v + ms for _, _, v, _, _, ms, *_ in segs)
    cpu.mem_map(0, (limit + 4095) & ~4095)
    for _, p, v, _, n, *_ in segs:
        cpu.mem_write(v, game[p : p + n])
    base = 0x5000000
    cpu.mem_map(base, 0x200000)
    cpu.mem_write(base, nro)
    cpu.reg_write(
        arm.UC_ARM_REG_C1_C0_2, cpu.reg_read(arm.UC_ARM_REG_C1_C0_2) | 0xF00000
    )
    cpu.reg_write(arm.UC_ARM_REG_FPEXC, 0x40000000)
    regs = [arm.UC_ARM_REG_R0, arm.UC_ARM_REG_R1, arm.UC_ARM_REG_R2, arm.UC_ARM_REG_R3]

    def get(p):
        return int.from_bytes(cpu.mem_read(p, 4), "little")

    def put(p, x):
        cpu.mem_write(p, s.pack("<I", x))

    def stub(p, fn):
        def impl(*_):
            val = fn(*(cpu.reg_read(r) for r in regs))
            cpu.reg_write(regs[0], 0 if val is None else val)
            cpu.reg_write(arm.UC_ARM_REG_PC, cpu.reg_read(arm.UC_ARM_REG_LR))

        cpu.hook_add(UC_HOOK_CODE, impl, begin=p, end=p)

    stop = base + 0xF0000
    sp = base + 0x1E0000
    from collections import deque

    from unicorn import UC_HOOK_BLOCK

    recent = deque(maxlen=16)
    cpu.hook_add(UC_HOOK_BLOCK, lambda _, a, *args: recent.append(hex(a)))

    def run(pc, *args):
        cpu.reg_write(arm.UC_ARM_REG_SP, sp)
        cpu.reg_write(arm.UC_ARM_REG_LR, stop)
        for r, x in zip(regs, args):
            cpu.reg_write(r, x)
        for i, x in enumerate(args[4:]):
            put(sp + 4 * i, x)
        try:
            cpu.emu_start(pc, stop, count=100000)
        except Exception as e:
            raise RuntimeError(
                f"PC {cpu.reg_read(arm.UC_ARM_REG_PC):#x} blocks {recent}"
            ) from e
        assert cpu.reg_read(arm.UC_ARM_REG_PC) == stop, hex(
            cpu.reg_read(arm.UC_ARM_REG_PC)
        )
        return cpu.reg_read(regs[0])

    # Resolve the shipping NRO's MOD0/ELF32 dynamic tables, including relative relocations.
    mod0 = get(base + 4)
    assert nro[mod0 : mod0 + 4] == b"MOD0"
    dyn = base + mod0 + get(base + mod0 + 4)
    tags = {}
    while get(dyn):
        tags[get(dyn)] = get(dyn + 4)
        dyn += 8
    names = {}
    exports = {}
    for i in range(get(base + tags[4] + 4)):
        n, v = s.unpack_from("<II", nro, tags[6] + 16 * i)
        n += tags[5]
        name = nro[n : nro.index(b"\0", n)].decode()
        names[i] = name
        if v:
            exports[name] = base + v
    hooks = {}
    heap = base + 0x100000
    allocations = set()

    def alloc(sz, *_):
        nonlocal heap
        ptr = heap
        heap += (sz + 31) & ~31
        assert heap < sp - 4096
        allocations.add(ptr)
        return ptr

    def free(p, *_):
        allocations.remove(p)

    def hook(retbuf, target, detour, original):
        hooks[target] = detour
        if combined and target == 0x7DC2C:
            # Forge interposes calls from the NRO into the game as well. The
            # saved original executes the PC-independent first instruction then
            # rejoins GU immediately after it.
            trampoline = stop + 0x800
            cpu.mem_write(
                trampoline,
                bytes(cpu.mem_read(target, 4)) + s.pack("<II", 0xE51FF004, target + 4),
            )
            put(original, trampoline)
            cpu.hook_add(
                UC_HOOK_CODE,
                lambda *_: cpu.reg_write(arm.UC_ARM_REG_PC, detour),
                begin=target,
                end=target,
            )
        else:
            put(original, target)
        cpu.mem_write(retbuf, bytes(24))
        return retbuf

    services = {"forge_hook_create": hook, "malloc": alloc, "free": free}
    for tab, size in [(tags[17], tags[18]), (tags[23], tags[2])]:
        for off in range(tab, tab + size, 8):
            where, info = s.unpack_from("<II", nro, off)
            kind = info & 255
            if kind == 23:
                put(base + where, base + get(base + where))
            else:
                assert kind == 22
                i = info >> 8
                p = stop + 4 * (1 + i)
                put(base + where, p)
                stub(p, services.get(names[i], lambda *_: 0))
    actor = base + 0x20000
    status = base + 0x40000
    manager = base + 0x60000
    hts = base + 0x70000
    hde = base + 0x80000
    anchor = status + 2456
    run(exports["forge_onLoad"], base + 0xE0000)
    run(exports["forge_onInit"], base + 0xE0000)
    required = {0x7DC2C, 0x1696E4, 0x16919C, 0x6ED40}
    assert hooks.keys() in (
        required,
        required | {0xCF5F64, 0x1696FC, 0xBAB60, 0xBCC04, 0xBCE6C, 0xA134C},
    )
    if combined:
        assert 0xCF5F64 in hooks
    stub(0x13EFDC4, lambda p, n, *_: cpu.mem_write(p, bytes(n)))  # __aeabi_memclr
    refs = Counter()

    def ref(p, n):
        refs[p] += n

    stub(0x887648, lambda p, *_: ref(p, 1))
    stub(0x887660, lambda p, *_: ref(p, -1))
    stub(0x6ED40, lambda *_: 19)
    stub(0x53A058, lambda p, *_: get(p + 1204) & 65535)
    put(0x1857830, manager)
    put(0x2126764, manager + 0x8000)
    put(manager + 0x8000 + 104, 0x3F800000)
    put(actor, 0x179ACF8)
    put(actor + 5160, status)
    put(actor + 30164, hde)
    put(actor + 30172, hts)
    # GU resolves HTS groups via the header count and a runtime pointer array.
    group_count = s.unpack_from("<I", hts_data, 8)[0]
    header, groups, objects = (hts + n for n in (0x100, 0x200, 0x1000))
    assert 0 < group_count <= 256
    put(hts + 100, header)
    put(hts + 104, groups)
    put(header + 8, group_count)
    for i in range(group_count):
        put(groups + 4 * i, objects + 12 * i)
        put(objects + 12 * i + 4, s.unpack_from("<I", hts_data, 16 + 8 * i)[0])
    put(actor + 1204, 4)
    put(actor + 1292, 0x3F800000)
    put(hde + 100, hde + 4096)
    put(hde + 104, hde + 4112)
    cpu.mem_write(hde + 4096, rows)
    run(0x1687A8, anchor)
    put(actor + 1268, 0xBF800000)

    def submit(hit=anchor):
        put(manager + 8224, 0)
        run(hooks[0x1696E4], hit)
        contacts = []
        p = get(manager + 8224)
        while p:
            assert p not in contacts
            contacts.append(p)
            p = get(p + 44)
        return contacts

    if combined:
        # Test one actor through the actual compiled handler, not a fresh actor
        # per choice. Only GU animation playback, scene and random inputs are fixtures.
        cpu.mem_write(actor + 28, s.pack("<f", 1.0))
        put(status + 880, 5000)
        put(status + 884, 5000)
        pl = base + 0xB0000
        players = base + 0xC0000
        put(0x1885B58, players)
        put(players + 28, pl)
        cpu.mem_write(status + 564, b"\0")
        rng = [99]
        rolls = []

        def draw(*_):
            rolls.append(rng[0])
            return rng[0]

        stub(0x496F0, draw)
        entries = 16 + 32 * s.unpack_from("<I", rows, len(rows) - 20)[0]
        count = s.unpack_from("<I", rows, len(rows) - 16)[0]
        motions = {
            s.unpack_from("<H", rows, entries + 32 * i + 2)[0]: tuple(
                rows[entries + 32 * i : entries + 32 * i + 2]
            )
            for i in range(count)
        }
        blends = {
            s.unpack_from("<H", rows, e + 2)[0]: rows[e + 7]
            for e in range(entries, entries + 32 * count, 32)
        }
        bank = archive[r"enemy\em001\em001_00\mot\em001_00_1", 0x76820D81]
        # Install a relocated GU LMT slot table; the SDK probes it using native B49E54.
        lmt_resource, lmt_header = base + 0xD8000, base + 0xD8100
        count_slots = s.unpack_from("<H", bank, 6)[0]
        put(lmt_resource + 100, lmt_header)
        cpu.mem_write(lmt_header + 6, s.pack("<H", count_slots))
        for i in range(count_slots):
            if s.unpack_from("<I", bank, 8 + 4 * i)[0]:
                put(lmt_header + 8 + 4 * i, base + 0xD8800)
        put(actor + 3844 + 4, lmt_resource)
        played = []

        def play(p, m, *_):
            assert p == actor and m >> 8 == 1
            # Verify GU's VFP call boundary independently of the Rust soft-float target.
            assert cpu.reg_read(arm.UC_ARM_REG_S0) == int.from_bytes(
                s.pack("<f", blends[m]), "little"
            )
            assert cpu.reg_read(arm.UC_ARM_REG_S1) == 0
            pos = s.unpack_from("<I", bank, 8 + 4 * (m & 255))[0]
            assert pos and s.unpack_from("<I", bank, pos + 8)[0] > 0
            if motions[m] in ((3, 1), (3, 2)):
                assert s.unpack_from("<I", bank, pos + 12)[0] == 24
            if motions[m] == (255, 34):
                assert s.unpack_from("<I", bank, pos + 12)[0] == 0
            cpu.mem_write(actor + 1204, s.pack("<H", m) + b"\0")
            put(actor + 1280, 0)
            played.append(motions[m])

        stub(0xAFF40, play)

        def environment(a, state, *args):
            assert a == actor
            cpu.mem_write(status + 442, bytes([state]))

        stub(0xBC8B0, environment)

        # Observe delegation separately from an Espinas update that simply waits.
        delegated = []

        def original_tick(a, flags, *_):
            delegated.append((a, flags))
            return 0xCAFE

        stub(0xCF5F64, original_tick)
        footer = hde + 4112 + (total - 1) * 32
        for address, temporary in [
            (actor, 0),                                     # different monster class
            (footer, int.from_bytes(b"ESP1", "little")),    # replay, not a hunt
            (actor + 5160, 0),                              # no live state
            (status + 880, 0),                              # native death handling
        ]:
            saved = get(address)
            put(address, temporary)
            assert run(hooks[0xCF5F64], actor, 3) == 0xCAFE
            put(address, saved)
        assert delegated == [(actor, 3)] * 4 and not played
        delegated.clear()

        def tick(done=False):
            cpu.mem_write(actor + 1206, bytes([4 if done else 0]))
            cpu.reg_write(arm.UC_ARM_REG_S1, 0x42F60000)
            assert run(hooks[0xCF5F64], actor, 0) == 0 and not delegated

        # A monster selects the bank; the LMT independently decides whether the clip exists.
        spawn_motion = next(m for m, request in motions.items() if request == (255, 39))
        slot_ptr = lmt_header + 8 + 4 * (spawn_motion & 255)
        for addr, size in ((actor + 3844 + 4, 4), (lmt_resource + 100, 4),
                           (lmt_header + 6, 2), (slot_ptr, 4)):
            saved = bytes(cpu.mem_read(addr, size))
            cpu.mem_write(addr, bytes(size))
            tick()
            assert not played
            run(hooks[0x6ED40], actor)
            cpu.mem_write(status + 417, bytes(1))
            cpu.mem_write(addr, saved)

        tick()
        tick(True)
        assert played == [(255, 39), (255, 34)]
        cpu.mem_write(pl + 72, s.pack("<f", 451.0))
        put(status + 880, 4900)
        for _ in range(120):
            tick(True)
        assert len(played) == 2 and refs[hts] == 0
        # Merely approaching may trigger a sleeping pose, never battle AI.
        rng[0] = 25
        for z, pose in ((451.0, 42), (-451.0, 41)):
            cpu.mem_write(pl + 72, s.pack("<f", z))
            for _ in range(124):
                tick()
                if played[-1] == (0, pose):
                    break
            assert played[-1] == (0, pose) and not submit()
            tick(True)
            assert played[-1] == (255, 34)
        cpu.mem_write(pl + 72, s.pack("<f", 451.0))
        rng[0] = 30
        put(status + 880, 4899)
        tick()
        tick(True)
        assert played[-2:] == [(255, 37), (255, 62)]
        tick(True)
        assert played[-1] == (3, 9)
        for _ in range(20):
            tick()
        assert played.count((3, 9)) == 1
        cpu.mem_write(pl + 72, s.pack("<f", 100.0))
        rng[0] = 5
        tick(True)
        assert played[-1] == (3, 12)
        cpu.mem_write(status + 1304, b"\1")
        rng[0] = 8
        tick(True)
        assert played[-1] == (3, 15)
        cpu.mem_write(pl + 72, s.pack("<f", 451.0))
        rng[0] = 0
        tick(True)
        assert played[-1] == (3, 2)

        def run_charge(expected, frames):
            assert len(submit()) == 2
            for i, frame in enumerate(frames):
                cpu.mem_write(actor + 1280, s.pack("<f", frame))
                tick()
                if played[-1] == expected:
                    return i + 1
                if frame == 24 and i > 74:
                    # Both original 107 effects fire at 26, two frames after wrap.
                    assert [
                        s.unpack("<f", cpu.mem_read(h + 92, 4))[0] for h in submit()
                    ] == [2.0, 2.0]
            raise AssertionError((expected, played))

        def charge_frames():
            yield from range(1, 75)
            while True:
                yield from range(24, 75)

        import itertools

        near = run_charge((252, 118), itertools.islice(charge_frames(), 150))
        assert 26 < near < 75 and len(submit()) == 3
        cpu.mem_write(pl + 64, s.pack("<f", 451.0))
        put(pl + 72, 0)
        tick(True)
        assert played[-1] == (253, 6) and len(submit()) == 3
        draws = len(rolls)
        for frame in range(44):
            cpu.mem_write(actor + 1280, s.pack("<f", frame))
            tick()
        assert abs(16384 - get(status + 84)) < 744
        tick(True)
        assert played[-1] == (3, 2) and len(rolls) == draws and len(submit()) == 2
        assert run_charge((252, 118), itertools.islice(charge_frames(), 150)) == near
        put(status + 84, 0)
        put(pl + 64, 0)
        cpu.mem_write(pl + 72, s.pack("<f", 450.0))
        rng[0] = 7
        tick(True)
        assert played[-1] == (3, 4) and len(submit()) == 12
        cpu.mem_write(pl + 72, s.pack("<f", 1000.0))
        rng[0] = 24
        tick(True)
        assert played[-1] == (3, 16) and len(submit()) == 2
        cpu.mem_write(status + 1304, b"\0")
        cpu.mem_write(pl + 72, s.pack("<f", 5001.0))
        rng[0] = 1  # calm far request 3:1: same run but different recovery
        tick(True)
        assert played[-1] == (3, 1)
        far = run_charge((252, 108), itertools.islice(charge_frames(), 400))
        assert far > 75 and len(submit()) == 1
        for roll, move in [
            (16, (0, 5)),
            (17, (0, 7)),
            (18, (0, 8)),
            (21, (254, 9)),
            (22, (0, 9)),
            (23, (0, 11)),
            (25, (0, 13)),
        ]:
            rng[0] = roll
            tick(True)
            assert played[-1] == move, (roll, played[-1])
            for hit in submit():
                run(hooks[0x16919C], hit)
            assert not submit()
        # Reusing the primary slot for an unrelated GU action must retain its
        # own group, including if the actor status still carries the tail-cut bit.
        run(hooks[0x7DC2C], actor, 0, 1, 0)
        cpu.mem_write(status + 948, b"\1\0")
        assert submit() == [anchor] and get(anchor + 136) == rows[16 + 32 + 25]
        put(status + 948, 0)

        def frame(t, end=False):
            cpu.mem_write(actor + 1280, s.pack("<f", t))
            tick(end)

        def choose(distance, roll, rage, expected, posture=0):
            cpu.mem_write(status + 64, bytes(12))
            cpu.mem_write(pl + 64, s.pack("<3f", 0, 0, distance))
            put(status + 84, 0)
            cpu.mem_write(status + 1304, bytes([rage]))
            cpu.mem_write(status + 442, bytes([posture]))
            # GU +704 is alert/target state: a ground monster can see a hunter.
            cpu.mem_write(status + 704, bytes([1]))
            rng[0] = roll
            tick(True)
            assert played[-1] == expected, (distance, roll, expected, played[-3:])

        for roll, move in [(27, (3, 8)), (28, (3, 10))]:
            choose(5001, roll, False, move)
            contacts = submit()
            world = next(
                h for h in contacts if s.unpack("<f", cpu.mem_read(h + 92, 4))[0] == 70
            )
            for i in range(1, 111):
                cpu.mem_write(status + 64, s.pack("<3f", i, 2 * i, 3 * i))
                for h in contacts:
                    run(hooks[0x16919C], h)
                assert bool(run(0x169328, world)) == (70 <= i < 110)
                if i in (70, 88, 109):
                    put(manager + 10288, 0)
                    assert run(hooks[0x1696FC], world) == 1
                    assert s.unpack("<B3xf3f", cpu.mem_read(manager + 8240, 20)) == (
                        1,
                        400,
                        70,
                        140,
                        210,
                    )
        for roll, move in [(18, (3, 13)), (19, (3, 14))]:
            choose(100, roll, True, move)
            for cycle in range(2):
                draw_count = len(rolls)
                for t in range(1, 105):
                    frame(t)
                assert get(status + 84) != 0
                if not cycle:
                    frame(105, True)
                    assert played[-2:] == [move, move] and len(rolls) == draw_count
        choose(1001, 31, False, (3, 0))
        choose(100, 0, False, (1, 0))
        frame(1)
        assert played[-1] == (252, 1)
        frame(2)
        assert played[-1] == (3, 11)
        area = base + 0xC8000
        node = area + 0x1000
        put(0x18977A0, area)
        stub(0x4FF9C0, lambda *_: area)
        stub(0x3CE060, lambda *_: node)
        put(area + 104, 1)
        put(node + 48, 1)
        cpu.mem_write(node + 40, bytes([16]))
        cpu.mem_write(node + 24, s.pack("<f", 550.0))
        choose(5001, 8, False, (1, 1))
        tick(True)
        assert played[-1] == (1, 0)

        def clip():
            pos = s.unpack_from("<I", bank, 8 + 4 * (get(actor + 1204) & 255))[0]
            return s.unpack_from("<II", bank, pos + 8)

        def until(move, limit=1800):
            n, loop = clip()
            f = 0
            for i in range(limit):
                frame(f, f + 1 >= n and loop == 0xFFFFFFFF)
                if played[-1] == move:
                    return i
                if f + 1 < n:
                    f += 1
                elif loop != 0xFFFFFFFF:
                    f = loop
            raise AssertionError((move, played[-5:], clip()))

        until((252, 1))
        for roll, nm, turn in [(14, 18, 122), (25, 19, 119)]:
            choose(2701, roll, True, (3, nm))
            draws = len(rolls)
            for i in range(nm - 17):
                until((250, turn))
                until((3, 19 if nm == 19 and i == 0 else 18))
            until((252, 118))
            assert len(rolls) == draws
        shell_mgr, param, remap, joints = (
            base + v for v in (0xC9000, 0xCA000, 0xCB000, 0xCC000)
        )
        put(0x1892550, shell_mgr)
        # Run GU's selector: field location is fixed, resource ID varies by species/variant.
        stub(0x48C388, lambda *_: 0)  # skip actual resource registration
        def select_resource(variant, expected):
            cpu.mem_write(actor + 46580, bytes([1, variant]))
            run(0xD0CA90, actor)
            assert get(actor + 51908) == expected
            return expected

        expected_resource = select_resource(0, 84)
        put(actor + 1176, remap)
        put(actor + 1172, joints)
        model = archive[r"enemy\em001\em001_00\mod\em001_00", 0x58A15856]
        bones = s.unpack_from("<I", model, 36)[0]
        nb = s.unpack_from("<H", model, 6)[0]
        logical = model[bones + 24 * (nb - 48 + 34)]
        cpu.mem_write(remap, bytes([255]) * 256)
        cpu.mem_write(remap + logical, b"\x01")
        matrix = joints + 160 + 16
        cpu.mem_write(
            matrix, s.pack("<16f", 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 15, 100, 450, 1)
        )
        shell = base + 0xCD000
        emitted = []
        constructed = []
        reject = False
        def construct(*_):
            constructed.append(param)
            return param
        stub(0x3F93F8, construct)

        def create(m, p, *_):
            assert (m, p, get(p + 4), get(p + 12)) == (shell_mgr, param, expected_resource, actor)
            if reject:
                return 0
            emitted.append((get(p + 32), get(p + 36)))
            return shell

        stub(0x48C4F0, create)

        def shot(t, pitch, yaw):
            import math

            frame(t - 1)
            before = len(emitted)
            frame(t)
            frame(t)
            assert emitted[before:] == [(pitch, yaw & 65535)]
            for k in (64, 4096):
                assert s.unpack("<3f", cpu.mem_read(shell + k, 12)) == (
                    15.0,
                    20.0,
                    530.0,
                )
            v = s.unpack("<3f", cpu.mem_read(shell + 4112, 12))
            pp, yy = (angle * math.tau / 65536 for angle in (pitch, yaw))
            want = (
                60 * math.sin(yy) * math.cos(pp),
                -60 * math.sin(pp),
                60 * math.cos(yy) * math.cos(pp),
            )
            assert max(abs(x - y) for x, y in zip(want, v)) < 0.001

        choose(1001, 14, True, (1, 1))
        tick(True)
        assert played[-1] == (3, 5)
        # Replay the emission frame with missing manager, sentinel slot and native rejection.
        sentinel = bytes([0xCD]) * 12
        cpu.mem_write(shell + 4112, sentinel)
        for mgr, slot, reject, constructions in [(0, 84, False, 0), (shell_mgr, 413, False, 0), (shell_mgr, 84, True, 1)]:
            put(0x1892550, mgr)
            put(actor + 51908, slot)
            before = len(constructed)
            frame(79)
            frame(80)
            assert len(constructed) - before == constructions and not emitted
            assert bytes(cpu.mem_read(shell + 4112, 12)) == sentinel
        reject = False
        shot(80, 0, 0)
        for roll, id, last in [(25, 6, 115), (27, 7, 116)]:
            choose(451, roll, True, (3, id))
            shot(76, 0x16C1, -0x1332)
            # The second emission in the same attack must resolve the new ID.
            expected_resource = select_resource(2, 85)
            shot(122, 0x16C1, 0x1333)
            expected_resource = select_resource(0, 84)
            tick(True)
            assert played[-1] == (252, last)
            if last == 115:
                shot(26, 0x1C72, 0)
            else:
                for t in range(36, 49):
                    frame(t)
                shot(50, 0x1E94, 0)
                assert s.unpack("<f", cpu.mem_read(status + 68, 4))[0] > 0
                length, _ = clip()
                for t in range(51, 800):
                    frame(min(t, length - 1))
                    if played[-1] == (252, 57):
                        break
                assert played[-1] == (252, 57) and get(status + 68) == 0
        choose(451, 10, True, (3, 17))
        until((252, 57))
        assert get(status + 68) == 0
        for posture in (1, 3):
            choose(451, 0, False, (2, 1), posture=posture)
            cpu.mem_write(status + 68, s.pack("<f", 400.0))
            until((252, 11))
            until((252, 19))
            assert get(status + 68) == 0 and len(emitted) == 7
        # Compare actual native lookups in both states and use the GU entry
        # point for adding anger, without setting the enraged bit for it.
        for mode, expected in [(0, [15, 20, 40]), (1, [70, 40, 30]), (2, [15, 20, 40])]:
            cpu.mem_write(status + 1304, bytes([mode]))
            hz = run(hooks[0xBAB60], actor, 0)
            assert list(cpu.mem_read(hz, 3)) == expected
            assert list(cpu.mem_read(hz + 8, 2)) == [100, 0]
        # Espinas only accumulates its FZ anger gauge while calm.
        gauge = get(status + 1296) & 0xFFFF
        assert run(hooks[0xBCC04], actor, 10) == gauge
        cpu.mem_write(status + 1304, b"\0")
        cpu.mem_write(status + 704, b"\0")
        put(status + 880, 4500)
        assert run(hooks[0xBCC04], actor, 349) == 698
        assert run(hooks[0xBCC04], actor, 1) == 700
        # Real BCE6C may read the donor's resources and invoke notifications.
        # Put a mock resource object in its normal slot. Stub the trait query;
        # leave GU's request-bit set/clear helpers intact.
        res = base + 0xD5000
        put(actor + 30188, res)
        vt = get(actor)
        stub(get(vt + 864), lambda *_: 0)
        # Interpose NRO calls to the hooked GU function, as the real loader does.
        detour = hooks[0xBCE6C]

        # For this call the saved original address equals the entry; hook only
        # on our call from NRO, not the detour's call to the original.
        def through(_, pc, *args):
            lr = cpu.reg_read(arm.UC_ARM_REG_LR)
            if base <= lr < base + len(nro) and not gate[0]:
                gate[0] = True
                cpu.reg_write(arm.UC_ARM_REG_PC, detour)

        gate = [False]
        cpu.hook_add(UC_HOOK_CODE, through, begin=0xBCE6C, end=0xBCE6C)
        rng[0] = 18
        cpu.mem_write(pl + 72, s.pack("<f", 100.0))
        tick(True)
        assert played[-1] == (3, 13) and get(status + 1304) & 255 == 1
        assert s.unpack("<f", cpu.mem_read(status + 1300, 4))[0] == 10800.0
        assert run(hooks[0xBCC04], actor, 999) == 700

        # Isolate GU's optional secondary rage path using a synthetic group 17
        # and parameter block. Neither is a claim about the FZ Espinas moveset.
        commands, secondary = res + 0x200, res + 0x400
        put(actor + 29428, commands)
        put(res + 100 + 120, secondary)
        cpu.mem_write(secondary, s.pack("<2h", 42, 1234))
        cpu.mem_write(secondary + 16, s.pack("<10f", *([1.0] * 10)))
        run(0xBCFC0, actor, 41)
        assert get(status + 1306) & 0xFFFF == 0  # group 17 is absent
        put(commands + 4 * 17, res + 0x600)
        run(0xBCFC0, actor, 41)
        assert get(status + 1306) & 0xFFFF == 41 and not get(status + 866) & 16
        run(0xBCFC0, actor, 1)
        assert get(status + 1306) & 0xFFFF == 42 and get(status + 866) & 16
        assert run(0x8172C, actor) == 1  # request pending, still in ordinary rage
        run(detour, actor, 2)  # invoke the setter as GU's group-17 dispatch does
        assert run(0x8172C, actor) == 0 and run(0x81774, actor) == 1
        assert not get(status + 866) & 16
        assert s.unpack("<f", cpu.mem_read(status + 1300, 4))[0] == 1234.0
        run(0xBA874, actor, 0)
        assert cpu.mem_read(status + 1304, 1) == b"\0" and get(status + 1300) == 0
        tail_name = r"enemy\em001\em001_00\mod\em001_00_tail"
        tail = archive[tail_name, 0x58A15856]
        assert s.unpack_from("<3H", tail, 6) == (0, 3, 22)
        assert archive[tail_name, 0x2749C8A8] == archive[tail_name[:-5], 0x2749C8A8]
        main_mesh = s.unpack_from("<I", model, 48)[0]
        drop_mesh = s.unpack_from("<I", tail, 48)[0]
        for material in (6, 10, 11, 12):
            q = main_mesh + 48 * material
            assert (
                s.unpack_from("<I", model, q + 4)[0] & 4095
                == {6: 8, 10: 101, 11: 0, 12: 101}[material]
            )
            assert bool(s.unpack_from("<I", model, q + 8)[0] & 1) == (material != 11)
        for i in range(3):
            mesh = drop_mesh + 48 * i
            assert (s.unpack_from("<I", tail, mesh + 4)[0] >> 12) & 4095 == 10 + i
            assert s.unpack_from("<I", tail, mesh + 20)[0] == 0x5E7F2030
        # Bank 3 slot 17 is GU's ordinary 11:0 death request. Its animation
        # must now drive the added FZ joints rather than the hidden donor rig.
        death_bank = archive[r"enemy\em001\em001_00\mot\em001_00_3", 0x76820D81]
        dead = s.unpack_from("<I", death_bank, 8 + 17 * 4)[0]
        track, n, frames, loop = s.unpack_from("<4I", death_bank, dead)
        assert frames > 1 and loop == 0xFFFFFFFF
        mapping = model[bones + 152 * nb : bones + 152 * nb + 256]
        assert all(
            bone == 255 or nb - 48 <= mapping[bone] < nb
            for bone in (death_bank[track + 36 * i + 3] for i in range(n))
        )
        # Capture the exact program passed to GU's native damage handler.
        stub(0xA134C, lambda a, p, *_: p)
        cpu.mem_write(actor + 29664, bytes([10, 7]))
        rx = archive[r"enemy\em001\em001_00\mot\em001_00_4", 0x76820D81]
        saved_program = None
        cpu.mem_write(status + 417, b"\1")
        assert run(hooks[0xA134C], actor, 12345, 0) == 12345  # no program installed mid-action
        for part, count, want in [
            (0, 1, [60]),
            (7, 1, [60]),
            (2, 1, [64]),
            (3, 1, [66]),
            (3, 2, [69, 70, 71]),
            (4, 2, [74, 75, 72]),
            (6, 2, [63]),
        ]:
            cpu.mem_write(status + 417, b"\0")
            cpu.mem_write(status + 23755, bytes([part << 3]))
            cpu.mem_write(status + 956 + 12 * part, bytes([count]))
            program = run(hooks[0xA134C], actor, 12345, 0)
            assert program != 12345 and (saved_program is None or program == saved_program)
            saved_program = program
            ops = [0, 3, 0, 0, 255] if len(want) == 3 else [0, 255]
            stage = []
            for i, expected_op in enumerate(ops):
                op, operand = s.unpack("<2I", cpu.mem_read(program + 8 * i, 8))
                assert op == expected_op
                if op == 255:
                    assert operand == 0
                elif op == 3:
                    assert operand == 240
                else:
                    m, blend, start, flags, until, params = s.unpack("<If4I", cpu.mem_read(operand, 24))
                    assert m >> 8 == 4 and blend == blends[m]
                    assert (start, flags, until, params) == (0, 0, 4 if i == 2 else 1, 0)
                    entry = s.unpack_from("<I", rx, 8 + 4 * (m & 255))[0]
                    assert entry and s.unpack_from("<I", rx, entry + 8)[0] > 1
                    stage.append(motions[m])
            assert stage == [(249, i) for i in want]
            cpu.mem_write(status + 417, b"\1")
            assert run(hooks[0xA134C], actor, 12345, 0) == saved_program
        cpu.mem_write(actor + 29664, bytes([11, 0]))
        assert run(hooks[0xA134C], actor, 12345, 0) == 12345
        # Native slot may still be owned until the actor destructor.
        assert run(hooks[0x6ED40], actor) == 19 and refs[hts] == 1
        run(0x1688C8, anchor)
        assert refs[hts] == 0
        print(
            f"{mod.name}: one actor sleep / wake / battle, source route sequence {played}; 107 -> 118 after {near} frames, 107 -> 108 after {far} frames."
        )
        return
    # The original donor must not acquire supplementary contacts.
    put(hde + 4096 + 8, 64)
    run(hooks[0x7DC2C], actor, 0, 19, 0)
    assert not allocations and refs[hts] == 1
    put(hde + 4096 + 8, total)
    run(hooks[0x7DC2C], actor, 0, 19, 0)
    assert refs[hts] == len(events)

    assert not submit(status + 2640)
    first = submit()
    assert len(first) == len(events) and first[-1] == anchor
    assert [get(p + 136) for p in first] == intact[::-1]
    cpu.mem_write(status + 948, b"\1\0")
    assert submit() == first and [get(p + 136) for p in first] == severed[::-1]
    cpu.mem_write(status + 948, bytes(2))
    # Match the order in the GU collision pass: advance all clocks, then
    # call the actual native eligibility predicate (not a reconstruction).
    peak = 0
    for dt in (1.0, 0.5, 2.0):
        put(manager + 0x8000 + 104, s.unpack("<I", s.pack("<f", dt))[0])
        run(hooks[0x7DC2C], actor, 0, 19, 0)
        assert submit() == first and refs[hts] == len(events)
        for tick in range(1, int(frames / dt) + 1):
            t = tick * dt
            contacts = submit()
            for hit in contacts:
                run(hooks[0x16919C], hit)
            enabled = [get(hit + 136) for hit in contacts if run(0x169328, hit)]
            expected = [
                g
                for g, start, duration in reversed(schedule)
                if start <= t < start + duration
            ]
            assert enabled == expected, (dt, t, enabled, expected)
            peak = max(peak, len(enabled))
    # A missing HTS table disables the already-linked primary and skips every extra.
    # The next pass must read the newly attached table.
    for field in (100, 104):
        run(hooks[0x7DC2C], actor, 0, 19, 0)
        saved = get(hts + field)
        put(hts + field, 0)
        assert submit() == [anchor]
        assert all(cpu.mem_read(p + 5, 1) == b"\0" for p in first)
        put(hts + field, saved)
        run(hooks[0x7DC2C], actor, 0, 19, 0)
        assert submit() == first
    # A numbered slot can be missing at runtime. Reject it even if in range.
    run(hooks[0x7DC2C], actor, 0, 19, 0)
    saved_slot = get(groups + 4 * intact[0])
    put(groups + 4 * intact[0], 0)
    submit()
    assert cpu.mem_read(anchor + 5, 1) == b"\0"
    put(groups + 4 * intact[0], saved_slot)
    # Same index in another loaded HTS must not be assigned to this contact.
    run(hooks[0x7DC2C], actor, 0, 19, 0)
    other = hts + 0x3000
    put(other + 100, header)
    put(other + 104, groups)
    put(actor + 30172, other)
    submit()
    assert cpu.mem_read(anchor + 5, 1) == b"\0" and get(anchor + 132) == hts
    put(actor + 30172, hts)
    # Out-of-range cut override: never index past the live HTS array.
    if group_count < 256:
        run(hooks[0x7DC2C], actor, 0, 19, 0)
        cut = hde + 4112 + (63 + len(events)) * 32
        saved_cut = bytes(cpu.mem_read(cut, 1))
        cpu.mem_write(cut, bytes([group_count]))
        cpu.mem_write(status + 948, b"\1\0")
        submit()
        assert cpu.mem_read(anchor + 5, 1) == b"\0"
        cpu.mem_write(cut, saved_cut)
        cpu.mem_write(status + 948, b"\0\0")
    run(hooks[0x7DC2C], actor, 0, 19, 0)
    put(actor + 1204, 1)
    for hit in submit():
        run(hooks[0x16919C], hit)
    assert not submit()
    assert run(hooks[0x6ED40], actor) == 19 and refs[hts] == 1
    run(0x1688C8, anchor)
    assert refs[hts] == 0
    assert not submit()
    print(
        f"{mod.name}: {len(events)} native contacts, {frames} frames, peak {peak}; geometry, tail variants, rearm, cancellation and release passed."
    )


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mod", type=Path, nargs="+")
    for mod in parser.parse_args().mod:
        verify(mod)
