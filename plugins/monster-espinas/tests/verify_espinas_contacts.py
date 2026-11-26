"""Run the built Forge NRO and native GU contact timers/list in Unicorn.

Loader services, resource references, and the surrounding quest are fixtures.
Resolved hit amounts enter GU's native HP and anger accounting; weapon damage,
terrain and a whole quest are not simulated.
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
    independent = (root / "arc/enemy/em107_00.arc").is_file()
    if independent:
        archive = assets(root / "arc/enemy/em107_00.arc")
    else:
        archive = assets(root / "arc/enemy/em001_00.arc")
    namespace = "em107" if independent else "em001"
    def resource(path, kind):
        return archive[path.replace("em001", namespace), kind]
    rows = resource(r"enemy\hit_data\em001_00_attackdata", 0x70BB64BA)
    combined = rows[-32:-28] == b"ESH5"
    total = s.unpack_from("<I", rows, 8)[0]
    hts_data = resource(r"enemy\hit_data\em001_00_hitsize", 0x5653C1B0)
    if not combined:
        lmt = resource(r"enemy\em001\em001_00\mot\em001_00_0", 0x76820D81)
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
        model = resource(r"enemy\em001\em001_00\mod\em001_00", 0x58A15856)
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

        return cpu.hook_add(UC_HOOK_CODE, impl, begin=p, end=p)

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
        if combined and target in (0x7DC2C, 0xBCC04, 0xBCE6C, 0x951A80):
            # Forge interposes calls from the NRO into the game as well. The
            # saved original executes the PC-independent first instruction then
            # rejoins GU immediately after it.
            trampoline = stop + 0x800 + 16 * (0x7DC2C, 0xBCC04, 0xBCE6C, 0x951A80).index(target)
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

    messages = []
    def log(level, fmt, msg, _):
        messages.append(bytes(cpu.mem_read(msg, 512)).split(b"\0", 1)[0].decode())
    services = {"forge_hook_create": hook, "malloc": alloc, "free": free, "forge_log": log}
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
    # Keep the actor outside the NRO image as its code/data grow.
    actor = base + 0x200000
    cpu.mem_map(actor, 0x10000)
    status = base + 0x40000
    manager = base + 0x60000
    hts = base + 0x70000
    hde = base + 0x80000
    anchor = status + 2456
    world_handlers = []
    if independent:
        # The separate quest test runs the actual provider. Here a minimal
        # pair of published services lets the combat replay call the content directly.
        owner = base + 0xe0800
        put(0x18a6190 + 56, owner)
        stub(0x13effd4, lambda *_: owner)
        descriptors = {}

        def publish_service(at, identifier, magic, handler):
            descriptor, api, name, callback = (at + n for n in (0, 64, 80, 128))
            cpu.mem_write(name, identifier + b"\0")
            put(descriptor + 4, name)
            put(descriptor + 32, int.from_bytes(magic, "little"))
            put(descriptor + 36, api)
            put(api, callback)
            stub(callback, handler)
            descriptors[run(0x7CBDB4, name, 0xFFFFFFFF) & 0x7FFFFFFF] = descriptor

        def register_species(raw, key, length, template):
            assert (raw, bytes(cpu.mem_read(key, length)), template) == (107, b"frontier:espinas", 1)
            return 0

        publish_service(owner + 0x100, b"mhgu::MonsterRegistryV1", b"MRG1", register_species)
        publish_service(owner + 0x200, b"mhgu::WorldContactV1", b"WCT1", lambda expand, *_: world_handlers.append(expand))
        stub(0x7aef68, lambda key, *_: descriptors.get(key, 0))
    run(exports["forge_onLoad"], base + 0xE0000)
    run(exports["forge_onInit"], base + 0xE0000)
    required = {0x7DC2C, 0x1696E4, 0x16919C, 0x6ED40}
    assert hooks.keys() in (
        required,
        required | {0xCF5F64, 0xBA874, 0xBCC04, 0xBCE6C, 0xA134C, 0x951A80} | ({0x4A858, 0x152960, 0x39BB30, 0x3D7E90, 0x560CA4, 0x3A3178, 0x192340, 0x56EF50, 0x55BDA4, 0xB84114} if independent else {0x1696FC}),
    )
    if independent:
        assert len(world_handlers) == 1
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
    put(actor + 46576, 107 if independent else 1)
    put(actor + 46580, 107 if independent else 1)
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
    for rate in [actor + 1292, status + 908, status + 916, status + 920]:
        put(rate, 0x3F800000)
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
        put(actor + 1188, 1)
        cpu.mem_write(actor + 28, s.pack("<f", 1.0))
        put(status + 880, 5000)
        put(status + 884, 5000)
        pl = base + 0xB0000
        players = base + 0xC0000
        put(0x1885B58, players)
        put(players + 28, pl)
        hunter_vt = base + 0x17a000
        active = hunter_vt + 512
        put(pl, hunter_vt)
        put(hunter_vt + 136, active)
        stub(active, lambda *_: 1)
        unavailable = set()
        stub(0x49240, lambda slot, *_: int(slot in unavailable))
        # Use GU's own initial "no target" descriptor, as B8C54 does.
        run(0xC6E14, actor, 255, 0, 0, 0)
        put(actor + 240, base + 0xD8700)
        rng = [99]
        rolls = []

        def draw(*_):
            value = rng.pop(0) if len(rng) > 1 else rng[0]
            rolls.append(value)
            return value

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
        blend_hints = {
            s.unpack_from("<H", rows, e + 2)[0]: rows[e + 29]
            for e in range(entries, entries + 32 * count, 32)
        }
        bank = resource(r"enemy\em001\em001_00\mot\em001_00_1", 0x76820D81)
        # Install a relocated GU LMT slot table; the SDK probes it using native B49E54.
        lmt_resource, lmt_header = base + 0xD8000, base + 0xD8100
        count_slots = s.unpack_from("<H", bank, 6)[0]
        put(lmt_resource + 100, lmt_header)
        cpu.mem_write(lmt_header + 6, s.pack("<H", count_slots))
        for i in range(count_slots):
            if s.unpack_from("<I", bank, 8 + 4 * i)[0]:
                put(lmt_header + 8 + 4 * i, base + 0xD8800)
        put(actor + 3844 + 4, lmt_resource)
        # Retain both the full motion timeline and the battle-action sequence.
        timeline = []
        action_time = [0.]
        rest_started = [0.]
        played = []
        rest_requests = {(0, 1), (0, 17)}

        def play(p, m, *_):
            assert p == actor and m >> 8 == 1
            # Verify GU's VFP call boundary independently of the Rust soft-float target.
            half_blend = blends[m] or blend_hints.get(get(actor + 1204) & 65535, 0)
            blend = 2 * (half_blend + 1) if half_blend else 0
            assert cpu.reg_read(arm.UC_ARM_REG_S0) == int.from_bytes(
                s.pack("<f", blend), "little"
            )
            assert cpu.reg_read(arm.UC_ARM_REG_S1) == 0
            pos = s.unpack_from("<I", bank, 8 + 4 * (m & 255))[0]
            assert pos and s.unpack_from("<I", bank, pos + 8)[0] > 0
            if motions[m] in ((3, 1), (3, 2)):
                assert s.unpack_from("<I", bank, pos + 12)[0] == 24
            if motions[m] == (255, 34):
                assert s.unpack_from("<I", bank, pos + 12)[0] == 0
            cpu.mem_write(actor + 1204, s.pack("<HH", m, 0x100 if blend else 0))
            for off in (1264, 1268):
                cpu.mem_write(actor + off, s.pack("<f", blend))
            cpu.mem_write(actor + 1296, s.pack("<f", float(bool(blend))))
            length, loop = s.unpack_from("<II", bank, pos + 8)
            cpu.mem_write(actor + 1276, s.pack("<4f", 0, 0, length - 1, 0 if loop == 0xffffffff else loop))
            put(actor + 1212, 4 if loop == 0xffffffff else 0)
            timeline.append(motions[m])
            if motions[m] in rest_requests:
                rest_started[0] = action_time[0]
            if motions[m] not in rest_requests and motions[m][0] != 248:
                played.append(motions[m])

        stub(0xAFF40, play)

        def environment(a, state, *args):
            assert a == actor
            cpu.mem_write(status + 442, bytes([state]))

        if independent:
            # A loaded DTB supplies GU's per-posture shape and fixed hover height.
            dtbase = alloc(224)
            put(actor + 30184, dtbase)
            put(dtbase + 132, alloc(104))
            cpu.mem_write(dtbase + 197, b"\1")
            cpu.mem_write(dtbase + 152, s.pack("<f", 350.))
        else:
            stub(0xBC8B0, environment)

        # Observe delegation separately from an Espinas update that simply waits.
        delegated = []

        def original_tick(a, flags, *_):
            delegated.append((a, flags))
            return 0xCAFE

        stub(0xCF5F64, original_tick)
        if independent:
            emc = resource(r"enemy\cmd_tbl\em001_00_cmdtbl", 0x3aabba02)
            # Load the shipping EMC with GU's decoder. The stream and heap are
            # test services; perception, script selection and command fetch run natively.
            emc_obj, stream, stream_vt, arena, arena_vt = (alloc(n) for n in (224, 32, 80, 32, 64))
            put(emc_obj, 0x1732ED4)
            put(stream, stream_vt)
            put(arena, arena_vt)
            put(0x18A6308, arena)
            stub(0x7AA47C, lambda *_: arena)
            stub(0x7AF3A0, alloc)
            stub(0x13EFE30, lambda dst, src, n, _: cpu.mem_write(dst, bytes(cpu.mem_read(src, n))) or dst)
            for vt, off, service in (
                (stream_vt, 76, lambda *_: len(emc)),
                (stream_vt, 52, lambda _, dst, n, __: cpu.mem_write(dst, emc) or n),
                (arena_vt, 28, lambda _, n, *__: alloc(n)),
                (arena_vt, 52, lambda *_: 0),
            ):
                addr = alloc(4)
                put(vt + off, addr)
                stub(addr, service)
            assert run(0x520A0, emc_obj, stream) == 1
            put(actor + 30196, emc_obj)
            put(0x18570D0, alloc(80))
            # Player class descriptor and mounted-state virtuals for C4BA8.
            player_type = 0x1898670  # GU player class descriptor (C5424)
            for off, addr, value in ((20, active + 4, player_type), (784, active + 8, 0)):
                put(hunter_vt + off, addr)
                stub(addr, lambda *_, value=value: value)
            put(0x1892550, alloc(29000))  # shell manager: no live projectiles
            cpu.mem_write(status + 586, b"\1")
            # GU accepts ordinary AI requests on the local authority. Leave the
            # actual 75574 -> 76154 validation, class remap and stage writes live.
            cpu.mem_write(status + 24228, b"\xff\xff")  # no attached scene effect
            import math
            def atan2f(*_):
                y, x = (s.unpack("<f", s.pack("<I", cpu.reg_read(r)))[0]
                        for r in (arm.UC_ARM_REG_S0, arm.UC_ARM_REG_S1))
                cpu.reg_write(arm.UC_ARM_REG_S0, s.unpack("<I", s.pack("<f", math.atan2(y, x)))[0])
            stub(0x13efd70, atan2f)
            put(0x184d750, alloc(96))  # no scripted scene actor
            put(actor + 30188, alloc(256))  # DTT with no optional per-action override
            cpu.mem_write(actor + 29728, b"\1")
            def perceive(native, args, expected):
                run(0x826B4, actor)
                assert run(native, actor, *args) == 1
                assert bytes(cpu.mem_read(actor + 29436, 2)) == bytes(expected)
                script = get(actor + 29432)
                assert bytes(cpu.mem_read(script, 4)) == b"\0\1\0\xff", (expected, bytes(cpu.mem_read(script, 8)).hex())
                observed = []
                def observe(*_):
                    observed.append(tuple(cpu.reg_read(r) for r in regs[:3]))
                trap = cpu.hook_add(UC_HOOK_CODE, observe, begin=0x75574, end=0x75574)
                run(0x82F80, actor, 0, alloc(1024))
                cpu.hook_del(trap)
                assert observed == [(actor, 1, 0)], (expected, observed)
                assert bytes(cpu.mem_read(actor + 29664, 2)) == bytes((1, 0))
                assert cpu.mem_read(status + 417, 1) == b"\0"


            perceive(0x7EA84, (1,), (4, 0))
            dispatcher = get(get(actor) + 500)
            assert dispatcher == 0xcf1550
            # The saved original is mocked here for the fallback cases below.
            # Hook the final class-dispatch branch instead of interposing the mock.
            cpu.hook_add(UC_HOOK_CODE, lambda *_: cpu.reg_write(arm.UC_ARM_REG_PC, hooks[0xCF5F64]),
                         begin=0xcf1958, end=0xcf1958)
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
        def note_update(*_):
            action_time[0] += s.unpack("<f", cpu.mem_read(actor + 28, 4))[0]
        cpu.hook_add(UC_HOOK_CODE, note_update, begin=hooks[0xCF5F64], end=hooks[0xCF5F64])

        def update(done=False):
            cpu.mem_write(actor + 1206, bytes([4 if done else 0]))
            cpu.reg_write(arm.UC_ARM_REG_S1, 0x42F60000)
            assert run(dispatcher if independent else hooks[0xCF5F64], actor, 0) == 0 and not delegated

        def finish_rest():
            # Most move-specific checks skip over the already asserted idle gate.
            # Use real GU clocks; no finished flag or RNG is injected into it.
            if motions.get(get(actor + 1204) & 65535) not in rest_requests:
                return
            before = len(timeline), len(rolls)
            delta = s.unpack("<f", cpu.mem_read(actor + 28, 4))[0]
            elapsed = action_time[0] - rest_started[0]
            while elapsed < 10.:
                run(0x951A80, actor)
                update()
                elapsed += delta
                if elapsed < 10.:
                    assert (len(timeline), len(rolls)) == before, ("early thought during idle", elapsed)
            assert len(timeline) > before[0] or any(
                "patrol:" in m or "waiting for motion" in m for m in messages[-2:]
            )

        def finish_blend():
            # Motion-specific tests work in authored frames, after the native
            # blend. Dedicated checks below measure the transition itself.
            for _ in range(160):
                if s.unpack("<f", cpu.mem_read(actor + 1268, 4))[0] <= 0.:
                    return
                run(0x951A80, actor)
                update()
            raise AssertionError(("blend never completed", get(actor+1188), s.unpack("<f", cpu.mem_read(actor+28,4)), s.unpack("<f", cpu.mem_read(actor+1268,4)), timeline[-3:]))

        def tick(done=False, *, blend=True):
            update(done)
            # These tests fast-forward a complete action, including its baked
            # replacement's explicit native follow-up motion.
            if done and timeline[-1][0] == 248:
                finish_blend()
                update(True)
            finish_rest()
            if blend:
                finish_blend()

        # A monster selects the bank; the LMT independently decides whether the clip exists.
        spawn_motion = next(m for m, request in motions.items() if request == (255, 39))
        slot_ptr = lmt_header + 8 + 4 * (spawn_motion & 255)
        for addr, size in ((actor + 240, 4), (actor + 3844 + 4, 4), (lmt_resource + 100, 4),
                           (status + 424, 1),
                           (lmt_header + 6, 2), (slot_ptr, 4)):
            saved = bytes(cpu.mem_read(addr, size))
            cpu.mem_write(addr, b"\4" if addr == status + 424 else bytes(size))
            tick()
            assert not played and cpu.mem_read(status + 417, 1) == b"\0"
            cpu.mem_write(addr, saved)

        # Same actor; an unavailable resource must not consume the first request.
        tick()
        tick(True)
        assert played == [(255, 39), (255, 34)]
        if independent:
            # AD7B4 watches the GU group/slot, which stays 1:0 throughout a hunt.
            # Fast-forward its timer to the last frame before recovery.
            scene, node = alloc(10816), alloc(16)
            prior_scene = get(0x1851fe0)
            put(0x1851fe0, scene)
            cpu.mem_write(scene + 10796, bytes([2]))
            cpu.mem_write(actor + 5120, bytes((1, 0)))
            cpu.mem_write(status + 416, bytes([2]))
            home = (1000., 50., 1000.)
            current_pos = (112., 50., -464.)
            cpu.mem_write(node, s.pack("<3f", *home))
            traps = [
                stub(0x88f524, lambda *_: 0), # surrounding scene service
                stub(get(get(actor) + 988), lambda *_: 0),
                stub(0x82d8c, lambda *_: 0),
                stub(0x5cf30, lambda *_: node),
            ]
            put(actor + 1188, 1)
            # Set up the sleep loop's last frame; use GU's real clock to wrap.
            cpu.mem_write(actor + 1276, s.pack("<f", 239.0))
            for i in range(4):
                run(0x951a80, actor)
                if i == 0:
                    tick()
                elif s.unpack("<f", cpu.mem_read(actor + 1280, 4))[0] < 5:
                    break
                else:
                    tick()
            assert i and s.unpack("<f", cpu.mem_read(actor + 1280, 4))[0] < 5
            for expected in (current_pos, home):
                cpu.mem_write(status + 64, s.pack("<3f", *current_pos))
                cpu.mem_write(actor + 5112, s.pack("<f", 7200.0))
                tick()  # first pass: wrapped, second pass: same frozen frame
                run(0xad7b4, actor)
                assert s.unpack("<3f", cpu.mem_read(status + 64, 12)) == expected
            for trap in traps:
                cpu.hook_del(trap)
            put(0x1851fe0, prior_scene)
            cpu.mem_write(status + 416, bytes(1))
            cpu.mem_write(status + 64, bytes(12))
        cpu.mem_write(pl + 72, s.pack("<f", 451.0))
        put(status + 880, 4900)
        assert run(hooks[0xBCC04], actor, 100) == 200
        for _ in range(120):
            tick(True)
        assert len(played) == 2 and refs[hts] == 0 and get(status + 1296) & 65535 == 0
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
        if independent:
            # Two complete 135-tick intervals, while the sleep animation is held.
            sound_mgr = alloc(16)
            put(0x1896F5C, sound_mgr)
            breaths = []
            audio_stub = stub(0x4EFC70, lambda manager, cue, owner, joint:
                              breaths.append((step, manager, cue, owner, joint)) or 0)
            for step in range(540):
                tick()
            assert len(breaths) == 2 and breaths[1][0] - breaths[0][0] == 270, breaths
            assert all((mgr, cue, owner) == (sound_mgr, 4096 + 87, actor) and 0 < joint < 255
                       for _, mgr, cue, owner, joint in breaths)
            put(0x1896F5C, 0)
            cpu.hook_del(audio_stub)
        put(status + 880, 4899)
        tick()
        assert run(hooks[0xBCC04], actor, 1) == 2
        tick(True)
        assert played[-2:] == [(255, 37), (255, 62)]
        assert get(status + 1296) & 65535 == 0
        # A GU perception command may re-enter 1:0 while the current FZ clip
        # is still playing. Let GU advance every frame through the end of motion 62.
        if independent:
            perceive(0x7E2A4, (1, 0), (5, 0))
            # Separate observation from selection: GU reset the primary target
            # (e.g. an intervening action), and hasn't redetected the hunter yet.
            run(0xBCB9C, actor, 0)
            run(0xC6E14, actor, 255, 0, 0, 0)
            assert cpu.mem_read(status + 560, 1) == b"\xff"
        put(actor + 1188, 1)
        for _ in range(200):
            run(0x951A80, actor)
            done = cpu.mem_read(actor + 1206, 1)[0] & 4
            run(dispatcher if independent else hooks[0xCF5F64], actor, 0)
            if done:
                break
            assert played[-1] == (255, 62) and cpu.mem_read(status + 417, 1) == b"\1"
        assert done
        assert timeline[-1] == (0, 1), ("missing post-wake idle", timeline[-3:])
        finish_rest()
        assert played[-1] == (3, 9)
        # Verify what GU sees, independently of the plugin's chosen animation.
        assert (run(0xC6BE4, actor, 1, 2, 0, 0), cpu.mem_read(status + 704, 1)[0]) == (1, 1), (
            bytes(cpu.mem_read(status + 560, 6)).hex(), cpu.mem_read(status + 704, 1)[0]
        )
        assert s.unpack("<3f", cpu.mem_read(status + 464, 12)) == (0., 0., 451.)
        for _ in range(20):
            tick()
        assert played.count((3, 9)) == 1
        if independent:
            # Awake hunter to the right at a nonzero map position. Execute GU's
            # timeline every frame, then transform the charge's own root track
            # into world movement. A fresh perception notification mid-turn must
            # not consume the pending charge or restart the turn animation.
            import math
            def bits(f):
                return s.unpack("<I", s.pack("<f", f))[0]
            def trig(fn):
                def inner(*_):
                    f = s.unpack("<f", s.pack("<I", cpu.reg_read(arm.UC_ARM_REG_S0)))[0]
                    cpu.reg_write(arm.UC_ARM_REG_S0, bits(fn(f)))
                return inner
            stub(0x13efde8, trig(math.sin))
            stub(0x13efdf4, trig(math.cos))
            start = (-857., -458., 512.)
            # The hunter starts behind the monster, then moves to its side
            # during the turn. The following charge must commit to the new spot.
            goal = (start[0] + 3600., start[1], start[2])
            cpu.mem_write(status + 64, s.pack("<3f", *start))
            cpu.mem_write(pl + 64, s.pack("<3f", start[0] + 2700, start[1], start[2] - 2700))
            cpu.mem_write(actor + 96, s.pack("<3f", 1, 1, 1))
            rng[0] = 1
            tick(True)
            assert played[-1] == (253, 6)
            before = len(played)
            for i in range(190):
                if i == 12:
                    cpu.mem_write(pl + 64, s.pack("<3f", *goal))
                    perceive(0x7E2A4, (1, 1), (5, 1))
                run(0x951A80, actor)
                run(dispatcher, actor, 0)
                if len(played) != before:
                    break
            assert played[before:] == [(3, 1)] and abs(16384 - get(status + 84)) < 744, (played[before:], get(status+84), timeline[-5:])
            assert tuple(s.unpack("<3f", cpu.mem_read(status + 464, 12))) == goal
            assert any("Espinas think:" in msg and "hunter=Some(0)" in msg for msg in messages)
            def bind_root_track():
                clip = s.unpack_from("<I", bank, 8 + 4 * (get(actor + 1204) & 255))[0]
                table, n = s.unpack_from("<II", bank, clip)
                tr = next(t for t in range(table, table + 36*n, 36) if bank[t+1:t+4] == bytes((4,0,255)))
                row = bytearray(bank[tr:tr+36])
                for off, size in ((12, s.unpack_from("<I", row, 8)[0]), (32, 32)):
                    src = s.unpack_from("<I", row, off)[0]
                    ptr = alloc(size)
                    cpu.mem_write(ptr, bank[src:src+size])
                    s.pack_into("<I", row, off, ptr)
                ptr = alloc(36)
                cpu.mem_write(ptr, bytes(row))
                run(0xafe6b4, actor + 1424, ptr)

            def move_frame():
                run(0x951A80, actor)
                run(0xc27cc, actor)
                cpu.reg_write(arm.UC_ARM_REG_S0, bits(0))
                cpu.reg_write(arm.UC_ARM_REG_S1, bits(1))
                run(0x72170, actor)
                run(0xc2844, actor)
                run(dispatcher, actor, 0)

            bind_root_track()
            prev = 3600.
            for i in range(220):
                move_frame()
                dist = math.dist(s.unpack("<3f", cpu.mem_read(status + 64, 12)), goal)
                assert prev - 55 < dist <= prev + 0.01, (i, prev, dist)
                prev = dist
                if played[-1] == (252, 108):
                    break
            assert played[-1] == (252, 108) and dist < 600, (played[-1], dist)
            print(f"Awake Espinas: GU detection -> right turn -> charge; hunter distance 3600 -> {dist:.0f}.")
            put(status + 84, 0)
            rng[0] = 30
        # Stay in one area at nonzero world coordinates. A type-10 target
        # with mask 4 means hunter 2 even when hunter 0 is closer.
        origin = (925.0, -458.0, 1460.0)
        pos = lambda p: cpu.mem_write(p, s.pack("<3f", *origin))
        pos(status + 64)
        put(actor + 4180, 4)
        other = base + 0x178000
        put(other, hunter_vt)
        cpu.mem_write(other + 4177, bytes([2]))
        put(players + 28 + 2 * 4, other)
        for ptr, distance in ((pl, 100), (other, 451)):
            pos(ptr + 64)
            cpu.mem_write(ptr + 72, s.pack("<f", origin[2] + distance))
            cpu.mem_write(ptr + 4180, bytes([4]))
        cpu.mem_write(status + 562, bytes([10]))
        cpu.mem_write(status + 564, bytes([4]))
        tick(True)
        assert played[-1] == (3, 9)  # far hunter chosen over nearby slot 0
        assert run(0xC6BE4, actor, 1, 10, 4, 0) == 1
        assert s.unpack("<3f", cpu.mem_read(status + 464, 12)) == (origin[0], origin[1], origin[2] + 451)
        cpu.mem_write(pl + 4180, bytes([3]))
        unavailable.add(2)  # still in the same area, but untargetable
        tick(True)
        assert played[-1] == (255, 1) and cpu.mem_read(status + 560, 1) == b"\xff"
        assert cpu.mem_read(status + 704, 1) == b"\0"
        last = len(played), len(rolls)
        for _ in range(120):
            tick()
        assert (len(played), len(rolls)) == last
        # Selected target is still absent. Reacquire the hunter in the same
        # area without waiting for the idle loop to finish, or moving the actor.
        cpu.mem_write(pl + 4180, bytes([4]))
        rng[0] = 5
        tick()
        assert played[-1] == (3, 12) and run(0xC6BE4, actor, 1, 2, 0, 0) == 1
        assert s.unpack("<3f", cpu.mem_read(status + 64, 12)) == origin
        unavailable.clear()
        cpu.mem_write(actor + 4180, bytes(1))
        cpu.mem_write(pl + 4180, bytes(1))
        cpu.mem_write(status + 562, bytes(3))
        put(players + 28 + 2 * 4, 0)
        cpu.mem_write(status + 64, bytes(12))
        cpu.mem_write(pl + 64, s.pack("<3f", 0, 0, 100))
        cpu.mem_write(status + 1304, b"\1")
        rng[0] = 8
        tick(True)
        assert played[-1] == (3, 15)
        cpu.mem_write(pl + 72, s.pack("<f", 451.0))
        rng[0] = 0
        tick(True)
        assert played[-1] == (3, 2)

        def run_charge(expected, frames):
            finish_blend()
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
            start = int(s.unpack("<f", cpu.mem_read(actor + 1280, 4))[0]) + 1
            yield from range(start, 75)
            while True:
                yield from range(24, 75)

        import itertools

        near = run_charge((252, 118), itertools.islice(charge_frames(), 150))
        assert 26 < near < 75 and len(submit()) == 3, (near, len(submit()), timeline[-4:])
        cpu.mem_write(pl + 64, s.pack("<f", 451.0))
        put(pl + 72, 0)
        tick(True)
        assert played[-1] == (253, 8), ("enraged right turn", played[-1])
        assert len(submit()) == 2
        draws = len(rolls)
        for frame in range(44):
            if frame == 8:
                cpu.mem_write(pl + 64, s.pack("<3f", 319.0, 0.0, 319.0))
            cpu.mem_write(actor + 1280, s.pack("<f", frame))
            tick()
        assert abs(8192 - get(status + 84)) < 744
        tick(True)
        assert played[-1] == (3, 2) and len(rolls) == draws and len(submit()) == 2
        assert run_charge((252, 118), itertools.islice(charge_frames(), 150)) == near
        # Stationary hunter behind-left. One quarter-turn cannot finish it:
        # let GU clock both clips, including the part beyond frame 44.
        put(status + 84, 0)
        cpu.mem_write(pl + 64, s.pack("<3f", -300., 0., -380.))
        tick(True)
        assert played[-1] == (253, 7)
        before = len(played)
        heading = round(math.atan2(-300., -380.) * 65536 / math.tau) & 65535
        late_turn = False
        for _ in range(140):
            run(0x951A80, actor)
            frame = s.unpack("<f", cpu.mem_read(actor + 1280, 4))[0]
            yaw = get(status + 84)
            run(dispatcher, actor, 0)
            late_turn |= frame > 44 and yaw != get(status + 84)
            if played[-1] == (3, 2):
                break
        assert late_turn and played[before:] == [(253, 7), (3, 2)], played[before:]
        assert abs(heading - get(status + 84)) < 683
        assert run_charge((252, 118), itertools.islice(charge_frames(), 150)) == near
        put(status + 84, 0)
        put(pl + 64, 0)
        cpu.mem_write(pl + 72, s.pack("<f", 450.0))
        rng[0] = 7
        tick(True)
        assert played[-1] == (3, 4) and len(submit()) == 7
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
            (20, (254, 9)),
            (20, (0, 9)),
            (23, (0, 11)),
            (25, (0, 13)),
        ]:
            rng[:] = [roll, 109 if move == (254, 9) else 3520] if roll == 20 else [roll]
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

        def choose(distance, roll, rage, expected, posture=0, dispatch_roll=None, *, blend=True):
            cpu.mem_write(status + 64, bytes(12))
            cpu.mem_write(pl + 64, s.pack("<3f", 0, 0, distance))
            put(status + 84, 0)
            cpu.mem_write(status + 1304, bytes([rage]))
            cpu.mem_write(status + 442, bytes([posture]))
            # GU +704 is alert/target state: a ground monster can see a hunter.
            cpu.mem_write(status + 704, bytes([1]))
            rng[:] = [roll] if dispatch_roll is None else [roll, dispatch_roll]
            tick(True, blend=blend)
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
                    expand = world_handlers[0] if independent else hooks[0x1696FC]
                    assert run(expand, world) == 1
                    assert s.unpack("<B3xf3f", cpu.mem_read(manager + 8240, 20)) == (
                        1,
                        400,
                        70,
                        140,
                        210,
                    )
                    if independent and i == 70:
                        group_size = get(groups + 4 * get(world + 136)) + 4
                        assert get(group_size) == 1
                        put(group_size, 2)
                        assert run(expand, world) == 0xFFFFFFFF
                        assert get(manager + 10288) == 1
                        put(group_size, 1)
                        put(manager + 10288, 64)
                        assert run(expand, world) == 0
                        assert get(manager + 10288) == 64
                        put(manager + 10288, 1)
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
        choose(100, 0, False, (1, 0), blend=False)
        cpu.mem_write(actor + 1280, s.pack("<f", 1.))
        update()
        assert played[-1] == (252, 1)
        finish_blend()
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
        # Area 4 in the reported quest. The generic stage marker is far
        # behind the hunter; do not use it as a species patrol destination.
        cpu.mem_write(node + 16, s.pack("<3f", 4430.76, 521.78, -2914.8))
        patrol_data, sets, route, slots, point = (alloc(n) for n in (120, 4, 28, 8, 48))
        put(actor + 30220, patrol_data)
        put(patrol_data + 104, 1)
        put(patrol_data + 116, sets)
        put(sets, route)
        cpu.mem_write(route + 4, bytes((11, 5, 121)))  # installed ARC has no 11/4/0
        cpu.mem_write(actor + 4179, bytes((11, 4)))
        cpu.mem_write(pl + 4180, bytes([4]))
        cpu.mem_write(status + 64, s.pack("<3f", 233, -351, 1004))
        cpu.mem_write(pl + 64, s.pack("<3f", -1047, -350, 1617))
        put(status + 84, 55170)
        run(0xC6E14, actor, 1, 2, 0, 0)
        rng[0] = 1  # 1420 units, calm, facing hunter: the logged patrol choice
        before = len(played)
        tick(True)
        assert len(played) == before, ("unmapped species patrol moved", played[before:])
        assert "Espinas patrol: area=4 skipped" in messages[-4:]
        rng[0] = 5  # next roll chooses a hunter-directed walk
        tick()
        assert played[before:] == [(1, 0)], played[before:]
        assert run(0xC6BE4, actor, 1, 2, 0, 0) == 1
        bind_root_track()
        def distance():
            return math.dist(s.unpack("<3f", cpu.mem_read(status + 64, 12))[::2], (-1047., 1617.))
        start_range = prev = distance()
        for _ in range(480):
            move_frame()
            here = distance()
            assert here <= prev + 0.01, (prev, here)
            prev = here
            if played[-1] == (252, 1):
                break
        assert played[-1] == (252, 1) and prev < 650
        print(f"Steppe area 4: skipped unbound patrol; walk {start_range:.0f} -> {prev:.0f}.")
        # A real typed route remains usable. Install a matching route with
        # a point already reached and a far point; query native 69E6F0.
        cpu.mem_write(route + 4, bytes((11, 4, 0)))
        put(route + 12, 2)
        put(route + 24, slots)
        put(slots, point)
        far_node = alloc(48)
        put(slots + 4, far_node)
        cpu.mem_write(far_node + 24, s.pack("<f", 550.))
        cpu.mem_write(point + 20, s.pack("<f", 1200.))  # same XZ, not a far point
        cpu.mem_write(status + 64, bytes(12))
        put(status + 84, 0)
        cpu.mem_write(pl + 64, s.pack("<3f", 0, 0, 5001))
        rng[0] = 8
        before = len(played)
        frame(2)
        assert len(played) == before and "Espinas patrol: area=4 skipped" in messages[-4:]
        rng[0] = 9  # same patrol choice, far point (slot 1)
        tick()
        assert played[-1] == (1, 1) and any("point=(0, 550)" in m for m in messages[-5:])
        tick(True)
        assert played[-1] == (1, 0)

        def clip():
            pos = s.unpack_from("<I", bank, 8 + 4 * (get(actor + 1204) & 255))[0]
            return s.unpack_from("<II", bank, pos + 8)

        def until(move, limit=1800):
            n, loop = clip()
            f = int(s.unpack("<f", cpu.mem_read(actor + 1280, 4))[0]) + 1
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
        for roll, variant, nm, turn in [(14, 42, 18, 123), (14, 41, 18, 120), (25, 101, 19, 119)]:
            choose(2701, roll, True, (3, nm), dispatch_roll=variant)
            draws = len(rolls)
            start = len(timeline)
            expected_chain = []
            for i in range(nm - 17):
                expected_chain.extend([(250, turn), (3, 19 if nm == 19 and i == 0 else 18)])
                until((250, turn))
                until((3, 19 if nm == 19 and i == 0 else 18))
            until((252, 118))
            assert len(rolls) == draws
            assert timeline[start:] == expected_chain + [(252, 118)], ("idle inserted inside chain", timeline[start:])
        shell_mgr, param, remap, joints = (
            base + v for v in (0xC9000, 0xCA000, 0xCB000, 0xCC000)
        )
        put(0x1892550, shell_mgr)
        def select_resource(variant, expected):
            if independent:
                return 413  # the GU setup constructor's unnumbered sentinel
            cpu.mem_write(actor + 46580, bytes([1, variant]))
            run(0xD0CA90, actor)
            assert get(actor + 51908) == expected
            return expected
        stub(0x48C388, lambda *_: 0)
        expected_resource = select_resource(0, 84)
        put(actor + 1176, remap)
        put(actor + 1172, joints)
        model = resource(r"enemy\em001\em001_00\mod\em001_00", 0x58A15856)
        bones = s.unpack_from("<I", model, 36)[0]
        nb = s.unpack_from("<H", model, 6)[0]
        from verify_espinas_package import check_opening_feet
        neutral = resource(r"enemy\em001\em001_00\mot\em001_00_0", 0x76820D81)
        check_opening_feet(model, bank, neutral, motions)
        first_skin = 0 if independent else nb - 48
        logical = model[bones + 24 * (first_skin + 34)]
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
            put(param + 4, 413)
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

        if independent:
            import zlib
            asset_mgr, vt, find, factory, shell_vt, init, owned = (
                base + 0xEA000 + 0x200*i for i in range(7)
            )
            put(0x2126A64, asset_mgr)
            put(asset_mgr, vt)
            put(vt+56, find)
            available = True
            def lookup(m, method, key, kind):
                path = br"shell\em\em107_00_shell00\em107_00_00"
                assert (m, method, key, kind) == (asset_mgr, find, ~zlib.crc32(path) & 0xffffffff, 0x4aa69872)
                return owned if available else 0
            stub(find, lookup)
            put(0x2126f48, asset_mgr)
            put(0x1892de8, vt)
            put(vt+8, factory)
            put(actor+12, 1)
            put(shell, shell_vt)
            put(shell_vt+316, init)
            stub(factory, lambda *_: shell)
            def initialize(obj, p, *_):
                assert (obj, get(shell + 4996)) == (shell, owned)
                return int(bool(create(shell_mgr, p)))
            stub(init, initialize)
            stub(0xc067ec, lambda *_: 0)
            stub(0x48c6c8, lambda *_: 0)
            assert get(shell_mgr + 28 + 84*4) == 0
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

        choose(1001, 14, True, (253, 2))
        tick(True)
        assert played[-1] == (3, 5)
        # Replay the emission frame with missing manager, sentinel slot and native rejection.
        sentinel = bytes([0xCD]) * 12
        cpu.mem_write(shell + 4112, sentinel)
        for mgr, slot, reject, constructions in [(0, 84, False, 0), (shell_mgr, 413, False, 0), (shell_mgr, 84, True, 1)]:
            put(0x1892550, mgr)
            if independent:
                available = slot != 413
            else:
                put(actor + 51908, slot)
            before = len(constructed)
            frame(79)
            frame(80)
            assert len(constructed) - before == constructions and not emitted
            assert bytes(cpu.mem_read(shell + 4112, 12)) == sentinel
        reject = False
        shot(80, 0, 0)
        def fly_to_landing(trigger, repeat):
            # Run the real GU clock, root movement and floor pass between NRO
            # updates. A posture-only stub hid the retail floor snap on takeoff.
            floor = -226.
            cpu.mem_write(status + 64, s.pack("<3f", -2617., floor, -239.))
            put(status + 1460, bits(floor))
            bind_root_track()
            before = len(emitted)
            height = floor
            peak = floor
            fast_steps = []
            wrapped = False
            shots = []
            last_f = 0
            takeoff = None
            rate = s.unpack("<f", cpu.mem_read(actor + 1292, 4))[0]
            assert math.isclose(rate, 1.3, rel_tol=1e-6)
            for t in range(1, 400):
                old_z = s.unpack("<f", cpu.mem_read(status + 72, 4))[0]
                prev_count = len(emitted)
                move_frame()
                run(0xBF340, actor)
                y, z = s.unpack("<2f", cpu.mem_read(status + 68, 8))
                # Also bounds displacement at the native clock's loop seam.
                assert abs(z-old_z) <= (58 if repeat == 70 else 102) * rate, (t, z-old_z)
                if played[-1] == (252, 57):
                    assert y == floor and cpu.mem_read(status + 442, 1) == b"\0"
                    break
                f = s.unpack("<f", cpu.mem_read(actor + 1280, 4))[0]
                if takeoff is None and f >= trigger:
                    takeoff = t
                if len(emitted) != prev_count:
                    shots.append((last_f, f))
                if t >= trigger + 12:
                    assert peak > floor + 25, ("GU pinned airborne height", t, y, peak)
                if f < last_f:
                    assert repeat <= f <= repeat + rate + 1e-4, (last_f, f, repeat)
                    wrapped = True
                if repeat == 70 and takeoff is not None and 10 <= t - takeoff <= 19:
                    fast_steps.append(z - old_z)
                assert abs(y-height) < 35 * rate, (t, y, height)
                height, peak, last_f = y, max(peak, y), f
            else:
                raise AssertionError(("never landed", played[-1], height))
            assert abs((peak - floor) - 596.7) < .2, peak  # Vita 81CE4E06 at the same rate
            if repeat == 70:
                assert wrapped, "motion 116 froze before touching the floor"
                assert len(fast_steps) == 10 and all(abs(dz + 75.2375)<0.02 for dz in fast_steps), fast_steps
                assert len(shots) == 1 and shots[0][0] < 50 <= shots[0][1] and emitted[before:] == [(0x1E94, 0)], (shots, emitted[before:])
            else:
                assert not emitted[before:]
            put(status + 1460, 0)
            print(f"Flight from {trigger}: peak {peak-floor:.1f}, landed after {t} GU frames.")

        for roll, id, last in [(25, 6, 115), (27, 7, 116)]:
            choose(451, roll, True, (3, id))
            shot(76, 0x16C1, -0x1332)
            # Resolve the resource for each emission.
            expected_resource = select_resource(0, 84) if independent else select_resource(2, 85)
            shot(122, 0x16C1, 0x1333)
            expected_resource = select_resource(0, 84)
            tick(True)
            assert played[-1] == (252, last)
            assert timeline[-2:] == [(3, id), (252, last)]
            if last == 115:
                shot(26, 0x1C72, 0)
            elif independent:
                fly_to_landing(36, 70)
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
        if independent:
            fly_to_landing(76, 116)
        else:
            until((252, 57))
            assert get(status + 68) == 0
        if independent:
            # The main action resumes after GU replaced the turn with a damage
            # motion. The opening of the old ground fireball is now obsolete.
            reaction_motion = next(m for m, req in motions.items() if req == (249, 60))
            shot_motion = next(m for m, req in motions.items() if req == (3, 5))
            shot_slot = lmt_header + 8 + 4 * (shot_motion & 255)
            for pending_resource in (False, True):
                choose(1001, 14, True, (253, 2))
                frame(3)
                if pending_resource:
                    # Even a queued playback retry belongs to the original turn.
                    saved_slot = get(shot_slot)
                    put(shot_slot, 0)
                    tick(True)
                    assert played[-1] == (253, 2)
                    put(shot_slot, saved_slot)
                cpu.mem_write(actor + 1204, s.pack("<H", reaction_motion))
                cpu.mem_write(pl + 72, s.pack("<f", 100))
                rng[0] = 8
                draws_before = len(rolls)
                tick(True)
                assert played[-1] == (3, 15) and len(rolls) == draws_before + 1, (
                    "interrupted turn replayed a stale attack", played[-1]
                )
            # Interrupt the backhop before its frame-50 shot. With the same
            # hunter still present, FZ selects a landing from the current posture.
            choose(451, 27, True, (3, 7))
            tick(True)
            assert played[-1] == (252, 116)
            frame(40)
            assert cpu.mem_read(status + 442, 1) == b"\1"
            cpu.mem_write(actor + 1204, s.pack("<H", reaction_motion))
            frame(50, True)
            assert played[-1] == (2, 1) and len(emitted) == 7
            until((252, 11))
            until((252, 19))
        for posture in (3, 1):
            choose(451, 0, False, (2, 1), posture=posture, blend=False)
            if independent:
                # Posture 3 has real Y at floor and a scaled display offset.
                # Higher free flight starts at physical Y. Apply GU's retail floor
                # pass to both, from the start of 15 through the end of 11.
                floor = -226.
                put(status + 1460, bits(floor))
                if posture == 3:
                    put(status + 68, bits(floor))
                    put(status + 428, bits(1.25))
                    put(status + 432, bits(1.2))
                    run(0xBF808, actor)
                    put(actor + 4996, 0)
                    run(0xBF828, actor)
                    height = floor + s.unpack("<f", cpu.mem_read(actor + 4996, 4))[0]
                    assert height == floor + 525.
                    put(actor + 4996, 0)
                else:
                    height = floor + 1500.
                    put(status + 68, bits(height))
                prev = height
                delta = 0.5 if posture == 1 else 1.
                put(actor + 28, bits(delta))
                final_start = None
                for i in range(700):
                    run(0x951A80, actor)
                    run(dispatcher, actor, 0)
                    run(0xBF340, actor)
                    y = s.unpack("<f", cpu.mem_read(status + 68, 4))[0]
                    assert floor <= y <= prev + 0.001, (posture, i, prev, y)
                    if y > floor and played[-1] == (2, 1):
                        assert 0 < prev - y <= 22 * delta, ("stepped descent", posture, i, prev, y)
                    if final_start is not None and y > floor + 0.01:
                        assert abs(prev - y - final_speed*delta) < 0.002, (posture, i, prev, y)
                    if i == 0:
                        assert cpu.mem_read(status + 442, 1) == b"\1" and y > floor, (
                            "hover height lost", posture, prev, y
                        )
                    if played[-1] == (252, 11) and final_start is None:
                        final_start = i
                        final_speed = (y-floor)/60.
                    if played[-1] == (252, 19):
                        assert final_start is not None and (i - final_start)*delta == 62
                        assert y == floor and cpu.mem_read(status + 442, 1) == b"\0"
                        break
                    prev = y
                else:
                    raise AssertionError(("descent did not finish", posture, played[-1], prev))
                put(actor + 28, bits(1.))
                put(status + 1460, 0)
            else:
                cpu.mem_write(status + 68, s.pack("<f", 400.0))
                until((252, 11))
                until((252, 19))
            assert len(emitted) == 7
        # Initialize GU's per-instance rows from the shipped DTT. The full
        # native DTT decoder is exercised in verify_espinas_quest.
        tune = resource(r"enemy\dt_tune\em001_00_dttune", 0x48E8AC29)
        res, calm, angry = (base + n for n in (0xD5000, 0xD5800, 0xD5900))
        put(actor + 30188, res)
        rage_params = alloc(124)
        cpu.mem_write(rage_params, tune[510:634])  # native 57740 validates this layout in the quest test
        put(res + 140, rage_params)
        put(res + 112, calm)
        put(res + 116, angry)
        at = 140 + tune[8]
        cpu.mem_write(calm, tune[at:at+80])
        cpu.mem_write(angry, tune[at+80:at+160])
        eighth = calm + 70
        assert 0xBAB60 not in hooks and get(0xBAB60) == 0xE3510008
        assert run(0xBABF4, actor) == eighth

        def hitzones(table, head):
            rows = [run(0xBAB60, actor, part) for part in range(8)]
            assert rows == [table + 10*i for i in range(7)] + [eighth]
            assert list(cpu.mem_read(rows[0], 3)) == head
            assert list(cpu.mem_read(rows[0] + 8, 2)) == [100, 0]

        hitzones(calm, [15, 20, 40])
        # Espinas only accumulates its FZ anger gauge while calm.
        gauge = get(status + 1296) & 0xFFFF
        cpu.mem_write(status + 1304, bytes([2]))
        assert run(hooks[0xBCC04], actor, 10) == gauge
        cpu.mem_write(status + 1304, b"\0")
        cpu.mem_write(status + 704, b"\0")
        # Feed resolved hits to GU's damage dispatcher (97DDC). GU subtracts
        # HP and calls BCC04 itself; the test never writes the ordinary rage mode.
        vt = get(actor)
        stub(get(vt + 864), lambda *_: 0)
        detour = hooks[0xBCE6C]
        put(status + 880, 2418)
        put(status + 884, 2475)
        part = alloc(80)
        put(res + 104, part)
        cpu.mem_write(part, s.pack("<h", 1000))
        cpu.mem_write(status + 958, s.pack("<h", 1000))
        cpu.mem_write(actor + 29728, b"\1")
        after_damage = stub(0x9A828, lambda *_: 0)  # reaction dispatch after accounting
        def damage(amount, hp, gauge):
            cpu.mem_write(status + 1160, s.pack("<h", amount))
            run(0x97DDC, actor)
            assert (get(status + 880), get(status + 1296) & 65535) == (hp, gauge)
        damage(36, 2382, 72)
        damage(43, 2339, 158)
        damage(8, 2331, 174)
        damage(100, 2231, 374)
        damage(108, 2123, 698)
        damage(1, 2122, 700)
        cpu.hook_del(after_damage)
        cpu.mem_write(status + 64, bytes(12))
        put(status + 84, 0)
        cpu.mem_write(pl + 64, s.pack("<3f", 0, 0, 1001))
        rng[0] = 14
        before = len(played), len(rolls)
        tick(True)
        assert played[before[0]:] == [(3, 3)] and get(status + 1304) & 255 == 1
        run(detour, actor, 1)  # reasserting the mode must not schedule another roar
        tick()
        assert played[before[0]:] == [(3, 3)] and len(rolls) == before[1]
        tick(True)
        assert played[-1] == (253, 2)
        tick(True)
        assert played[-1] == (3, 5)
        shot(80, 0, 0)
        cpu.mem_write(pl + 72, s.pack("<f", 2701))
        tick(True)
        assert played[-1] == (3, 18)
        until((250, 122))
        until((3, 18))
        until((252, 118))
        print("GU damage -> 174/700 at 2331 HP -> rage entry roar -> fireball -> two-leg charge and brake.")
        assert s.unpack("<f", cpu.mem_read(status + 1300, 4))[0] == 10800.0
        hitzones(angry, [70, 40, 30])
        assert run(hooks[0xBCC04], actor, 999) == 700

        # Examine the real GU playback clocks while the same attack is active.
        def rate_is(value):
            for p in (status + 908, actor + 1292, actor + 1612):
                assert math.isclose(s.unpack("<f", cpu.mem_read(p, 4))[0], value, rel_tol=1e-6), (hex(p), value)
        put(status + 880, 496)  # max 2475: 495 is exactly 20%
        tick()
        rate_is(1.3)
        put(status + 880, 495)
        tick()
        rate_is(1.5)
        run(hooks[0xBA874], actor, 0)
        tick()
        rate_is(1.5)
        rng[0] = 14
        # Deliver a real rage transition while the ordinary landing is in progress.
        put(actor + 30220, 0)  # tower: no area landing point
        choose(1001, 14, False, (2, 1), posture=1)
        rate_is(1.)
        cpu.mem_write(status + 68, s.pack("<f", 400.))
        run(detour, actor, 1)
        started, draws = len(played), len(rolls)
        tick(blend=False)
        assert played[started:] == [(2, 7)]
        rate_is(1.3)  # airborne group 2 retains the normal rage rate at low HP
        for _ in range(2):
            tick(blend=False)
        assert played[-1] == (2, 1)
        until((252, 11))
        until((252, 19))
        tick(True)
        assert played[started:] == [(2, 7), (2, 1), (252, 11), (252, 19), (3, 3)]
        assert not cpu.mem_read(status + 442, 1)[0] and len(rolls) == draws
        rate_is(1.5)
        run(detour, actor, 1)
        tick()
        assert played[started:].count((3, 3)) == 1
        tick(True)
        assert len(rolls) > draws and played[-1] != (3, 3)
        print("GU clocks: 1.3 normal rage, 1.5 low-HP attacks; airborne event lands and roars once.")

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
        hitzones(calm, [15, 20, 40])
        assert run(0x8172C, actor) == 0 and run(0x81774, actor) == 1
        assert not get(status + 866) & 16
        assert s.unpack("<f", cpu.mem_read(status + 1300, 4))[0] == 1234.0
        run(detour, actor, 1)
        hitzones(angry, [70, 40, 30])
        run(hooks[0xBA874], actor, 0)
        hitzones(calm, [15, 20, 40])
        assert cpu.mem_read(status + 1304, 1) == b"\0" and get(status + 1300) == 0
        # The thought gate is a logic clock, independent of animation speed,
        # HP, anger and GU update size. Execute every update without tick()'s
        # convenience wait and inspect the full motion timeline, including idles.
        for rage in (False, True):
            attack = (3, 15) if rage else (3, 12)
            idle = (0, 17) if rage else (0, 1)
            for delta in (0.5, 1., 2.):
                choose(100, 8 if rage else 5, rage, attack)
                put(actor + 28, bits(delta))
                before = len(timeline), len(rolls)
                update(True)
                assert timeline[before[0]:] == [idle] and len(rolls) == before[1]
                for _ in range(int(10 / delta) - 1):
                    run(0x951A80, actor)
                    update()
                    assert timeline[before[0]:] == [idle] and len(rolls) == before[1]
                run(0x951A80, actor)
                update()
                assert timeline[before[0]:] == [idle, attack] and len(rolls) == before[1] + 1
                put(actor + 28, bits(1.))

        # Unavailable idle playback must not start the gate. Once playback
        # succeeds it gets the full wait, even after many failed retries.
        choose(100, 8, True, (3, 15))
        idle_motion = next(m for m, req in motions.items() if req == (0, 17))
        idle_slot = lmt_header + 8 + 4 * (idle_motion & 255)
        saved_slot = get(idle_slot)
        put(idle_slot, 0)
        before = len(timeline), len(rolls)
        update(True)
        for _ in range(20):
            update()
        assert (len(timeline), len(rolls)) == before
        put(idle_slot, saved_slot)
        update()
        assert timeline[before[0]:] == [(0, 17)]
        finish_rest()
        assert timeline[before[0]:] == [(0, 17), (3, 15)]

        # A GU damage motion cancels the old gate and pending route. Returning
        # from it enters idle afresh, with all five logic updates still due.
        update(True)
        for _ in range(3):
            update()
        before = len(timeline), len(rolls)
        cpu.mem_write(actor + 1204, s.pack("<H", reaction_motion))
        update()
        assert timeline[before[0]:] == [(0, 17)] and len(rolls) == before[1]
        finish_rest()
        assert timeline[before[0]:] == [(0, 17), (3, 15)]
        print("GU idle: calm/rage gate matches FZ at 0.5/1/2 frame steps; retries and damage restart correctly; charge chains contain no idle.")

        # Compare real GU clock output against the native FZ blend regression.
        # Fireball 47 blends for six FZ ticks. Its incoming frame is held for
        # the first five, then the final tick advances by 2*1.3 = 2.6.
        put(status + 880, 2000)
        for delta in (0.5, 1., 2., 3.):
            choose(1001, 14, True, (253, 2))
            tick(True, blend=False)
            assert played[-1] == (3, 5)
            assert s.unpack("<f", cpu.mem_read(actor + 1268, 4))[0] == 12.
            put(actor + 28, bits(delta))
            before = len(emitted)
            elapsed = 0.
            for _ in range(int(12 / delta)):
                run(0x951A80, actor)
                run(dispatcher, actor, 0)
                elapsed += delta
                current = s.unpack("<f", cpu.mem_read(actor + 1276, 4))[0]
                assert abs(current - max(0., elapsed - 10.) * 1.3) < 0.0001, (delta, elapsed, current)
                assert len(emitted) == before
                rate_is(1.3)
            assert abs(current - 2.6) < .0001
            for _ in range(200):
                run(0x951A80, actor)
                run(dispatcher, actor, 0)
                if len(emitted) > before:
                    source_frame = s.unpack("<f", cpu.mem_read(actor + 1280, 4))[0]
                    assert 80 <= source_frame < 80 + 1.3 * delta + .001
                    break
            assert len(emitted) == before + 1
            for _ in range(2):
                run(0x951A80, actor)
                run(dispatcher, actor, 0)
            assert len(emitted) == before + 1
            put(actor + 28, bits(1.))

            choose(451, 0, True, (3, 2), blend=False)
            # Contacts exist in native storage but must not be submitted while
            # the incoming motion is held. All their clocks advance together.
            contacts = [anchor]
            # submit is empty until the clock releases; locate the active extras
            # from the already-installed list after the first active update.
            put(actor + 28, bits(delta))
            put(manager + 0x8000 + 104, bits(delta))
            elapsed = 0.
            for _ in range(int(12 / delta)):
                run(0x951A80, actor)
                run(dispatcher, actor, 0)
                elapsed += delta
                visible = submit()
                if elapsed <= 4.:
                    assert not visible
                else:
                    assert len(visible) == 2
                    contacts = visible
                for hit in contacts:
                    run(hooks[0x16919C], hit)
                expected = 26. - max(0., elapsed - 4.) * 1.3
                assert abs(s.unpack("<f", cpu.mem_read(anchor + 92, 4))[0] - expected) < .001
                rate_is(1.3)
            put(actor + 28, bits(1.))
            put(manager + 0x8000 + 104, bits(1.))
            until((252, 118))
        print("GU/FZ blend: fireball frame 2.6 at twelve GU frames; 0.5/1/2/3 updates, single shot and synchronized melee clocks passed.")

        # Both parts are complete actions in the source. Their native incoming
        # blends must execute, while the battle script must not think between them.
        for roll, request, followup, count in [(7, (3, 4), (248, 4), 5), (28, (3, 10), (248, 10), 1)]:
            choose(450 if request == (3, 4) else 5001, roll, request == (3, 4), request)
            start, draws = len(timeline), len(rolls)
            update(True)
            assert timeline[start:] == [followup] and len(rolls) == draws
            finish_blend()
            assert timeline[start:] == [followup] and len(submit()) == count
        print("Native follow-ups 125 -> 111 and 103 -> 104 keep their blends without an idle or RNG draw.")

        # The shared engine clock also accepts small non-monster actors. An
        # unmatched vtable must delegate before reading monster-only offsets.
        foreign = base + 0x300000
        cpu.mem_map(foreign, 0x2000)
        put(foreign, hunter_vt)
        put(foreign + 1188, 1)
        cpu.mem_write(foreign + 1204, s.pack("<HH", 1, 0x100))
        put(foreign + 1212, 4)
        for off, value in [(28, 1.), (1264, 12.), (1268, 12.), (1292, 1.), (1296, 1.), (1284, 100.)]:
            put(foreign + off, bits(value))
        run(0x951A80, foreign)
        assert s.unpack("<f", cpu.mem_read(foreign + 1276, 4))[0] == 1.
        assert s.unpack("<f", cpu.mem_read(foreign + 1268, 4))[0] == 11.
        cpu.mem_unmap(foreign, 0x2000)

        tail_name = r"enemy\em001\em001_00\mod\em001_00_tail".replace("em001", namespace)
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
        death_bank = resource(r"enemy\em001\em001_00\mot\em001_00_3", 0x76820D81)
        dead = s.unpack_from("<I", death_bank, 8 + 17 * 4)[0]
        track, n, frames, loop = s.unpack_from("<4I", death_bank, dead)
        assert frames > 1 and loop == 0xFFFFFFFF
        mapping = model[bones + 152 * nb : bones + 152 * nb + 256]
        assert all(
            bone == 255 or first_skin <= mapping[bone] < first_skin + 48
            for bone in (death_bank[track + 36 * i + 3] for i in range(n))
        )
        # Capture the exact program passed to GU's native damage handler.
        stub(0xA134C, lambda a, p, *_: p)
        cpu.mem_write(actor + 29664, bytes([10, 7]))
        rx = resource(r"enemy\em001\em001_00\mot\em001_00_4", 0x76820D81)
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
            previous = get(actor + 1204) & 65535
            for i, expected_op in enumerate(ops):
                op, operand = s.unpack("<2I", cpu.mem_read(program + 8 * i, 8))
                assert op == expected_op
                if op == 255:
                    assert operand == 0
                elif op == 3:
                    assert operand == 240
                else:
                    m, blend, start, flags, until, params = s.unpack("<If4I", cpu.mem_read(operand, 24))
                    half_blend = blends[m] or blend_hints.get(previous, 0)
                    assert m >> 8 == 4 and blend == (2 * (half_blend + 1) if half_blend else 0)
                    previous = m
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
