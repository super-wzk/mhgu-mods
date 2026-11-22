"""Move the equipped joint after birth and read the same glow at GU draw submission."""
import math
import struct

from unicorn import arm_const as arm
from verify_native_fx import model_renderer
from verify_style import Game


def swing_arcs(root, arcs):
    nx = root / "dist/switch-axe-extreme/romfs/nativeNX"
    effects = nx / "effect/frontier_sa"
    g = Game((root.parents[1] / "mhgu/main.elf").read_bytes())
    load, camera, draw = model_renderer(g, nx)
    g.put(0x188A0D4, g.alloc(128))  # Immediate world-pose initialization.
    g.put(0x2126504, g.alloc(1800))
    g.stub(0x9B90F8, lambda *_: 0)

    def math_call(fn, registers):
        args = [struct.unpack("<f", struct.pack("<I", g.cpu.reg_read(reg)))[0]
                for reg in registers]
        g.cpu.reg_write(arm.UC_ARM_REG_S0, struct.unpack("<I", struct.pack("<f", fn(*args)))[0])

    g.stub(0x13EFD70, lambda *_: math_call(math.atan2, (arm.UC_ARM_REG_S0, arm.UC_ARM_REG_S1)))
    g.stub(0x13F0190, lambda *_: math_call(math.asin, (arm.UC_ARM_REG_S0,)))

    # The leading m265 layer travels forward; its plane tilts by the authored
    # roll before the hunter yaw. Load the shipped PEL as well as the EFL.
    pel = (effects / "particles.pel").read_bytes()
    layers = {arc[0] for arc in arcs}
    records = {}
    at = 16
    for _ in range(struct.unpack_from("<I", pel, 12)[0]):
        at += pel[at] + 1
        at += pel[at] + 1
        row = pel[at:at + 160]
        at += 160
        layer, = struct.unpack_from("<H", row, 48)
        if layer in layers:
            records[layer] = row
    resources = {layer: load((effects / f"e{layer}.efl").read_bytes()) for layer in layers}
    g.cpu.mem_write(camera, struct.pack("<16f", 1,0,0,0, 0,1,0,0, 0,0,1,0, 230,180,630,1))

    for layer, submitted, origin, facing, roll in arcs:
        owner, proof, record, bank, entries, desc = (
            g.alloc(n) for n in (416, 256, 160, 144, 4, 32)
        )
        g.run(0x328900, owner)
        g.put(owner + 52, bank)
        g.put(owner + 12, 1)
        g.put(bank + 104, 1)
        g.put(bank + 116, entries)
        g.put(entries, desc)
        g.cpu.mem_write(proof + 48, submitted)
        g.cpu.mem_write(record, records[layer])
        g.run(0x32A548, owner, proof, record)
        unit = g.alloc(896)
        g.run(0x3273F0, unit, owner, proof)
        g.run(0x32AA44, owner, unit, resources[layer], proof)
        assert g.run(0x9BDA64, unit) == 1
        g.run(0x327588, unit)
        assert g.half(unit + 56) == 4
        assert math.dist(struct.unpack("<3f", g.cpu.mem_read(unit + 64, 12)), origin) < .01

        g.put(unit + 272, 1)
        generator = g.get(unit + 496)
        assert g.string(g.get(generator + 52) + 80).endswith(r"\m265")
        for _ in range(6):
            g.run(0x327948, unit)
            g.cpu.mem_write(unit + 256, struct.pack("<f", 1.))
            g.cpu.mem_write(unit + 352, bytes(g.cpu.mem_read(unit + 64, 12)))
            g.run(0x9B839C, unit)
            g.run(0x9B9668, unit)
            g.run(0xA5A48C, generator)
            g.run(0x9B975C, unit)
            g.run(0xA94C10, generator)

        packet, = draw(generator)
        travel = tuple(a - b for a, b in zip(packet.position, origin))
        length = math.hypot(*travel)
        assert length > 0 and abs(travel[1]) < .01, (facing, roll, travel)
        yaw = math.radians(facing)
        assert math.dist(travel, (math.sin(yaw) * length, 0, math.cos(yaw) * length)) < .02
        x, y = math.cos(math.radians(roll + 180)), math.sin(math.radians(roll + 180))
        normal = tuple(v / math.hypot(*packet.normal) for v in packet.normal)
        assert math.dist(normal, (math.cos(yaw) * x, y, -math.sin(yaw) * x)) < .012
    print(f"GU swing arcs: {len(arcs)} X/A cases passed native spawn and model draw checks.")


def moving_blade(root, glow_proofs, jet_proofs):
    nx = root / "dist/switch-axe-extreme/romfs/nativeNX"
    effects = nx / "effect/frontier_sa"
    g = Game((root.parents[1] / "mhgu/main.elf").read_bytes())
    load, camera, draw = model_renderer(g, nx)

    def floats(address, count):
        return struct.unpack(f"<{count}f", g.cpu.mem_read(address, 4 * count))

    def put(address, *values):
        g.cpu.mem_write(address, struct.pack(f"<{len(values)}f", *values))

    def matrix(address, c, s, xyz):
        put(address, c, 0, -s, 0, 0, 1, 0, 0, s, 0, c, 0, *xyz, 1)

    # The scene supplies a live equipment handle. GU's C1628 resolves its
    # logical joint 0 to the world matrix, independently of the hunter root.
    equipment, vtable, bones, mapping = (g.alloc(n) for n in (1200, 100, 320, 256))
    handle, handle_vt = g.alloc(16), g.alloc(8)
    g.put(equipment, vtable)
    g.put(equipment + 12, 1)
    g.put(equipment + 240, handle)
    g.put(equipment + 1172, bones)
    g.put(equipment + 1176, mapping)
    g.cpu.mem_write(mapping, b"\x01")
    g.put(vtable + 84, 0xC1628)
    g.put(handle, handle_vt)
    for offset, result in ((0, 1), (4, equipment)):
        callback = g.alloc(4)
        g.put(handle_vt + offset, callback)
        g.stub(callback, lambda *_, value=result: value)
    g.put(0x2126504, g.alloc(1800))
    # Step the EFL stages below, directly after the native unit updated its pose.
    g.stub(0x9B90F8, lambda *_: 0)

    pel = (effects / "particles.pel").read_bytes()
    pos = 16
    seen = set()
    blade_layers = 0
    proofs = glow_proofs | jet_proofs
    for _ in range(struct.unpack_from("<I", pel, 12)[0]):
        pos += 1 + pel[pos]
        pos += 1 + pel[pos]
        row = pel[pos:pos + 160]
        pos += 160
        layer = struct.unpack_from("<H", row, 48)[0]
        if layer not in proofs:
            continue
        seen.add(layer)
        owner, proof, record, bank, entries, desc = (g.alloc(n) for n in (416, 256, 160, 144, 4, 32))
        g.put(owner + 52, bank)
        g.put(bank + 104, 1)
        g.put(bank + 116, entries)
        g.put(entries, desc)
        g.cpu.mem_write(proof + 48, proofs[layer])
        g.put(proof + 48 + 192, handle)
        g.cpu.mem_write(record, row)
        g.run(0x32A548, owner, proof, record)
        # Discharge PEL rows default to world orientation. Read GU's merged
        # result to catch callers that follow the joint's position alone.
        assert g.cpu.mem_read(proof + 48 + 72, 1) == b"\0", layer
        unit = g.alloc(896)
        g.run(0x3273F0, unit, owner, proof)
        g.put(unit + 244, load((effects / f"e{layer}.efl").read_bytes()))
        assert g.run(0x9BDA64, unit) == 1
        g.put(unit + 272, 1)
        if layer in jet_proofs:
            for c, s in ((1, 0), (0, 1)):
                matrix(bones + 176, c, s, (100, 60, -20))
                g.run(0x327948, unit)
                assert math.dist(floats(unit + 64, 3), (100 - 20*c + 40*s, 60, -20 + 20*s + 40*c)) < .001
                qx, qy, qz, qw = floats(unit + 80, 4)
                forward = (2*(qx*qz + qw*qy), 2*(qy*qz - qw*qx), 1 - 2*(qx*qx + qy*qy))
                assert math.dist(forward, (s, 0, c)) < .001, (layer, forward)
            continue
        gen = g.get(unit + 496)
        blade = g.string(g.get(gen + 52) + 80).endswith(r"\m121")
        blade_layers += int(blade)

        visible_frames = 0
        required_frames = 4 if blade else 3
        for frame in range(35):
            x = 100 if visible_frames == 0 else 600
            if visible_frames == 3:
                # Tilt upwards independently of hunter yaw.
                put(bones + 176, 1, 0, 0, 0, 0, 0, -1, 0, 0, 1, 0, 0, x, 60, -20, 1)
                expected_origin, forward = (x - 10, 80, -20), (0, 1, 0)
            else:
                c, s = (0, 1) if visible_frames == 2 else (1, 0)
                matrix(bones + 176, c, s, (x, 60, -20))
                expected_origin, forward = (x - 10*c + 20*s, 60, -20 + 10*s + 20*c), (s, 0, c)
            matrix(camera, 1, 0, (x, 80, 1000))
            g.run(0x327948, unit)
            origin = floats(unit + 64, 3)
            assert math.dist(origin, expected_origin) < .001
            # Supply unit scale and the resolved position to the render step.
            put(unit + 96, 1, 1, 1)
            put(unit + 256, 1)
            put(unit + 352, *origin)
            g.run(0x9B839C, unit)
            g.run(0x9B9668, unit)
            g.run(0xA5A48C, gen)
            g.run(0x9B975C, unit)
            g.run(0xA94C10, gen)
            draws = draw(gen)
            if not draws:
                continue
            position = draws[0].position
            if visible_frames == 0:
                before = position
            elif visible_frames == 1:
                assert abs(position[0] - before[0] - 500) < 5, (layer, position, before)
            if blade:
                axis = draws[0].normal
                length = math.dist(axis, (0, 0, 0))
                assert length > 0
                assert math.dist(tuple(v / length for v in axis), forward) < .001, (layer, frame, axis)
            visible_frames += 1
            if visible_frames == required_frames:
                break
        assert visible_frames == required_frames, (layer, visible_frames)
    assert seen == proofs.keys() and blade_layers == 4
    print(f"GU sword FX: {len(jet_proofs)} discharge layers inherit joint rotation; all four blade axes followed a 90-degree tilt.")
