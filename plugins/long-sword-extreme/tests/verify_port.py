"""Replay FZ w07 release selection and the packaged GU style and blade-color hooks."""
import math
import struct
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
REPO = ROOT.parents[1]
sys.path.insert(0, str(REPO / "plugins/great-sword-extreme/tests"))
from verify_assets import Machine, MEM
from verify_style import DECODE, ENCODE, MANAGER, SUBMIT_MODEL, boot, hunters, load_registered_style, service, setup_controller
from unicorn import arm_const

NX = ROOT / "dist/long-sword-extreme/romfs/nativeNX"
DATA = (ROOT / "target/assets/long_sword.flsa").read_bytes()


def records(state, kind, fmt):
    count, offset = struct.unpack_from("<2I", DATA, 16 + 24 * state + 8 * kind)
    return list(struct.iter_unpack(fmt, DATA[offset:offset + count * struct.calcsize(fmt)]))


def release_motion_fx(fz, player):
    """Run the retail clip dispatcher against the exported w07 swing records."""
    timeline = (ROOT / "target/assets/effects/motion_fx.bin").read_bytes()
    rows = b"".join(
        timeline[i + 2:i + 18] for i in range(0, len(timeline), 18)
        if struct.unpack_from("<H", timeline, i)[0] == 1448
        and struct.unpack_from("<H", timeline, i + 4)[0] != 1
    )
    directory = MEM + 0x2b000
    fz.c.mem_write(directory, struct.pack("<IIHHI", 1, directory + 8, 1448,
                                          len(rows) // 16, directory + 16) + rows)
    calls = []
    def handler(kind):
        def record(_, bank, selector, joint):
            calls.append((kind, bank, selector, joint))
            return 0
        return record
    for kind in (0, 5, 6):
        pointer = int.from_bytes(fz.c.mem_read(0x82c368f8 + 4 * kind, 4), "little")
        fz.trap(pointer & ~1, handler(kind))
    expected = [(4, 6, 4023, 2, 10), (14, 0, 5, 10, 0), (14, 6, 4023, 9, 0),
                (28, 0, 5, 10, 0), (28, 5, 4035, 6, 0), (34, 6, 4023, 18, 10),
                (36, 6, 4023, 8, 10), (54, 0, 5, 10, 0), (54, 5, 4035, 2, 0),
                (56, 6, 4023, 8, 0), (60, 6, 4023, 18, 0)]
    for start in (0, 30, 52):
        last = start
        for now in sorted([start, start, 60, 61] + [e[0] for e in expected if e[0] >= start]):
            fz.c.mem_write(player + 544, struct.pack("<f", last))
            fz.c.mem_write(player + 476, struct.pack("<2f", now, now-last))
            calls.clear()
            fz.call(0x817bd900, player, 1448, directory)
            assert calls == [e[1:] for e in expected if last <= e[0] < now]
            last = now
    fz.put(player + 516, 1)
    fz.c.mem_write(player + 544, struct.pack("<f", 0))
    calls.clear()
    fz.call(0x817bd900, player, 1448, directory)
    assert not calls
    fz.put(player + 516, 0)
    for kind in (0, 5, 6):
        address = int.from_bytes(fz.c.mem_read(0x82c368f8 + 4 * kind, 4), "little") & ~1
        fz.c.hook_del(fz.hooks.pop(address))
    print("FZ 817BD900 release swing: all 11 records, tier entries, half-open interval and blend gate passed")


def dust_position(data, motion, frame, origin):
    angles, speed = (motion + offset for offset in struct.unpack_from("<HH", data, motion + 56))
    keys = struct.unpack_from("<I", data, angles)[0]
    turns = {time: (yaw, roll) for time, _, _, yaw, _, roll, _ in
             struct.iter_unpack("<I6f", data[angles + 4:angles + 4 + 28 * keys])}
    position = list(origin)
    for time in range(frame):
        if time in turns:
            yaw, roll = turns[time]
        velocity = struct.unpack_from("<f", data, speed + 8 + time * 12)[0]
        position[0] += velocity * math.cos(yaw) * math.cos(roll)
        position[1] += velocity * math.cos(yaw) * math.sin(roll)
        position[2] -= velocity * math.sin(yaw)
    return position


def dry_dust():
    routes = (ROOT / "target/assets/effects/routes.bin").read_bytes()
    first, count = struct.unpack_from("<HH", routes, 59 * 4)
    assert count == 14
    # Positions after six calls to FZ HD 113D2700 with the first three
    # wide definitions of 4035:2 and a zero facing angle.
    for index, expected_xz in enumerate(((0., -310.59375), (77.6484375, -232.9453125),
                                         (-77.6484375, -232.9453125))):
        data = (NX / f"effect/frontier_ls/e{first + index}.efl").read_bytes()[48:]
        model, motion = (struct.unpack_from("<I", data, at)[0] >> 8 for at in (4, 12))
        assert data[12] == 1 and data[model + 3] == 1  # Movement and BlendAlpha.
        joint, = struct.unpack_from("<I", data, 16)
        assert struct.unpack_from("<I", data, joint)[0] & 0xF000 == 0  # Inherit anchor yaw.
        assert struct.unpack_from("<I", data, model + 256)[0] & 0xF00 == 13 << 8
        scale = model + struct.unpack_from("<H", data, model + 306)[0]
        frame, size = struct.unpack_from("<If", data, scale + 4 + 6 * 28)
        assert frame == 12 and math.isclose(size, 7.2, abs_tol=1e-5)
        color = model + struct.unpack_from("<H", data, model + 66)[0]
        assert struct.unpack_from("<I4B", data, color + 16) == (12, 126, 118, 109, 124)

        position = dust_position(data, motion, 12, (0., 0., 0.))
        assert all(math.isclose(actual, expected, abs_tol=0.001)
                   for actual, expected in zip(position, (expected_xz[0], 18.51527, expected_xz[1])))

    # 4035:11 (instant slash) and 4035:13 (running charged slash).
    bank = (ROOT / "target/assets/effects/4035.bank").read_bytes()
    emitters, vectors = struct.unpack_from("<2H", bank, 2)
    start = 28 + emitters * 112
    curves = {}
    for x, y, z, frame, _, curve, _, _ in struct.iter_unpack(
            "<3fIHBBI", bank[start:start + vectors * 24]):
        curves.setdefault(curve, {})[frame] = (x, y - 20., z)
    for route, curve_ids, delays, life, alpha in (
            (72, (110, 113, 111, 114, 112, 115), (0, 0, 2, 2, 6, 6), 32, 255),
            (73, (116, 119, 117, 120, 118, 120), (0,) * 6, 24, 124)):
        first, count = struct.unpack_from("<HH", routes, route * 4)
        assert count == len(curve_ids)
        for index, (curve, delay) in enumerate(zip(curve_ids, delays)):
            data = (NX / f"effect/frontier_ls/e{first + index}.efl").read_bytes()[48:]
            model, envelope, motion, joint = (struct.unpack_from("<I", data, at)[0]
                                              for at in (4, 8, 12, 16))
            model, envelope, motion = model >> 8, envelope >> 8, motion >> 8
            assert data[model + 3] == 1
            assert struct.unpack_from("<I", data, joint)[0] & 0xF000 == 0
            assert struct.unpack_from("<I", data, model + 256)[0] & 0xF00 == 13 << 8
            assert struct.unpack_from("<I", data, envelope + 4)[0] == delay + life
            color = model + struct.unpack_from("<H", data, model + 66)[0]
            size = struct.unpack_from("<I", data, color)[0] & 255
            colors = {t: (r, g, b, a) for t, r, g, b, a, _ in struct.iter_unpack(
                "<I4BI", data[color + 4:color + 4 + size * 12])}
            assert colors[delay + 12 if route == 73 else delay + 14] == (126, 118, 109, alpha)
            origin = struct.unpack_from("<3f", data, joint + 32)
            for age in (0, 12, 24):
                actual = dust_position(data, motion, delay + age, origin)
                assert all(math.isclose(a, b, abs_tol=0.001)
                           for a, b in zip(actual, curves[curve][age])), (route, index, age, actual)
    print("FZ dry dust: directional puffs and both rings preserve travel, lifetime, tint and alpha")


def source():
    dry_dust()
    fz = Machine(REPO / "mhfz/eboot.bin.elf", True)
    def ptr(p):
        return int.from_bytes(fz.c.mem_read(p, 4), "little")
    name = "太刀".encode("cp932") + bytes(1)
    assert bytes(fz.c.mem_read(ptr(0x82CB0558 + 7 * 4), len(name))) == name
    path = b"motion" + bytes([92]) + b"w07.mot" + bytes(1)
    assert bytes(fz.c.mem_read(ptr(0x82BB30E8 + 4 * ptr(0x8258A2EC + 7 * 4)), len(path))) == path
    assert DATA[:8] == b"FLSA" + struct.pack("<2H", 2, 85)
    assert sum(bool(records(i, 0, "<6H")) for i in range(85)) == 64
    for sex in "mf":
        for group, count in ((0, 9), (2, 3), (4, 54)):
            data = (NX / f"player/mot/frontier_ls/{sex}_{group}.lmt").read_bytes()
            assert data[:4] == b"LMT" + bytes(1)
            slots = struct.unpack_from("<H", data, 6)[0]
            offsets = struct.unpack_from(f"<{slots}I", data, 8)
            source_slots = 60 if group == 4 else slots
            assert sum(bool(x) for x in offsets[:source_slots]) == count
            if group == 4:
                assert slots == 61 and offsets[60]
                # State 12 uses 1401 with 60 additional forward units. Other
                # callers of 1401 still play the original root track.
                original = struct.unpack_from("<3f", data, offsets[1] + 16)
                stepping = struct.unpack_from("<3f", data, offsets[60] + 16)
                assert original[:2] == stepping[:2] and abs(stepping[2] - original[2] - 60) < 0.001
                def root_at(clip, frame):
                    table, count = struct.unpack_from("<II", data, clip)
                    tr = next(t for t in range(table, table+36*count, 36) if data[t+3]==255)
                    keys, = struct.unpack_from("<I",data,tr+12)
                    bounds, = struct.unpack_from("<I",data,tr+32)
                    span, low = (struct.unpack_from("<f",data,bounds+i)[0] for i in (8,24))
                    return low + span * struct.unpack_from("<H",data,keys+8*frame+4)[0] / 65535
                for frame in range(36,59):
                    # Full samples at even FZ frames, interpolated at GU half ticks.
                    extra = min(60., max(0.,(frame-38)*3.75))
                    assert abs(root_at(offsets[60],frame)-root_at(offsets[1],frame)-extra)<0.025
                # FZ 811F9750 replay with the w07 root curve and ordinary travel.
                # Tick 42→44 is boosted, tick 44→46 uses only the authored motion.
                for frame, forward in ((28, 80.194), (44, 1376.312), (46, 1422.160)):
                    assert abs(root_at(offsets[51], frame) - forward) < 0.025, (sex, frame)

    player, ext, param = (MEM + x for x in (0x20000, 0x24000, 0x25000))
    release_motion_fx(fz, player)
    sprint = 0x825901F4 + 7 * 16
    clip, _, _, gate = struct.unpack("<4H", fz.c.mem_read(sprint, 8))
    assert (clip, gate) == (1409, 30)
    fz.c.mem_write(player + 820, b"\x01")
    for frame, blend, accepted in ((29.5, 0, 0), (30, 1, 0), (30, 0, 1)):
        fz.c.mem_write(player + 476, struct.pack("<f", frame))
        fz.put(player + 516, blend)
        fz.c.reg_write(arm_const.UC_ARM_REG_S0, struct.unpack("<I", struct.pack("<f", gate))[0])
        assert fz.call(0x817332DA, player, 0) == accepted
    fz.c.mem_write(player + 820, b"\0")
    # Standard reach, mhfdat directory +216, selector 0. Check the native
    # wrist transform and radius used by 816FE058 for a hunter's blade.
    rig, bones, shape, capsule = (MEM + i for i in (0x32000, 0x33000, 0x35000, 0x35100))
    fz.put(player + 1656, rig)
    fz.put(rig + 36, bones)
    fz.c.mem_write(player + 184, struct.pack("<f", 1))
    for x, z in ((0, 1), (1, 0)):
        fz.c.mem_write(bones + 384 * 14, struct.pack(
            "<16f", z, 0, -x, 0, 0, 1, 0, 0, x, 0, z, 0, 480, 90, -300, 1))
        for i, (a, b) in enumerate(((100, 220), (60, 100), (220, 260))):
            fz.c.mem_write(shape, struct.pack("<4HI7f", 14, 1, i, 0, 0, 52, 0, 0, a, 0, 0, b))
            assert fz.call(0x8183AAAE, player, shape, capsule, capsule + 32) == 1
            assert struct.unpack("<7f", fz.c.mem_read(capsule, 28)) == (
                480+x*a, 90, -300+z*a, 480+x*b, 90, -300+z*b, 52)
    fz.trap(0x8182E032, lambda *_: 3)
    fz.trap(0x81818BF8, lambda *_: ext)
    fz.trap(0x817C3E88, lambda *_: param)
    fz.trap(0x8184373A, lambda *_: 150)
    fz.trap(0x81818AAC, lambda *_: 900)
    fz.trap(0x8182BB4E, lambda *_: 0)
    fz.trap(0x8182B03C, lambda *_: 1)
    fz.trap(0x8182B4FE, lambda *_: 0)
    fz.trap(0x811F1A4A, lambda *_: 23)
    fz.trap(0x817C5C40, lambda *_: 0)
    out = []
    fz.trap(0x8182D1D8, lambda _, group, state, param: out.append((group,state,param)) or 0)
    fz.c.mem_write(param, DATA[-64:])
    for cut,expected in ((0,None),(1,76),(424,76),(425,83),(849,83),(850,84)):
        out.clear()
        fz.c.mem_write(ext+448, struct.pack("<h",cut))
        fz.call(0x811FA43E,player)
        assert out == ([] if expected is None else [(1,expected,12)]), (cut,out)
    fz.trap(0x811F1A4A, lambda _, press, execute, style: 25 if press == execute == 0x2010000 and style else 26)
    fz.c.mem_write(player + 18, b"\0")
    out.clear()
    fz.call(0x811FA176, player)
    assert out == [(1, 78, 4)], out
    fz.trap(0x811F1A4A, lambda *_: 1)
    out.clear()
    fz.call(0x811FA7A2, player)
    assert out == [(0, 28, 4)], out
    fz.c.hook_del(fz.hooks.pop(0x811F1A4A))
    fz.trap(0x8172C3D4, lambda *_: 0)
    fz.trap(0x817C45EC, lambda *_: 1)
    fz.trap(0x8172B7D8, lambda button, *_: int(button == 116))
    fz.trap(0x8172B7E8, lambda *_: 0)
    fz.trap(0x8184743A, lambda _, threshold, *__: int(threshold <= 100))
    for mask, expected in ((1 << 14, 14), (3 << 14, 15)):
        fz.c.mem_write(player + 3579, bytes([13]))
        assert fz.call(0x811F1A4A, player, mask, mask, 1) == expected
    def roll_angle(_, dst, *__):
        fz.c.mem_write(dst, struct.pack("<H", yaw))
        return 1
    fz.trap(0x8172C2D4, roll_angle)
    for yaw, expected in ((0, 28), (9101, 28), (9102, 49), (32768, 49),
                          (32769, 50), (56434, 50), (56435, 28)):
        out.clear()
        fz.call(0x811F238E, player, 85)
        assert out == [(0, expected, 4)], (yaw, out)
    for state, strikes in ((76,[264]),(83,[263,264]),(84,[250,263,264])):
        events = records(state,2,"<HBB4H")
        assert [r[-1] for r in events if r[-2] == 17] == strikes
        assert sum(r[-2] == 25 for r in events) == 1
    assert [(r[3],r[-1]) for r in records(78,2,"<HBB4H") if r[-2]==10] == [(0,3),(28,2),(30,1),(32,0)]
    assert [(r[3],r[-2],r[-1]) for r in records(77,2,"<HBB4H") if r[-2] in (6,17)] == [(0,6,22),(46,17,260)]
    # Run the family-108 update with the original threshold and node-matrix readers.
    effect, rig, node = (MEM + x for x in (0x26000, 0x27000, 0x28000))
    fz.put(effect + 28, ext)
    fz.put(effect + 172, player)
    fz.put(0x82C3B324, MEM + 0x29000)
    fz.put(player + 1656, rig)
    fz.put(rig + 36, node)
    fz.c.mem_write(player + 812, struct.pack("<H", 1451))
    fz.trap(0x817C3E88, lambda group, *_: param + (group - 173) * 32)
    fz.trap(0x81849B60, lambda *_: 0)
    for frame in range(26, 51, 2):
        fz.c.mem_write(player + 476, struct.pack("<f", frame))
        fz.c.mem_write(node + 48, struct.pack("<3f", *sweep_position(frame)))
        fz.call(0x8115B0A2, effect)
        assert fz.c.mem_read(effect + 9, 1) == bytes([int(frame >= 46)])
        assert fz.c.mem_read(effect + 104, 2) == struct.pack("<H", 2 if frame < 46 else 0)
        for offset, last in ((68, 30), (56, 42)):
            assert struct.unpack("<3f", fz.c.mem_read(effect + offset, 12)) == sweep_position(min(frame, last))
    # Execute the full-charge emission block itself. 8171A740 writes the
    # three VFP arguments that are absent from the decompiled call signature.
    flash, auras = [], []
    def capture_flash(desc, *_):
        assert struct.unpack("<5I", fz.c.mem_read(desc + 8, 20)) == (4023, 5, 14, 30, 7)
        flash.append(struct.unpack("<3f", fz.c.mem_read(ptr(desc + 28), 12)))

    def capture_aura(owner, _, kind, bone):
        assert (owner, kind, bone) == (player, 3, 14)
        offset = ptr(fz.c.reg_read(arm_const.UC_ARM_REG_SP))
        auras.append(struct.unpack("<3f", fz.c.mem_read(offset, 12)))

    fz.trap(0x81E6FD80, capture_flash)
    fz.trap(0x81159D80, capture_aura)
    fz.c.reg_write(arm_const.UC_ARM_REG_R6, player)
    fz.c.reg_write(arm_const.UC_ARM_REG_R11, MEM + 0x30000)
    fz.c.reg_write(arm_const.UC_ARM_REG_SP, MEM + 0x3FF000)
    fz.c.emu_start(0x811F9C8B, 0x811F9D86, count=300)
    assert flash == [(0., 0., 100.)] and auras == [(0., 0., z) for z in (100., 160., 220.)]
    print("FZ w07 release thresholds, contacts, parry, instant slash, dash charge anchors and six GU motion banks passed")



def release_mesh_frames(g):
    """Run GU's native vector sampler at every animated release model edge."""
    routes = list(struct.iter_unpack("<HH", (ROOT / "target/assets/effects/routes.bin").read_bytes()))
    sample, random = g.alloc(16), g.alloc(16)
    g.cpu.mem_write(random, struct.pack("<4f", .5, .5, .5, 0.))
    for route, authored_delays in ((25, [6]), (27, [8]), (29, [0, 8, 16, 22, 28])):
        first, count = routes[route]
        tracks = []
        for selector in range(first, first + count):
            raw = (NX / f"effect/frontier_ls/e{selector}.efl").read_bytes()[48:]
            model, life = (struct.unpack_from("<I", raw, at)[0] >> 8 for at in (4, 8))
            name = raw[model + 80:model + 128].split(b"\0")[0].decode()
            mesh = int(name.rsplit("m", 1)[-1])
            if not 403 <= mesh <= 420:
                continue
            if mesh == 403:
                tracks.append([])
            tracks[-1].append(mesh)
            delay = authored_delays[len(tracks) - 1]
            start = delay if mesh == 403 else delay + 8 + 2 * (mesh - 404)
            end = delay + (8 if mesh == 403 else min(42, 10 + 2 * (mesh - 404)))
            assert struct.unpack_from("<I", raw, life + 4)[0] == end
            if mesh == 403:
                continue
            base = g.alloc(len(raw))
            g.cpu.mem_write(base, raw)
            scale = base + model + g.half(base + model + 306)
            g.run(0xAFB2BC, sample, scale, start - 1, random, 0)
            assert struct.unpack("<3f", g.cpu.mem_read(sample, 12)) == (0., 0., 0.)
            g.run(0xAFB2BC, sample, scale, start, random, 0)
            assert all(x > 0. for x in struct.unpack("<3f", g.cpu.mem_read(sample, 12)))
        assert tracks == [list(range(403, 421))] * len(authored_delays)
    print("GU release follow-up: all seven mesh tracks switch 403–420 at their authored ages")


def dash_charge_effects(g, actor, controller, set_action):
    """Keep one set of attached effects per charge and stop it on every exit."""
    parent, manager, pel, handle = (g.alloc(128) for _ in range(4))
    for at in (240, 1172, 1176):
        g.put(actor + at, parent)
    g.stub(g.get(g.get(actor) + 304), lambda *_: parent)
    g.put(0x188A0D4, manager)
    g.put(0x2126F48, manager)
    resources, vtable = g.alloc(64), g.alloc(64)
    g.put(0x2126A64, resources)
    g.put(resources, vtable)
    load = g.alloc(4)
    g.put(vtable + 48, load)
    g.stub(load, lambda *_: pel)
    g.put(pel + 80, 1)
    g.stub(0x3268D8, lambda *_: handle)
    g.stub(0x326C78, lambda *_: 0)

    accepted = []
    fail_next_allocation = True

    def allocate(size, alignment, *_):
        nonlocal fail_next_allocation
        if fail_next_allocation:
            fail_next_allocation = False
            return 0
        return g.alloc(size, alignment)

    def spawn(_, unit, __, ___):
        selector = g.get(g.cpu.reg_read(arm_const.UC_ARM_REG_SP))
        assert g.get(unit + 320) == parent
        accepted.append((selector, unit))
        return 1

    g.stub(0x3288C4, allocate)
    g.stub(0x3267D0, spawn)
    for offset, value in ((28, 2.), (5088, 900.), (1284, 70.)):
        g.cpu.mem_write(actor + offset, struct.pack("<f", value))
    g.cpu.mem_write(actor + 5078, struct.pack("<h", 100))
    g.put(actor + 10112, 100)
    inputs = g.alloc(40)
    g.cpu.mem_write(inputs + 34, struct.pack("<h", 120))

    routes = list(struct.iter_unpack("<HH", (ROOT / "target/assets/effects/routes.bin").read_bytes()))
    aura, aura_count = routes[19]
    flash, flash_count = routes[20]
    expected = [aura] * (3 * aura_count) + list(range(flash, flash + flash_count))
    # 4023:4 mesh 84. Evaluate the emitted material keys with the GU engine,
    # including the settled alpha while the hunter continues running.
    efl = (NX / f"effect/frontier_ls/e{aura}.efl").read_bytes()[48:]
    base = g.alloc(len(efl))
    g.cpu.mem_write(base, efl)
    model = base + (g.get(base + 4) >> 8)
    assert g.string(model + 80) == r"effect\frontier_ls\m84"
    sampled, random = g.alloc(16), g.alloc(16)
    g.cpu.mem_write(random, struct.pack("<4f", .5, .5, .5, 0.))
    for age, size, alpha in ((2, .8, 255), (8, 1.12, 72), (24, 1.6, 72), (120, 1.6, 72)):
        g.run(0xAFB2BC, sampled, model + g.half(model + 306), age, random, 0)
        assert all(abs(s - size) < .001 for s in struct.unpack("<3f", g.cpu.mem_read(sampled, 12)))
        g.run(0xAF938C, sampled, model + g.half(model + 66), age, 128)
        assert g.get(sampled) >> 24 == alpha
    # 31D92C transforms the unit's local offset using the joint's current
    # matrix. Keep a renderable parent with a different root and hand position.
    pose, world, matrix, pose_vt, get_joint, zero = (g.alloc(n) for n in (300, 128, 64, 96, 4, 16))
    g.put(pose, pose_vt)
    g.put(pose + 12, 1)
    g.put(pose + 240, parent)
    g.put(pose_vt + 84, get_joint)
    mapped = False
    g.stub(0xC1628, lambda owner, *_: matrix if mapped else owner + 176)
    def joint(_, index, *__):
        assert index == 8
        return matrix
    g.stub(get_joint, joint)
    pel_data = (NX / "effect/frontier_ls/particles.pel").read_bytes()
    proofs = {}
    at = 16
    for _ in range(struct.unpack_from("<I", pel_data, 12)[0]):
        at += 1 + pel_data[at]
        at += 1 + pel_data[at]
        selector = struct.unpack_from("<H", pel_data, at + 48)[0]
        if selector in expected:
            proofs[selector] = g.alloc(160)
            g.cpu.mem_write(proofs[selector], pel_data[at:at + 160])
        at += 160
    for exit_kind in ("release", "evade", "hit", "unload"):
        accepted.clear()
        set_action(16, 0x8000 + 71)
        g.put(inputs, 1 << 27)
        mapped = False
        for _ in range(49):
            g.run(controller, actor, inputs)
        assert not accepted
        mapped = True
        for _ in range(2):
            g.run(controller, actor, inputs)
        assert sorted(selector for selector, _ in accepted) == expected, (
            exit_kind, g.half(actor + 9482), [selector for selector, _ in accepted]
        )
        offsets = [(selector, *struct.unpack("<3f", g.cpu.mem_read(unit + 144, 12)))
                   for selector, unit in accepted]
        assert sorted(offsets) == sorted([(aura, 0., 0., z) for z in (100., 160., 220.)]
                                          + [(s, 0., 0., 100.) for s in range(flash, flash + flash_count)])
        for selector, unit in accepted:
            assert g.get(unit + 132) == 0x29 and g.half(unit + 194) == 8
            assert g.cpu.mem_read(proofs[selector] + 56, 1) == (b"\x02" if selector == aura else b"\0")
            g.run(0x31CC6C, unit + 128, proofs[selector])
        for forward, origin in (((0, 0, 1), (80, 100, 50)), ((1, 0, 0), (440, 90, -30))):
            x, _, z = forward
            g.cpu.mem_write(matrix, struct.pack("<16f", z, 0, -x, 0, 0, 1, 0, 0, x, 0, z, 0, *origin, 1))
            for (_, unit), (_, _, _, distance) in zip(accepted, offsets):
                g.run(0x31D92C, world, unit, zero, unit + 144, zero, zero, zero, zero,
                      0, 1, 0, pose, unit + 80)
                assert struct.unpack("<3f", g.cpu.mem_read(world, 12)) == tuple(
                    base + axis * distance for base, axis in zip(origin, forward))
        for _ in range(30):
            g.run(controller, actor, inputs)
        assert sorted(selector for selector, _ in accepted) == expected

        # The engine may release an effect before the action ends. Its
        # allocator is then free to return this address to another effect.
        retired = accepted.pop()[1]
        g.run(g.hooks[0x32A104], retired)
        assert g.cpu.mem_read(retired + 48, 1) == b"\0"
        if exit_kind == "release":
            g.put(inputs, 0)
            g.run(controller, actor, inputs)
            assert g.half(actor + 9482) == 0x8000 + 72
        elif exit_kind == "evade":
            g.put(inputs, (1 << 27) | (1 << 3))
            g.run(controller, actor, inputs)
            assert (g.half(actor + 9478), g.half(actor + 9482)) == (1, 6)
        elif exit_kind == "hit":
            set_action(64, 1)
            g.run(controller, actor, inputs)
        else:
            g.run(controller, 0, 0)
        assert g.cpu.mem_read(retired + 48, 1) == b"\0"
        assert all(g.cpu.mem_read(unit + 48, 1) == b"\x02" for _, unit in accepted)

    print("GU sprint charge: three blade positions and flash on two poses, retry, no stacking and exit cleanup passed")


def sweep_position(frame):
    travel = frame - 30
    return 100. * travel, 60. + 2. * travel, 300. - 25. * travel


def instant_slash(g, actor, controller, set_action):
    """Run GU's list expansion and monster hurtbox intersection with the blade unavailable."""
    manager, clock = g.alloc(10400), g.alloc(112)
    g.put(0x1857830, manager)
    g.put(0x2126764, clock)
    g.cpu.mem_write(clock + 104, struct.pack("<f", 2.))
    for address in (0x276748, 0x5CEB4, 0x258FFC, 0x2B76CC, 0x887648, 0x887660):
        g.stub(address, lambda *_: 0)
    g.stub(0x13EFDC4, lambda dst, n, *_: g.cpu.mem_write(dst, bytes(n)))
    template = g.alloc(56)
    g.stub(g.originals[0x2A8A88], lambda owner, index, *_: template if (owner, index) == (actor, 1) else 0)

    data = (NX / "player/hit/frontier_ls/instant.hts").read_bytes()
    shape, rows, group, vtable, getter = (g.alloc(n) for n in (112, 4, 12, 72, 4))
    count, offset = struct.unpack_from("<2I", data, 16)
    assert count == 1
    row = g.alloc(len(data) - offset)
    g.cpu.mem_write(row, data[offset:])
    g.put(shape, vtable)
    g.put(shape + 80, 1)
    g.put(vtable + 68, getter)
    g.stub(getter, lambda _, index, *__: group if index == 0 else 0)
    g.put(group + 4, count)
    g.put(group + 8, rows)
    g.put(rows, row)
    mgr, vt, loader = (g.alloc(n) for n in (16, 64, 4))
    g.put(0x2126A64, mgr)
    g.put(mgr, vt)
    g.put(vt + 48, loader)
    g.stub(loader, lambda _, kind, name, flags: shape if
           (kind, g.string(name), flags) == (0x1857810, r"player\hit\frontier_ls\instant", 1) else 0)
    g.put(0x188A0D4, 0)

    # Keep the stock weapon bank present even when the private blade HTS is unavailable.
    g.put(actor + 12700, shape)

    monster, parts, body, vt = (g.alloc(n) for n in (47000, 88, 1500, 1024))
    g.put(monster, vt)
    g.put(monster + 5160, body)
    g.cpu.mem_write(parts + 44, b"\xff\xff")
    g.stub(0x710B0, lambda *_: parts)
    for offset in (152, 880, 1012):
        stub = g.alloc(4)
        g.put(vt + offset, stub)
        g.stub(stub, lambda *_: 0)
    contacts = []
    g.stub(0x170968, lambda _, hit, target, __: contacts.append((hit, target)))
    g.stub(g.get(g.get(actor) + 148), lambda *_: 0)
    inputs = g.alloc(40)

    # Read clip 1451's actual GU root track, including the baked FZ dash multiplier.
    lmt = (NX / "player/mot/frontier_ls/m_4.lmt").read_bytes()
    clip, = struct.unpack_from("<I", lmt, 8 + 51 * 4)
    tracks, count, clip_length = struct.unpack_from("<3I", lmt, clip)
    root, = [t for t in range(tracks, tracks + count * 36, 36) if lmt[t:t + 4] == bytes((4, 4, 0, 255))]
    keys, = struct.unpack_from("<I", lmt, root + 12)
    bounds, = struct.unpack_from("<I", lmt, root + 32)
    scales = struct.unpack_from("<4f", lmt, bounds)
    origin = struct.unpack_from("<4f", lmt, bounds + 16)

    def clip_position(frame):
        key = struct.unpack_from("<3H", lmt, keys + 8 * frame)
        return struct.unpack("<3f", struct.pack("<3f", *(origin[i] + scales[i] * key[i] / 65535 for i in range(3))))

    def begin():
        set_action(16, 0x8000 + 77)
        g.cpu.mem_write(actor + 1284, struct.pack("<f", clip_length))
        g.cpu.mem_write(actor + 64, struct.pack("<3f", *position(0)))
        g.cpu.mem_write(actor + 1280, struct.pack("<f", 0))
        g.run(controller, actor, inputs)
        assert g.half(actor + 1204) == 1451

    def update(frame):
        g.cpu.mem_write(actor + 1280, struct.pack("<f", frame))
        g.cpu.mem_write(actor + 64, struct.pack("<3f", *position(frame)))
        g.run(controller, actor, inputs)
        g.put(manager + 8224, 0)  # fresh native attack list
        g.run(g.hooks[0x16ABAC], manager)
        return g.get(manager + 8224)

    def intersects(hit, center):
        contacts.clear()
        g.cpu.mem_write(manager + 28, struct.pack("<B3x4f", 1, 5., *center))
        g.run(0x16C0C0, manager, hit, monster)
        return contacts == [(hit, monster)]

    for position, frames, dt in ((clip_position, range(0, 47, 2), 2),
                                 (clip_position, range(47), 1),
                                 (sweep_position, (0, 26, 28, 38, 46), 2)):
        g.cpu.mem_write(clock + 104, struct.pack("<f", dt))
        begin()
        for frame in frames:
            path = update(frame)
            if frame < 46:
                assert not path and g.get(manager + 10288) == 0
        assert path and path not in (actor + 12320, actor + 12504)
        assert g.get(manager + 10288) == 1 and g.get(path + 132) == shape
        assert struct.unpack("<B3x7f", g.cpu.mem_read(manager + 8240, 32)) == (
            2, 300., *position(30), *position(42))
        serial = g.half(path + 52)
        for frame in (30, 33, 36, 39, 42):
            assert intersects(path, position(frame)), frame
        x, y, z = position(36)
        assert not intersects(path, (x, y + 320, z))
        # The source's fixed segment stays put as the owner moves away.
        for frame in range(46 + dt, 70, dt):
            assert update(frame) == path and g.half(path + 52) == serial
            assert intersects(path, position(36))
        update(70)
        assert g.get(manager + 10288) == 0
        g.run(controller, 0, 0)

    begin()
    for frame in (26, 30, 42):
        assert update(frame) == 0
    g.cpu.mem_write(actor + 9478, struct.pack("<H2xH", 64, 1))
    assert update(46) == 0 and g.get(manager + 10288) == 0
    print("GU instant slash: independent collision list, swept hits, side miss, skipped frames, expiry and interruption passed")


def attack_rolls(g, actor, controller, set_action):
    # Use GU's native world stick angle with the hunter's camera override.
    g.cpu.hook_del(g.traps.pop(0x27F5D0))
    g.cpu.mem_write(actor + 11990, b"\x01")
    g.put(actor + 12016, 0x3000)
    facing = 0x7000
    inputs = g.alloc(40)
    resmgr, vtable, load = (g.alloc(size) for size in (16, 64, 4))
    g.put(0x2126A64, resmgr)
    g.put(resmgr, vtable)
    g.put(vtable + 48, load)

    def load_motion(_, kind, name, flags):
        if kind != 0x2125210:
            return 0
        rel = g.string(name).replace("\\", "/")
        assert rel in ("player/mot/frontier_ls/m_0", "player/mot/frontier_ls/f_0") and flags == 1
        bank = (NX / (rel + ".lmt")).read_bytes()
        count, = struct.unpack_from("<H", bank, 6)
        header, obj = g.alloc(8 + count * 4), g.alloc(104)
        g.cpu.mem_write(header, bank[:8])
        for slot in (6, 7):
            offset, = struct.unpack_from("<I", bank, 8 + slot * 4)
            clip = g.alloc(64)
            g.cpu.mem_write(clip, bank[offset:offset + 64])
            g.put(header + 8 + slot * 4, clip)
        g.put(obj + 80, 1)
        g.put(obj + 100, header)
        return obj

    g.stub(load, load_motion)
    for i, (yaw, strength, slot) in enumerate((
            (9102, 120, 7), (32768, 120, 7), (32769, 120, 6), (56434, 120, 6),
            (9101, 120, 28), (56435, 120, 28), (0x4000, 47, 28), (0, 120, 28))):
        # Cycle between the vertical slash (80) and thrust (38) cancel windows.
        state, opening = (0, 80) if i % 2 == 0 else (4, 38)
        g.cpu.mem_write(g.get(actor + 5060) + 1240, bytes([i % 2]))
        set_action(16, 0x8000 + state)
        g.put(actor + 1280, 0)
        g.cpu.mem_write(actor + 1284, struct.pack("<f", 160.))
        g.put(inputs, 0)
        g.run(controller, actor, inputs)
        g.put(actor + 4076, facing)
        g.put(actor + 5280, 0x1100)  # the body and target facing can differ
        stick = (facing - yaw - 0x3000 + 0x4000) & 0xffff
        g.cpu.mem_write(actor + 5132, struct.pack("<H", stick))
        g.cpu.mem_write(inputs + 32, struct.pack("<Hh", stick, strength))
        g.put(inputs, 1 << 3)
        for frame in (opening - 1, opening):
            g.cpu.mem_write(actor + 1280, struct.pack("<f", frame))
            g.run(controller, actor, inputs)
            assert g.half(actor + 9482) == (0x8000 + state if frame < opening else 6)
        g.put(inputs, 0)
        g.cpu.mem_write(inputs + 34, bytes(2))
        g.put(actor + 5132, 0)
        g.put(actor + 4076, 0)
        g.run(controller, actor, inputs)  # native roll tick after the stick returned to neutral
        packed = g.run(ENCODE, actor, 28)
        assert packed & 255 == slot, (state, yaw, strength, hex(packed), slot)
        assert g.run(ENCODE, actor, 28) == packed
        if slot != 28:
            assert g.run(DECODE, actor, packed) == {6: 5012, 7: 5013}[slot]
            resource = g.get(actor + 3844 + 4 * (packed >> 8))
            clip = g.run(0xB49E54, resource, slot)
            dx, _, dz = struct.unpack("<3f", g.cpu.mem_read(clip + 16, 12))
            assert (dx > 350 if slot == 6 else dx < -350) and abs(dz) < 1
        set_action(1, 0)
        g.run(controller, actor, inputs)
        set_action(1, 6)
        assert g.run(ENCODE, actor, 28) == 28
    print("GU attack cancels: FZ left/right sectors, forward roll, neutral stick, signed travel and roll cleanup passed")


def startup_camera(elf):
    # All three styles loaded, before a hunter exists or any style is selected.
    g = boot(elf, b"[Weapons]\n", ("registry", "sword"))
    submit = g.hooks[SUBMIT_MODEL]
    create = g.hook

    def hook(ret, addr, detour, saved):
        result = create(ret, addr, detour, saved)
        if addr in (0x1C334, 0x898AC0):
            # Forge ARM32 copies the two displaced words verbatim, then jumps back.
            jump = 0xE51FF004
            trampoline = g.alloc(16)
            g.cpu.mem_write(trampoline, bytes(g.cpu.mem_read(addr, 8)) + struct.pack("<II", jump, addr + 8))
            g.put(saved, trampoline)
            g.cpu.mem_write(addr, struct.pack("<II", jump, detour))
        return result

    g.hook = hook
    for base, path in (
        (0x9300000, ROOT.parent / "dual-blades-extreme/dist/dual-blades-extreme/romfs/nativeNX/plugins/dual_blades_extreme.nro"),
        (0x9400000, NX / "plugins/long_sword_extreme.nro"),
    ):
        exports = g.load(path, base)
        for entry in ("forge_onLoad", "forge_onInit"):
            g.run(exports[entry], MANAGER)
        assert g.hooks[SUBMIT_MODEL] == submit

    view = g.alloc(160)
    vtable = 0x1730C28
    g.put(view, vtable)
    snapshots = []
    g.stub(0xB8B5F0, lambda _, camera, *__: snapshots.append(camera))
    for slot in (72, 76):
        g.stub(g.get(vtable + slot), lambda matrix, *_: g.cpu.mem_write(matrix, bytes(64)))
    # The startup unit dispatcher calls virtual +40. Exercise the actual thunk,
    # detour, trampoline, and renderer entry, including the return to the caller.
    g.run(g.get(vtable + 40), view)
    assert snapshots == [view]
    assert g.get(0x1C338) == 0xE12FFF1E  # Separate virtual method: bx lr.
    g.run(g.get(vtable + 48), view)
    print("GU startup camera: all three styles, native snapshot and adjacent virtual method passed")


def plugin(elf, selected):
    g = boot(elf, b"[Weapons]\n7=3\n" if selected else b"[Weapons]\n", ("registry",))
    _, selection = service(g)
    d = load_registered_style(g, NX / "plugins/long_sword_extreme.nro")
    assert g.get(d)==3 and g.get(d+4)==7 and bytes(g.cpu.mem_read(d+16,2))==bytes((1,3))
    assert g.run(selection,7)==(3 if selected else 0)
    a,eq,remote = hunters(g)
    g.put(a,0x174cf64)
    g.cpu.mem_write(eq+1236,bytes([7]))
    timer = struct.unpack("<I", struct.pack("<f", 600.))[0]
    g.put(a + 13108, timer)
    g.put(a + 13120, timer)
    g.put(a + 10336, 0x40)
    g.run(g.hooks[0x1198AB4], a)
    assert g.get(a + 10336) == (0x40 if selected else 0x240)
    assert g.get(a + 10340) & 0x100  # Preserve the other native timer's flag.
    assert g.get(a + 13108) == g.get(a + 13120) == timer
    for at in (10336, 10340, 13108, 13120):
        g.put(a + at, 0)
    for level in (1,2,3):
        g.run(g.hooks[0x119ed8c],a,level)
        assert g.get(a+13104)==(0 if selected else level)
        g.run(g.hooks[0x119ed8c],remote,level)
        assert g.get(remote+13104)==level
        if selected:
            g.put(a+13100,0x42800000)
            g.run(g.hooks[0x119ed98],a,1)
            assert g.get(a+13104)==g.get(a+13100)==0
    if selected:
        g.cpu.mem_write(a+9478,struct.pack("<3H",16,0,0x8000+78))
        out=g.alloc(12)
        for bit,expect in ((0,1),(16,2),(0,1)):
            g.put(a+9488,bit)
            assert g.run(g.get(d+60),a,out)==expect
        assert bytes(g.cpu.mem_read(out,12))==struct.pack("<fII",8.,0,40)
        g.cpu.mem_write(a+9482,struct.pack("<H",0x8000+77))
        assert g.run(g.get(d+60),a,out)==0
        transition_allowed = True

        def allow_transition(allowed):
            nonlocal transition_allowed
            transition_allowed = allowed

        set_action = setup_controller(g, a, accept_transition=lambda: transition_allowed)
        release_mesh_frames(g)
        dash_charge_effects(g, a, g.get(d + 20), set_action)
        instant_slash(g, a, g.get(d + 20), set_action)
        attack_rolls(g, a, g.get(d + 20), set_action)
        from verify_fidelity import replay
        replay(g, a, d, set_action, allow_transition)
    print("GU packaged NRO registration, three art slots, guard policy and blade-color writes passed", selected)


if __name__ == "__main__":
    source()
    elf=(REPO/"mhgu/main.elf").read_bytes()
    startup_camera(elf)
    plugin(elf,True)
    plugin(elf,False)
