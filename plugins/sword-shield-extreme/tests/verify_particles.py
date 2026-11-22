"""Compare draw packets from live GU emitters using packaged SnS layers."""

import math
import struct
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
REPO = ROOT.parents[1]
NX = ROOT / "dist/sword-shield-extreme/romfs/nativeNX"
sys.path.insert(0, str(REPO / "plugins/great-sword-extreme/tests"))
from verify_native_fx import emit_model_layer, model_renderer
from verify_style import SUBMIT_MODEL, boot, load_registered_style
from verify_port import Scene, float_bits
from unicorn import arm_const as arm


def jump_slash():
    vtable = 0x174A354
    scene = Scene(1, 5, "sword-shield", vtable)
    g, actor = scene.g, scene.actor
    manager, manager_vt, load, pel, handle, proof, parent = (
        g.alloc(n) for n in (16, 64, 4, 128, 32, 416, 16)
    )
    g.put(0x2126A64, manager)
    g.put(manager, manager_vt)
    g.put(manager_vt + 48, load)
    g.put(pel + 80, 1)
    g.put(0x188A0D4, g.alloc(128))
    g.put(0x2126F48, g.alloc(128))
    g.put(0x21270E0, g.alloc(128))
    scene.stub(load, lambda _, kind, *__: pel if kind == 0x1889FDC else 0)
    scene.stub(0x3268D8, lambda *_: handle)
    scene.stub(0x3288C4, lambda *_: proof)
    scene.stub(0x328900, lambda *_: g.cpu.mem_write(proof, bytes(416)))
    scene.stub(0x326C78, lambda *_: 0)
    scene.stub(g.get(vtable + 304), lambda *_: parent)
    scene.stub(0x53A058, lambda *_: 5001)
    scene.stub(0x2A84D8, lambda *_: 0)
    scene.stub(0x539F4C, lambda *_: 0)

    sword = g.alloc(1200)
    g.put(actor + 9120, sword)
    for model in (actor, sword):
        bones, mapping = g.alloc(176), g.alloc(256)
        g.put(model + 1172, bones)
        g.put(model + 1176, mapping)
    sword_pose = g.get(sword + 1172) + 16
    arm_pose = g.get(actor + 1172) + 16
    routes = list(struct.iter_unpack("<2H", (ROOT / "target/assets/effects/routes.bin").read_bytes()))
    spawned = {}
    samples = {}

    def sp():
        return g.cpu.reg_read(arm.UC_ARM_REG_SP)

    def capture(_, unit, *__):
        spawned[g.get(sp())] = bytes(g.cpu.mem_read(unit + 128, 200))
        return 1

    def terrain(*_):
        if floor is None:
            return 0
        g.put(g.get(sp() + 4), float_bits(floor))
        g.put(g.get(sp() + 8) + 20, float_bits(water))
        return 1

    scene.stub(0x3267D0, capture)
    scene.stub(0x18192C, terrain)

    def tick(frame):
        g.cpu.mem_write(actor + 1276, struct.pack("<2f", frame + 1, frame))
        scene.tick()

    def check_route(route, position, yaw):
        first, count = routes[route]
        expected = (0, yaw % 360, 0)
        for layer in range(first, first + count):
            data = spawned[layer]
            assert math.dist(struct.unpack_from("<3f", data, 176), position) < .01
            assert math.dist(struct.unpack_from("<3f", data, 32), expected) < .01

    # Fixed attack and weapon joint, with no surface, dry floor, or water below.
    for floor, water, facing, tilt in (
        (120, -1e6, 0, 0), (-60, -1e6, 90, 35), (20, 55, 270, -40),
        (None, -1e6, 45, 25), (-120, -1e6, 45, 90),
    ):
        scene.action(16, 0x8038)
        g.put(actor + 64, float_bits(40))
        g.put(actor + 68, float_bits(120))
        g.put(actor + 72, float_bits(90))
        g.put(actor + 4076, int(facing * 65536 / 360))
        # Joint yaw 60 degrees with variable pitch. FZ projects its X axis,
        # so the tilt cannot leak into the ground emitter's elevation.
        angle, pitch = math.radians(60), math.radians(tilt)
        c, s, cp, st = math.cos(angle), math.sin(angle), math.cos(pitch), math.sin(pitch)
        axes = (c, 0, -s, 0, s*st, cp, c*st, 0, s*cp, -st, c*cp, 0)
        if tilt == 90:
            # When the X axis points straight down, FZ uses the Z axis for yaw.
            axes = (0, -1, 0, 0, c, 0, -s, 0, s, 0, c, 0)
        pose = struct.pack("<16f", *axes, 90, 145, 160, 1)
        g.cpu.mem_write(sword_pose, pose)
        g.cpu.mem_write(arm_pose, pose)
        tick(0)
        assert g.half(actor + 1204) == 1447
        for frame in (14, 22, 30):
            spawned.clear()
            tick(frame)
            first, count = routes[1]
            for layer in range(first, first + count):
                data = spawned[layer]
                assert struct.unpack_from("<h", data, 66)[0] == (2 if frame != 30 else 0)
                assert struct.unpack_from("<I", data, 192)[0] == parent
                assert data[72] == 2 and struct.unpack_from("<3f", data, 32) == (0, facing, 0)
            if facing == 45:
                samples[first] = (spawned[first], facing)
        spawned.clear()
        tick(54)
        splash = floor is not None and water >= floor - 1
        ground_height = 120 if floor is None else floor
        if splash:
            ground_height = water
        check_route(13 if splash else 10, (40, ground_height, 90), facing + 180)
        spawned.clear()
        tick(58)
        ground_point = (90, floor if floor is not None else 145, 160)
        for route in (2, 4):
            check_route(route, ground_point, facing)
        check_route(11, ground_point, 240)
        yaw = math.radians(facing)
        check_route(3, (40 + 150 * math.sin(yaw), 120, 90 + 150 * math.cos(yaw)), facing + 180)
        if facing == 45:
            layer = routes[3][0]
            samples[layer] = (spawned[layer], facing + 180)
        spawned.clear()
        tick(58)
        assert not spawned
    check_jump_native_yaw(scene, parent, samples)
    print("GU SnS clip 1447: projected ground bursts, airborne root burst and joint yaw passed")


def check_jump_native_yaw(scene, parent, samples):
    g = scene.g
    effects = NX / "effect/frontier_sns"
    load, _, _ = model_renderer(g, NX)
    scene.stub(0x9B90F8, lambda *_: 0)
    g.put(0x2126504, g.alloc(1800))

    def math_call(fn, *regs):
        args = [struct.unpack("<f", struct.pack("<I", g.cpu.reg_read(reg)))[0] for reg in regs]
        g.cpu.reg_write(arm.UC_ARM_REG_S0, float_bits(fn(*args)))

    scene.stub(0x13EFD70, lambda *_: math_call(math.atan2, arm.UC_ARM_REG_S0, arm.UC_ARM_REG_S1))
    scene.stub(0x13F0190, lambda *_: math_call(math.asin, arm.UC_ARM_REG_S0))
    vtable = g.alloc(8)
    g.put(parent, vtable)
    for offset, value in ((0, 1), (4, scene.actor)):
        callback = g.alloc(4)
        g.put(vtable + offset, callback)
        scene.stub(callback, lambda *_, value=value: value)
    g.put(scene.actor + 12, 1)

    pel = (effects / "particles.pel").read_bytes()
    at = 16
    checked = set()
    for _ in range(struct.unpack_from("<I", pel, 12)[0]):
        at += pel[at] + 1
        at += pel[at] + 1
        row = pel[at:at + 160]
        at += 160
        layer, = struct.unpack_from("<H", row, 48)
        if layer not in samples:
            continue
        checked.add(layer)
        submitted, facing = samples[layer]
        owner, proof, record, bank, entries, desc, unit = (
            g.alloc(n) for n in (416, 256, 160, 144, 4, 32, 896)
        )
        g.run(0x328900, owner)
        g.put(owner + 52, bank)
        g.put(owner + 12, 1)
        g.put(bank + 104, 1)
        g.put(bank + 116, entries)
        g.put(entries, desc)
        g.cpu.mem_write(proof + 48, submitted)
        g.cpu.mem_write(record, row)
        g.run(0x32A548, owner, proof, record)
        g.run(0x3273F0, unit, owner, proof)
        resource = load((effects / f"e{layer}.efl").read_bytes())
        g.run(0x32AA44, owner, unit, resource, proof)
        assert g.run(0x9BDA64, unit) == 1
        g.run(0x327588, unit)
        g.run(0x327948, unit)
        position = (90, 145, 160) if submitted[70] == 0 else struct.unpack_from("<3f", submitted, 176)
        assert math.dist(struct.unpack("<3f", g.cpu.mem_read(unit + 64, 12)), position) < .01
        x, y, z, w = struct.unpack("<4f", g.cpu.mem_read(unit + 80, 16))
        forward = (2 * (x*z + w*y), 2 * (y*z - w*x), 1 - 2 * (x*x + y*y))
        yaw = math.radians(facing)
        assert math.dist(forward, (math.sin(yaw), 0, math.cos(yaw))) < .01, (layer, forward)
    assert checked == samples.keys()


def replay():
    elf = (REPO / "mhgu/main.elf").read_bytes()
    g = boot(elf, None, ("registry",))
    shared_submit = g.hooks[SUBMIT_MODEL]
    load_registered_style(g, NX / "plugins/sword_shield_extreme.nro")
    assert g.hooks[SUBMIT_MODEL] == shared_submit
    load, _, draw = model_renderer(g, NX, depth_scene=True)

    # Dry and wet contact layers use different depths and emission delays.
    # The vacuum layer uses the shared RGB gain without a depth record.
    for layer, threshold, frames in ((20, .03, 5), (49, .03, 12), (90, .01, 5), (2, 0, 5)):
        data = (NX / f"effect/frontier_sns/e{layer}.efl").read_bytes()
        gen = emit_model_layer(g, load, data, frames)
        params = g.get(gen + 52)
        signature = g.cpu.mem_read(params + 136, 3)
        if threshold:
            assert signature == b"FZF"
            (configured,) = struct.unpack("<f", g.cpu.mem_read(params + 140, 4))
            assert math.isclose(configured, threshold, abs_tol=1e-7)
        else:
            assert signature == bytes(3)

        separator = params + 80 + len(b"effect\\frontier_sns")
        assert g.cpu.mem_read(separator, 1) == b"\\"
        g.cpu.mem_write(separator, b"_")
        (original,) = draw(gen)
        g.cpu.mem_write(separator, b"\\")
        adapted = draw(gen)
        rgb = tuple((channel * 7 + 5) // 10 for channel in original.rgb)
        assert original.alpha > 0 and original.rgb != rgb, (layer, original)
        if threshold:
            near, far = adapted
            assert near.alpha > 0 and far.alpha > 0
            assert near.alpha + far.alpha == original.alpha
            for sample, fraction in ((near, .25), (far, .75)):
                assert sample.rgb == rgb and sample.uv == original.uv
                assert not sample.flags & 0x18
                assert math.isclose(
                    original.position[2] - sample.position[2],
                    threshold * fraction * 4993,
                    abs_tol=.001,
                )
        else:
            assert adapted == (original._replace(rgb=rgb),)
    routes = (ROOT / "target/assets/effects/routes.bin").read_bytes()
    layer, count = struct.unpack_from("<2H", routes, 4 * 23)
    assert count == 1
    ring = (NX / f"effect/frontier_sns/e{layer}.efl").read_bytes()
    for frame in (2, 15, 30):
        gen = emit_model_layer(g, load, ring, frame)
        packet, = draw(gen)
        radius = .10 + .78 * frame / 30
        assert math.isclose(math.hypot(*packet.basis_x), radius, abs_tol=.001), (frame, packet)
        assert math.isclose(math.hypot(*packet.normal), radius, abs_tol=.001)
        assert packet.position == (0., 0., 0.) and packet.alpha > 0
    print("GU SnS model submissions: depth, vacuum RGB gain and centered expanding ring passed")


if __name__ == "__main__":
    replay()
    jump_slash()
