"""Focused checks against GU's ARM EFL loader, emitters and draw router."""
import math
import struct
from typing import NamedTuple
from unicorn import UC_HOOK_CODE, arm_const as arm
from verify_style import Game, REPO, NX, boot, effect_routes, hunters, DRAW_ROUTE, SUBMIT_MODEL


def resources(g):
    routes = effect_routes()
    aura = routes[9]
    assert len(aura) == 4
    output, random = g.alloc(16), g.alloc(16)
    g.cpu.mem_write(random, struct.pack("<4f", 0.5, 0.5, 0.5, 0))
    for index, layer in enumerate(aura):
        efl = (NX / f"effect/frontier_gs/e{layer}.efl").read_bytes()
        assert efl[:8] == b"EFL\0\x06\x03\x12\x20"
        data = efl[48:]
        common, model, life, move = struct.unpack_from("<4I", data)
        assert model & 255 == 5 and move & 255 == 0
        address = g.alloc(len(data))
        g.cpu.mem_write(address, data)
        model = address + (model >> 8)
        assert g.cpu.mem_read(model + 3, 1) == bytes([0 if index < 2 else 1])
        scale, color = g.half(model + 306), g.half(model + 66)
        # Use GU's own interpolation and RGBA paths, including the native loop.
        for frame in (0, 1, 2, 3, 4, 30, 90, 120):
            g.run(0xAFB2BC, output, model + scale, frame, random, 0)
            x, y, z = struct.unpack("<3f", g.cpu.mem_read(output, 12))
            assert x == y and z == 1
            if index < 2:
                assert 2 <= x <= 3
            else:
                assert abs(x - (2 + min(frame, 90) / 30)) < 0.001
            g.run(0xAF938C, output, model + color, frame, 128)
            rgba = g.get(output)
            assert rgba >> 24 <= 96
            if frame == 0:
                assert rgba >> 24 == 0
            if frame >= 90:
                assert rgba == (0x6040C0FF if index < 2 else 0x602020E0)
    # Three ordinary-charge tiers and the uppercut's fourth tier each have
    # their own native flash, with no dormant opaque models in the idle aura.
    for level in range(1, 5):
        flash = routes[9 + level]
        assert len(flash) == 2
        efl = (NX / f"effect/frontier_gs/e{flash[0]}.efl").read_bytes()[48:]
        model = struct.unpack_from("<I", efl, 4)[0] >> 8
        assert efl[model + 3] == 0
        scale = struct.unpack_from("<H", efl, model + 306)[0]
        ptr = g.alloc(len(efl))
        g.cpu.mem_write(ptr, efl)
        g.run(0xAFB2BC, output, ptr + model + scale, 0, random, 0)
        assert struct.unpack("<f", g.cpu.mem_read(output, 4))[0] == 6 * (0.5 + level / 4)
    print("GU native EFL curves: charge alpha, colour, pulse and all four tier flashes passed")


def binding_stack(g, samples=((4, b"effect\\frontier_gs\\m2"), (5, b"effect\\frontier_db\\m2"))):
    draw = 0xA956D8
    ctx, gen, params = g.alloc(0x308E8), g.alloc(192), g.alloc(320)
    g.cpu.mem_write(gen + 70, b"\x05")
    g.put(gen + 52, params)
    g.put(ctx + 4, 4096)
    size = 344 + 4096 * 8
    bindings = bytes(range(251)) * (size // 251) + bytes(range(size % 251))
    g.cpu.mem_write(ctx + 0xAC, bindings)
    for addr in (0x13EFE30, 0x13EFDD0):
        g.stub(addr, lambda dest, src, n, _: g.cpu.mem_write(dest, bytes(g.cpu.mem_read(src, n))) or dest)

    def packet(*_):
        g.put(ctx + 0xAC, 55)
        g.put(ctx + 0xAC + size - 4, 66)
        g.put(ctx + 64, g.get(ctx + 64) + 4)
        return 9

    g.stub(g.originals[draw], packet)
    for depth, model in samples:
        g.cpu.mem_write(params + 80, model + b"\0")
        g.put(ctx + 0x308E4, depth)
        active = bytes(g.cpu.mem_read(ctx + 0xAC + size, depth * size))
        cursor = g.get(ctx + 64)
        assert g.run(draw, gen, ctx, 256) == 9
        assert g.get(ctx + 0x308E4) == depth and g.get(ctx + 64) == cursor + 4
        assert bytes(g.cpu.mem_read(ctx + 0xAC, size)) == bindings, (depth, model)
        assert bytes(g.cpu.mem_read(ctx + 0xAC + size, depth * size)) == active


class ModelDraw(NamedTuple):
    position: tuple[float, float, float]
    uv: tuple[float, float]
    normal: tuple[float, float, float]
    basis_x: tuple[float, float, float]
    rgb: tuple[int, int, int]
    alpha: int
    flags: int


def model_renderer(g, nx, depth_scene=False):
    # Only host memory, resource lookup and math imports are supplied here.
    # The EFL loader, joint setup, emitter allocation and particle ticks are GU code.
    for addr in (0x13EFD7C, 0x13EFDAC, 0x13EFDC4):
        g.stub(addr, lambda dest, n, *_: g.cpu.mem_write(dest, bytes(n)) or dest)
    g.stub(0x13EFEFC, lambda dest, n, value, _: g.cpu.mem_write(dest, bytes([value & 255]) * n) or dest)
    for addr in (0x13EFE30, 0x13EFDD0, 0x13EFFF8):
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
    # The shared draw hook uses GU's five binding snapshots and depth word.
    context, cache, camera = g.alloc(0x308E8), g.alloc(640), g.alloc(64)
    if depth_scene:
        view = g.alloc(0x100)
        g.put(context + 0xD7C, view)
        camera = view + 0xB0
        for address, z in ((view + 0x70, -1000), (camera, 1000)):
            g.cpu.mem_write(address, struct.pack(
                "<16f", 1,0,0,0, 0,1,0,0, 0,0,1,0, 0,0,z,1))
    g.put(cache + 112, model)
    g.put(cache + 140, 1)
    g.stub(0xBAE520, lambda *_: cache)
    g.stub(0xA74880, lambda *_: 0)
    g.stub(0xA9AF40, lambda *_: 0)
    g.stub(0x885824, lambda *_: camera)
    submissions = []

    def submit(effect, ctx, mesh, matrix):
        sp = g.cpu.reg_read(arm.UC_ARM_REG_SP)
        uv = g.get(sp + 16)
        red, green, blue, alpha = g.cpu.mem_read(g.get(sp), 4)
        submissions.append(ModelDraw(
            position=struct.unpack("<3f", g.cpu.mem_read(matrix + 48, 12)),
            uv=struct.unpack("<2f", g.cpu.mem_read(uv, 8)),
            normal=struct.unpack("<3f", g.cpu.mem_read(matrix + 32, 12)),
            basis_x=struct.unpack("<3f", g.cpu.mem_read(matrix, 12)),
            rgb=(red, green, blue),
            alpha=alpha,
            flags=g.get(g.get(sp + 8) + 4),
        ))
        return 0

    g.stub(getattr(g, "originals", {}).get(SUBMIT_MODEL, SUBMIT_MODEL), submit)
    manager = obj(4)
    g.put(0x2126A64, manager)

    def load(owner, dti, name, flags):
        assert dti == 0x21251C8 and flags == 1
        path = nx / (g.string(name).replace("\\", "/") + ".mod")
        assert path.is_file(), path
        return model

    slot(manager, 48, load)
    stream = obj(4)
    data = b""
    slot(stream, 76, lambda *_: len(data))
    slot(stream, 52, lambda owner, dest, n, _: g.cpu.mem_write(dest, data[:n]) or n)

    def load_efl(raw):
        nonlocal data
        data = raw
        resource = g.alloc(160)
        assert g.run(0xB5C5CC, resource, stream) == 1
        return resource

    def draw(generator):
        submissions.clear()
        g.run(0xA956D8, generator, context, 256)
        return tuple(submissions)

    return load_efl, camera, draw


def emission(g):
    load_efl, camera, draw = model_renderer(g, NX)
    routes = effect_routes()
    fourth, third = routes[1], routes[2]
    release, ready, charge = routes[7:10]
    depth_layers = {release[2], release[5]}
    impact_layers = {routes[i][0] for i in range(14, 19)}
    counter_layers = set().union(routes[19], routes[20], routes[21])
    blade_layers = routes[23]
    sprint_and_guard = set().union(*routes[32:36])
    debris = routes[17][0]
    # 4006:1 alternates an upright streak with two rising mist layers, five times.
    offsets = [(-50, -100), (-150, 0), (-100, 140), (-50, 210), (0, 280)]
    assert len(fourth) == 3 * len(offsets)
    uppercut_mesh = {layer: (0, 77, 35)[i % 3] for i, layer in enumerate(fourth)}
    uppercut_mesh.update({third[0]: 77, third[1]: 35})
    sampled = {0, charge[0], charge[2], routes[12][0]}
    sampled.update(ready, depth_layers, impact_layers, counter_layers, blade_layers, uppercut_mesh)
    sampled.update(sprint_and_guard)
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
        if layer not in sampled:
            continue
        following = layer in ready
        counter = layer in counter_layers or layer in blade_layers
        resource = load_efl(data)
        unit = g.alloc(528)
        g.run(0x9B8DF8, unit)
        g.put(unit + 244, resource)
        assert g.run(0x9BDA64, unit) == 1
        assert g.half(unit + 480) == g.half(unit + 484) == 1
        gen = g.get(unit + 496)
        if layer in sprint_and_guard:
            params = g.get(gen + 52)
            assert g.cpu.mem_read(params + 136, 3) == b"FZF"
            fade, = struct.unpack("<f", g.cpu.mem_read(params + 140, 4))
            allowed = (0., .01, .03) if layer in routes[35] else (.03,)
            assert min(abs(fade - expected) for expected in allowed) < 1e-7
        native_joint = g.get(gen + 24)
        assert g.get(native_joint + 260) == g.get(resource + 104) + joint
        assert struct.unpack("<3f", g.cpu.mem_read(native_joint + 240, 12)) == (1, 1, 1)
        g.put(unit + 272, 1)
        fourth_wave = layer in fourth
        delay = (fourth.index(layer) // 3) * 4 if fourth_wave else 0
        if fourth_wave:
            # GU's root matrix at a quarter turn in a non-origin area.
            g.cpu.mem_write(unit + 288, struct.pack(
                "<16f", 0,0,-1,0, 0,1,0,0, 1,0,0,0, 1200,85,-700,1))
        visible = False
        debris_positions = []
        # The delayed third pair of counter blade flashes lives through age 76.
        frames = 91 if layer in charge or layer in sprint_and_guard else 77 if layer in blade_layers else 30
        for frame in range(frames):
            g.run(0x9B9668, unit)
            if following:
                # Move the GU joint after the native particle has already been born.
                x = 180 if frame >= 8 else 0
                g.cpu.mem_write(native_joint + 48, struct.pack("<4f", x, 0, 0, 1))
            g.run(0xA5A48C, gen)
            g.run(0x9B975C, unit)
            g.run(0xA94C10, gen)
            if layer == debris and frame in (2, 3):
                packet, = draw(gen)
                debris_positions.append(packet.position)
            depth_age = frame + 1 - (6 if layer == release[5] else 0)
            if layer in depth_layers and depth_age in (6, 7, 8):
                g.cpu.mem_write(camera, struct.pack(
                    "<16f", 1,0,0,0, 0,1,0,0, 0,0,1,0, 0,0,1000,1))
                packet, = draw(gen)
                assert math.dist(packet.position, (0, 0, 62. * (depth_age - 6))) < 0.01, (
                    layer, depth_age, packet.position)
            if (following and frame in (6, 9)) or (layer == release[2] and frame in (3, 5)):
                x = 180 if following and frame >= 8 else 0
                g.cpu.mem_write(camera, struct.pack("<16f", 1,0,0,0, 0,1,0,0, 0,0,1,0, x,0,1000,1))
                packet, = draw(gen)
                if following:
                    assert abs(packet.position[0] - x) < 0.01, (layer, frame, packet.position)
                # Independent source lifetimes. GU evaluates age 1 on the first tick.
                life = g.get(g.get(resource + 104) + (struct.unpack_from("<I", data, 56)[0] >> 8) + 4)
                cols = math.ceil(math.sqrt(life + 1))
                rows = math.ceil((life + 1) / cols)
                age = frame + 1
                assert math.dist(packet.uv, ((age % cols) / cols, (age // cols) / rows)) < 1e-6, (layer, frame, packet.uv)
            if frame == delay + 4 and layer in uppercut_mesh:
                mesh = uppercut_mesh[layer]
                assert g.string(g.get(gen + 52) + 80) == rf"effect\frontier_gs\m{mesh}"
                for yaw, pitch in ((0, 0), (math.pi / 2, 0), (math.pi / 2, math.pi / 4)):
                    c, s = math.cos(yaw), math.sin(yaw)
                    a, b = math.cos(pitch), math.sin(pitch)
                    g.cpu.mem_write(camera, struct.pack(
                        "<16f", c, 0, -s, 0, s*b, a, c*b, 0, s*a, -b, c*a, 0, 0, 0, 0, 1,
                    ))
                    packet, = draw(gen)
                    normal = packet.normal
                    if fourth_wave:
                        local_x, local_z = offsets[fourth.index(layer) // 3]
                        rise = 0 if mesh == 0 else 5 * 200 / 32
                        world = packet.position
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
                # The counter includes authored negative scales for mirrored meshes.
                checked_draw = counter or layer in sprint_and_guard
                scaled = all(v != 0 for v in size) if checked_draw else all(v > 0 for v in size)
                if alpha > 0 and scaled and not visible:
                    if checked_draw:
                        assert draw(gen), path
                        mesh = NX / g.string(g.get(gen + 52) + 80).replace("\\", "/")
                        material = mesh.with_suffix(".mrl").read_bytes()
                        tex_slot = struct.unpack_from("<I", material, 20)[0] + 12
                        tex = material[tex_slot:tex_slot+64].split(b"\0")[0].decode().replace("\\", "/")
                        assert (NX / (tex + ".tex")).is_file()
                    visible = True
                if layer in charge:
                    assert alpha <= 96
                node = g.get(node + 4)
        assert visible, path
        if layer == debris:
            a, b = debris_positions
            assert abs(b[0] - a[0] - 7.2) < 0.02 and abs(b[2] - a[2] - 6.6) < 0.02 and b[1] > a[1], (a, b)
    print("GU native EFL: sprint slash and GP draw, depth fade, and uppercut transforms passed")


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


def emit_model_layer(g, load, data, frames):
    unit = g.alloc(528)
    g.run(0x9B8DF8, unit)
    g.put(unit + 244, load(data))
    assert g.run(0x9BDA64, unit) == 1
    gen = g.get(unit + 496)
    g.put(unit + 272, 1)
    for _ in range(frames):
        g.run(0x9B9668, unit)
        g.run(0xA5A48C, gen)
        g.run(0x9B975C, unit)
        g.run(0xA94C10, gen)
    return gen


def soft_particles(g):
    load, _, draw = model_renderer(g, NX, depth_scene=True)
    layer = effect_routes()[8][1]
    data = (NX / f"effect/frontier_gs/e{layer}.efl").read_bytes()
    gen = emit_model_layer(g, load, data, 25)
    params = g.get(gen + 52)
    assert g.cpu.mem_read(params + 136, 3) == b"FZF"
    assert abs(struct.unpack("<f", g.cpu.mem_read(params + 140, 4))[0] - .03) < 1e-7

    # Unicorn runs NEON VNEG.F32, but MHGURecomp aborts on the SIMD encoding.
    # Mask off the D, M and Q bits so all register assignments are checked.
    def check_instruction(cpu, address, size, _):
        word, = struct.unpack("<I", cpu.mem_read(address, 4))
        assert word & 0xFFBF0F90 != 0xF3B90780, f"unsupported NEON VNEG.F32 at {address:#x}"

    provider = g.hooks[SUBMIT_MODEL] & ~0xFFFFF
    g.cpu.hook_add(
        UC_HOOK_CODE, check_instruction, begin=provider, end=provider + 0xFFFFF,
    )

    # Compare the same live particle's original and adapted draw packets.
    signature = bytes(g.cpu.mem_read(params + 136, 8))
    path_start = bytes(g.cpu.mem_read(params + 80, 1))
    g.cpu.mem_write(params + 136, b"\0" * 8)
    g.cpu.mem_write(params + 80, b"_")
    original, = draw(gen)
    g.cpu.mem_write(params + 80, path_start)
    expected_rgb = tuple((channel * 7 + 5) // 10 for channel in original.rgb)
    assert expected_rgb != original.rgb
    # GU A956D8 passes particle + 24 to the draw. B007FC reads bits 5-15.
    node = g.get(gen + 176)
    particle_flags = g.get(node + 24)
    for selector in (0, 1, 6):
        g.put(node + 24, (particle_flags & ~0xFFE0) | (selector << 5))
        full, = draw(gen)
        assert full.rgb == (original.rgb if selector == 6 else expected_rgb), (selector, full)
        assert full.alpha == original.alpha
    g.put(node + 24, particle_flags)
    g.cpu.mem_write(params + 136, signature)
    # FZ HD pixel shader 0x37 divides the normalized depth delta by c165.x.
    for threshold in (.03, .01):
        g.cpu.mem_write(params + 140, struct.pack("<f", threshold))
        near, far = draw(gen)
        assert near.rgb == far.rgb == expected_rgb
        assert near.alpha + far.alpha == original.alpha
        assert not (near.flags | far.flags) & 0x18
        assert near.uv == far.uv == original.uv
        for sample, fraction in ((near, .25), (far, .75)):
            expected = threshold * fraction / 0.00020028039580211043
            delta = original.position[2] - sample.position[2]
            assert abs(delta - expected) < .001, (threshold, delta, expected)
            assert abs(math.hypot(*sample.basis_x) / math.hypot(*original.basis_x)
                       - (1000 - sample.position[2]) / (1000 - original.position[2])) < .00001

    # Changing the seeds exercises both mirror axes, even with fade off.
    g.cpu.mem_write(params + 139, b"\x03" + bytes(4))
    g.cpu.mem_write(node + 272, struct.pack("<2f", .25, .25))
    left, = draw(gen)
    g.cpu.mem_write(node + 272, struct.pack("<2f", .75, .75))
    right, = draw(gen)
    assert left.basis_x == tuple(-v for v in right.basis_x)
    assert left.position == right.position
    assert left.alpha == right.alpha == original.alpha
    print("GU model submissions: RGB gain, blend isolation, depth and stable mirror passed")


if __name__ == "__main__":
    elf = (REPO / "mhgu/main.elf").read_bytes()
    resources(Game(elf))
    binding_stack(boot(elf, None))
    print("GU draw at binding stack depths 4 and 5 preserves live and outer bindings")
    emission(boot(elf, b"[Weapons]\n0=1\n"))
    soft_particles(boot(elf, None))
    draw(boot(elf, b"[Weapons]\n0=1\n"))
