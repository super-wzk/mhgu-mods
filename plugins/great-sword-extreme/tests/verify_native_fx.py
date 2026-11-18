"""Focused checks against GU's ARM EFL loader, emitters and draw router."""
import math
import struct
from unicorn import arm_const as arm
from verify_style import Game, REPO, ROOT, NX, boot, hunters, DRAW_ROUTE


def resources(g):
    routes = (ROOT / "target/assets/effects/routes.bin").read_bytes()
    first, count = struct.unpack_from("<HH", routes, 9 * 4)
    assert count == 4
    output, random = g.alloc(16), g.alloc(16)
    g.cpu.mem_write(random, struct.pack("<4f", 0.5, 0.5, 0.5, 0))
    for layer in range(first, first + count):
        efl = (NX / f"effect/frontier_gs/e{layer}.efl").read_bytes()
        assert efl[:8] == b"EFL\0\x06\x03\x12\x20"
        data = efl[48:]
        common, model, life, move = struct.unpack_from("<4I", data)
        assert model & 255 == 5 and move & 255 == 0
        address = g.alloc(len(data))
        g.cpu.mem_write(address, data)
        model = address + (model >> 8)
        assert g.cpu.mem_read(model + 3, 1) == b"\1"
        scale, color = g.half(model + 306), g.half(model + 66)
        # Use GU's own interpolation and RGBA paths, including the native loop.
        for frame in (0, 1, 2, 3, 4, 30, 90, 120):
            g.run(0xAFB2BC, output, model + scale, frame, random, 0)
            x, y, z = struct.unpack("<3f", g.cpu.mem_read(output, 12))
            assert x == y and z == 1
            if layer - first < 2:
                assert 2 <= x <= 3
            else:
                assert abs(x - (2 + min(frame, 90) / 30)) < 0.001
            g.run(0xAF938C, output, model + color, frame, 128)
            rgba = g.get(output)
            assert rgba >> 24 <= 96
            if frame == 0:
                assert rgba >> 24 == 0
            if frame >= 90:
                assert rgba == (0x6040C0FF if layer - first < 2 else 0x602020E0)
    # Three ordinary-charge tiers and the uppercut's fourth tier each have
    # their own native flash, with no dormant opaque models in the idle aura.
    for level in range(1, 5):
        first, count = struct.unpack_from("<HH", routes, (9 + level) * 4)
        assert count == 2
        efl = (NX / f"effect/frontier_gs/e{first}.efl").read_bytes()[48:]
        model = struct.unpack_from("<I", efl, 4)[0] >> 8
        assert efl[model + 3] == 1
        scale = struct.unpack_from("<H", efl, model + 306)[0]
        ptr = g.alloc(len(efl))
        g.cpu.mem_write(ptr, efl)
        g.run(0xAFB2BC, output, ptr + model + scale, 0, random, 0)
        assert struct.unpack("<f", g.cpu.mem_read(output, 4))[0] == 6 * (0.5 + level / 4)
    print("GU native EFL curves: charge alpha, colour, pulse and all four tier flashes passed")


def emission(g):
    # Only host memory, resource lookup and math imports are supplied here.
    # The EFL loader, joint setup, emitter allocation and particle ticks are GU code.
    for addr in (0x13EFD7C, 0x13EFDAC, 0x13EFDC4):
        g.stub(addr, lambda dest, n, *_: g.cpu.mem_write(dest, bytes(n)) or dest)
    g.stub(0x13EFEFC, lambda dest, n, value, _: g.cpu.mem_write(dest, bytes([value & 255]) * n) or dest)
    for addr in (0x13EFE30, 0x13EFDD0):
        g.stub(addr, lambda dest, src, n, _: g.cpu.mem_write(dest, bytes(g.cpu.mem_read(src, n))) or dest)
    g.stub(0x887660, lambda *_: 0)
    g.stub(0x887648, lambda *_: 0)

    def math_call(fn):
        x = struct.unpack("<f", struct.pack("<I", g.cpu.reg_read(arm.UC_ARM_REG_S0)))[0]
        g.cpu.reg_write(arm.UC_ARM_REG_S0, struct.unpack("<I", struct.pack("<f", fn(x)))[0])

    g.stub(0x13EFDDC, lambda *_: math_call(math.sqrt))
    g.stub(0x13EFDE8, lambda *_: math_call(math.sin))
    g.stub(0x13EFDF4, lambda *_: math_call(math.cos))
    g.run(0x7C72E4)  # Native vector/quaternion constants and sine table.

    def obj(n):
        p = g.alloc(n)
        g.put(p, g.alloc(128))
        return p

    def slot(p, offset, fn):
        entry = g.alloc(4)
        g.put(g.get(p) + offset, entry)
        g.stub(entry, fn)

    allocator = obj(4)
    for offset in (28, 32):
        slot(allocator, offset, lambda owner, n, *_: g.alloc(n))
    slot(allocator, 52, lambda *_: 0)
    g.put(0x18A6308, allocator)
    g.stub(0x7AA47C, lambda *_: allocator)
    scratch = g.alloc(4096)
    g.stub(0xBA5770, lambda *_: 0)
    g.stub(0xB91F44, lambda *_: scratch)
    g.stub(0xB92480, lambda *_: 1)
    effects = obj(640)
    g.put(0x2126554, effects)
    slot(effects, 108, lambda *_: 1)
    model = obj(256)
    g.put(model + 80, 1)
    g.put(model + 116, g.alloc(48))
    g.put(model + 120, 1)
    context, cache, camera = g.alloc(24000), g.alloc(640), g.alloc(64)
    g.put(cache + 112, model)
    g.put(cache + 140, 1)
    g.stub(0xBAE520, lambda *_: cache)
    g.stub(0xA74880, lambda *_: 0)
    g.stub(0xA9AF40, lambda *_: 0)
    g.stub(0x885824, lambda *_: camera)
    normals = []
    submissions = []

    def submit(effect, ctx, mesh, matrix):
        normals.append(struct.unpack("<3f", g.cpu.mem_read(matrix + 32, 12)))
        sp = g.cpu.reg_read(arm.UC_ARM_REG_SP)
        uv = g.get(sp + 16)
        submissions.append((struct.unpack("<3f", g.cpu.mem_read(matrix + 48, 12)),
                            struct.unpack("<2f", g.cpu.mem_read(uv, 8))))
        return 0

    g.stub(0xC90380, submit)
    manager = obj(4)
    g.put(0x2126A64, manager)

    def load(owner, dti, name, flags):
        assert dti == 0x21251C8 and flags == 1
        path = NX / (g.string(name).replace("\\", "/") + ".mod")
        assert path.is_file(), path
        return model

    slot(manager, 48, load)
    stream = obj(4)
    data = b""
    slot(stream, 76, lambda *_: len(data))
    slot(stream, 52, lambda owner, dest, n, _: g.cpu.mem_write(dest, data[:n]) or n)
    routes = (ROOT / "target/assets/effects/routes.bin").read_bytes()
    charge, count = struct.unpack_from("<HH", routes, 36)
    flash, _ = struct.unpack_from("<HH", routes, 12 * 4)
    fourth, fourth_count = struct.unpack_from("<HH", routes, 4)
    third, _ = struct.unpack_from("<HH", routes, 8)
    release, _ = struct.unpack_from("<HH", routes, 28)
    ready, ready_count = struct.unpack_from("<HH", routes, 32)
    # 4006:1 alternates an upright streak with two rising mist layers, five times.
    offsets = [(-50, -100), (-150, 0), (-100, 140), (-50, 210), (0, 280)]
    assert fourth_count == 3 * len(offsets)
    uppercut_mesh = {fourth+i: (0, 77, 35)[i % 3] for i in range(fourth_count)}
    uppercut_mesh.update({third: 77, third + 1: 35})
    for path in sorted((NX / "effect/frontier_gs").glob("e*.efl")):
        data = path.read_bytes()
        lists, joints = struct.unpack_from("<HH", data, 16)
        assert lists == joints == 1
        # A declared joint must have an offset table AND a complete descriptor.
        # Without this, GU treats emitter flags as the joint pointer and can
        # report successful initialization with a corrupt/invisible transform.
        joint = struct.unpack_from("<I", data, 48 + lists * 16)[0]
        assert 16 * lists + 4 * joints <= joint <= len(data) - 48 - 112, path
        layer = int(path.stem[1:])
        following = ready <= layer < ready + ready_count
        depth = layer in (release + 2, release + 5)
        if not following and not depth and layer not in (0, charge, charge + 2, flash) and layer not in uppercut_mesh:
            continue
        resource = g.alloc(160)
        assert g.run(0xB5C5CC, resource, stream) == 1
        unit = g.alloc(528)
        g.run(0x9B8DF8, unit)
        g.put(unit + 244, resource)
        assert g.run(0x9BDA64, unit) == 1
        assert g.half(unit + 480) == g.half(unit + 484) == 1
        gen = g.get(unit + 496)
        native_joint = g.get(gen + 24)
        assert g.get(native_joint + 260) == g.get(resource + 104) + joint
        assert struct.unpack("<3f", g.cpu.mem_read(native_joint + 240, 12)) == (1, 1, 1)
        g.put(unit + 272, 1)
        fourth_wave = fourth <= layer < fourth + fourth_count
        delay = ((layer - fourth) // 3) * 4 if fourth_wave else 0
        if fourth_wave:
            # GU's root matrix at a quarter turn in a non-origin area.
            g.cpu.mem_write(unit + 288, struct.pack(
                "<16f", 0,0,-1,0, 0,1,0,0, 1,0,0,0, 1200,85,-700,1))
        visible = False
        for frame in range(91 if charge <= layer < charge + count else 30):
            g.run(0x9B9668, unit)
            if following:
                # Move the GU joint after the native particle has already been born.
                x = 180 if frame >= 8 else 0
                g.cpu.mem_write(native_joint + 48, struct.pack("<4f", x, 0, 0, 1))
            g.run(0xA5A48C, gen)
            g.run(0x9B975C, unit)
            g.run(0xA94C10, gen)
            depth_age = frame + 1 - (6 if layer == release + 5 else 0)
            if depth and depth_age in (6, 7, 8):
                g.cpu.mem_write(camera, struct.pack(
                    "<16f", 1,0,0,0, 0,1,0,0, 0,0,1,0, 0,0,1000,1))
                submissions.clear()
                g.run(0xA956D8, gen, context, 256)
                (position, _), = submissions
                assert math.dist(position, (0, 0, 62. * (depth_age - 6))) < 0.01, (
                    layer, depth_age, position)
            if (following and frame in (6, 9)) or (layer == release + 2 and frame in (3, 5)):
                x = 180 if following and frame >= 8 else 0
                g.cpu.mem_write(camera, struct.pack("<16f", 1,0,0,0, 0,1,0,0, 0,0,1,0, x,0,1000,1))
                submissions.clear()
                g.run(0xA956D8, gen, context, 256)
                (position, uv), = submissions
                if following:
                    assert abs(position[0] - x) < 0.01, (layer, frame, position)
                # Independent source lifetimes. GU evaluates age 1 on the first tick.
                life = g.get(g.get(resource + 104) + (struct.unpack_from("<I", data, 56)[0] >> 8) + 4)
                cols = math.ceil(math.sqrt(life + 1))
                rows = math.ceil((life + 1) / cols)
                age = frame + 1
                assert math.dist(uv, ((age % cols) / cols, (age // cols) / rows)) < 1e-6, (layer, frame, uv)
            if frame == delay + 4 and layer in uppercut_mesh:
                mesh = uppercut_mesh[layer]
                assert g.string(g.get(gen + 52) + 80) == rf"effect\frontier_gs\m{mesh}"
                for yaw, pitch in ((0, 0), (math.pi / 2, 0), (math.pi / 2, math.pi / 4)):
                    c, s = math.cos(yaw), math.sin(yaw)
                    a, b = math.cos(pitch), math.sin(pitch)
                    g.cpu.mem_write(camera, struct.pack(
                        "<16f", c, 0, -s, 0, s*b, a, c*b, 0, s*a, -b, c*a, 0, 0, 0, 0, 1,
                    ))
                    normals.clear()
                    submissions.clear()
                    g.run(0xA956D8, gen, context, 256)
                    normal, = normals
                    if fourth_wave:
                        local_x, local_z = offsets[(layer - fourth) // 3]
                        rise = 0 if mesh == 0 else 5 * 200 / 32
                        (world, _), = submissions
                        assert math.dist(world, (1200 + local_z, 85 + rise, -700 - local_x)) < 0.01, (layer, world)
                    expected = (s, 0, c) if mesh == 0 else (s*a, -b, c*a)
                    assert math.dist([v / math.hypot(*normal) for v in normal], expected) < 0.001
            scale = struct.unpack("<3f", g.cpu.mem_read(gen + 320, 12))
            assert all(math.isfinite(v) and v > 0 for v in scale)
            node = g.get(gen + 176)
            while node:
                alpha = g.get(node + 260) >> 24
                size = struct.unpack("<3f", g.cpu.mem_read(node + 224, 12))
                assert all(math.isfinite(v) for v in size)
                visible |= alpha > 0 and all(v > 0 for v in size)
                if charge <= layer < charge + count:
                    assert alpha <= 96
                node = g.get(node + 4)
        assert visible, path
    print("GU native EFL loading, emission and uppercut camera-facing transforms passed")


def draw(g):
    p, eq, other = hunters(g)
    g.stub(0x65F93C, lambda *_: 64)
    for group, action in ((1, 0), (2, 0), (2, 3)):
        g.cpu.mem_write(p + 9478, struct.pack("<3H", group, 0, action))
        g.cpu.mem_write(p + 10057, b"\0")
        g.cpu.mem_write(p + 5140, bytes(32))
        g.cpu.mem_write(p + 5108, struct.pack("<H", 0x160 if group == 2 else 0x120))
        g.put(p + 5188, 0x40)
        g.run(0x1160058, p)
        assert g.get(p + 5156) & 2
        # The real native lookup used by common movement, including the
        # plain-draw and guard-draw routes that never enter stock group 16.
        for kind in (0, 1, 3):
            result = g.run(DRAW_ROUTE, p, kind)
            assert struct.unpack("<3H", g.cpu.mem_read(result, 6)) == (16, 0x802F, 0)
        for kind in (2, 4):
            assert g.run(DRAW_ROUTE, p, kind) == g.run(g.originals[DRAW_ROUTE], p, kind)
        assert g.run(DRAW_ROUTE, p, 5) == g.run(g.originals[DRAW_ROUTE], p, 5)
    g.cpu.mem_write(p + 10057, b"\1")
    assert g.run(DRAW_ROUTE, p, 2) == g.run(g.originals[DRAW_ROUTE], p, 2)
    assert g.run(DRAW_ROUTE, other, 3) == g.run(g.originals[DRAW_ROUTE], other, 3)
    print("GU native X+A command and draw routes: idle, walking, sprinting and isolation passed")


if __name__ == "__main__":
    elf = (REPO / "mhgu/main.elf").read_bytes()
    resources(Game(elf))
    emission(boot(elf, b"[General]\nstyle=1\n"))
    draw(boot(elf, b"[General]\nstyle=1\n"))
