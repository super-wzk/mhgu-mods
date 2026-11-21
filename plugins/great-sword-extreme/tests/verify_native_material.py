"""Run GU's model-particle draw parameter path; GPU submission is not emulated."""
import math
import struct
from unicorn import arm_const as arm
from verify_style import Game, REPO, ROOT, NX


def radiance_texels(directory, layer, source_mesh, source_atlas, life, delay, speed, samples):
    def texture(name):
        data = (directory / (name + ".tex")).read_bytes()
        dimensions, = struct.unpack_from("<I", data, 8)
        w, h = dimensions >> 6 & 8191, dimensions >> 19
        bh = min(16, 1 << (((h + 7) // 8) - 1).bit_length())

        def at(x, y):
            x, y = 4 * (x % w), y % h
            gob = (y // (8 * bh) * ((4*w + 63)//64) + x//64) * bh + y//8 % bh
            p = 24 + 512*gob + (x%64//32)*256 + (y%8//2)*64 + (x%32//16)*32 + (y%2)*16 + x%16
            return data[p:p+4]

        return w, h, at

    def first_uv(name):
        mod = (directory / (name + ".mod")).read_bytes()
        vertex, = struct.unpack_from("<I", mod, 52)
        return struct.unpack_from("<2e", mod, vertex + 16)

    name = f"r{layer}"
    w, h, source = texture(f"t{source_atlas}")
    width, height, baked = texture(name)
    u, v = first_uv(f"m{source_mesh}")
    s, t = first_uv(name)
    left, top = round(u*w - s*width), round(v*h - t*height)
    cols = math.ceil(math.sqrt(life + 1))
    cw, ch = width // cols, height // math.ceil((life + 1) / cols)
    for frame, rgba in samples:
        age = frame - delay
        # These reference frames have integral scroll in original atlas texels.
        dx, dy = speed[0] * age * w, speed[1] * age * h
        assert abs(dx - round(dx)) < 1e-5 and abs(dy - round(dy)) < 1e-5
        for y in range(ch):
            for x in range(cw):
                original = source(left + x + round(dx), top + y + round(dy))
                expected = tuple(round(c * tint / 255) for c, tint in zip(original, rgba))
                actual = baked(age % cols * cw + x, age // cols * ch + y)
                assert max(abs(a-b) for a,b in zip(actual, expected)) <= 1, (layer, frame, x, y, actual, expected)


def rendering(g):
    # Supply the graphics allocator, material binding and final GPU submission.
    # C90B50, 886BB4 and B007FC still select and write the actual native state.
    for addr in (0x13EFD7C, 0x13EFDAC, 0x13EFDC4):
        g.stub(addr, lambda dest, n, *_: g.cpu.mem_write(dest, bytes(n)) or dest)
    for addr in (0x13EFE30, 0x13EFDD0):
        g.stub(addr, lambda dest, src, n, _: g.cpu.mem_write(dest, bytes(g.cpu.mem_read(src, n))) or dest)
    g.stub(0x86A438, lambda *_: 16)
    for addr in (0xBD3C2C, 0x8826FC, 0x893CA8, 0x882760, 0x87A0B8):
        g.stub(addr, lambda *_: 0)
    ctx, descriptors, defs = (g.alloc(24000) for _ in range(3))
    uniforms, default, meta = g.alloc(65536), g.alloc(256), g.alloc(64)
    g.put(meta + 24, 64)
    g.put(meta + 36, default)
    g.cpu.mem_write(descriptors, struct.pack("<I", meta) * 6000)
    g.cpu.mem_write(defs, b"".join(struct.pack("<II", i, 0) for i in range(3000)))
    g.put(ctx + 8, descriptors)
    g.put(ctx + 12, defs)
    g.put(ctx + 16, defs)
    g.put(ctx + 20, uniforms)
    g.put(ctx + 24, uniforms + 65536)
    g.put(ctx + 3524, default)
    matrix = g.alloc(64)
    for offset in (0, 20, 40, 60):
        g.put(matrix + offset, 0x3F800000)
    g.stub(0x885804, lambda *_: matrix)
    base_color = g.alloc(256)
    g.stub(0x87A048, lambda *_: g.put(ctx + 4772, base_color))
    attributes, material_data = {}, b""
    g.stub(0xB0FFD4, lambda obj, key, *_: next(
        (value for slot, (kind, value) in attributes.items()
         if kind == 0 and slot & 65535 == key & 65535), 0))
    mat, mat_vt, get_color = g.alloc(128), g.alloc(128), g.alloc(4)
    g.put(mat, mat_vt)
    g.put(mat_vt + 40, get_color)
    g.stub(get_color, lambda out, *_: g.cpu.mem_write(out, material_data[:16]))
    model, model_vt, get_mat = g.alloc(256), g.alloc(128), g.alloc(4)
    g.put(model, model_vt)
    g.put(model_vt + 64, get_mat)
    g.stub(get_mat, lambda *_: mat)
    g.put(model + 116, g.alloc(48))
    effect, uv, flags, color = (g.alloc(n) for n in (640, 48, 16, 16))
    g.put(effect + 112, model)
    g.put(effect + 140, 1)
    blend = g.alloc(352)
    g.put(0x21268B4, blend)
    g.run(0xC7A3CC, blend + 104)
    for i in range(34):
        state = g.get(blend + 108 + 4 * i)
        g.put(ctx + 8 * (state & 4095) + 516, state)

    routes = list(struct.iter_unpack("<HH", (ROOT / "target/assets/effects/routes.bin").read_bytes()))
    uppercut_flash, fourth, third, dust, late_dust, release, ready, charge, blade = (
        routes[i][0] for i in (0, 1, 2, 5, 6, 7, 8, 9, 23))
    directory = NX / "effect/frontier_gs"
    assert not (directory / "m66_alpha.tex").exists()
    blend_states = {
        0: 0x805701F3,  # Additive alpha
        1: 0x0F09B1F4,  # Alpha blend
        6: 0x8150B1F9,  # Reverse subtract with alpha blend factors
    }
    # Radiance's atlas supplies its tint and opacity. GU must multiply it by
    # neutral white, including when another particle previously set the tint.
    cases = {
        uppercut_flash: (0, [(2, (255, 64, 32, 255)), (8, (255, 64, 32, 128))]),
        # The 4006 smoke uses the default ground tint and each source alpha key.
        fourth + 1: (1, [(2, (126, 118, 109, 255)), (8, (126, 118, 109, 128))]),
        third: (1, [(2, (126, 118, 109, 255)), (8, (126, 118, 109, 128))]),
        dust + 1: (1, [(2, (126, 118, 109, 255)), (8, (126, 118, 109, 128))]),
        late_dust + 2: (1, [(10, (126, 118, 109, 255)), (22, (126, 118, 109, 128))]),
        release: (6, [(0, (255,) * 4)]),
        release + 2: (0, [(4, (255,) * 4), (6, (255,) * 4)]),
        release + 4: (1, [(5, (0,) * 4), (6, (255,) * 4)]),
        ready + 1: (0, [(24, (255,) * 4)]),
        charge: (0, [(90, (255, 192, 64, 96))]),
        charge + 2: (1, [(90, (224, 32, 32, 96))]),
    }
    # FZ 4022:9 has three delayed pairs of layers with separate alpha curves.
    for i, (white_peak, white_end, glow_peak, glow_end) in enumerate(
        [(6, 24, 12, 32), (19, 40, 26, 50), (28, 61, 39, 75)]
    ):
        cases[blade + 2 * i] = (1, [
            (white_peak, (255,) * 4), (white_end, (255, 255, 255, 0)),
        ])
        cases[blade + 2 * i + 1] = (0, [
            (glow_peak, (255, 200, 124, 124)), (glow_end, (255, 0, 0, 0)),
        ])
    for layer, (selector, samples) in cases.items():
        resource = (directory / f"e{layer}.efl").read_bytes()
        assert struct.unpack_from("<f", resource, 12)[0] == (30 if layer in (charge, charge + 2) else 60)
        efl = resource[48:]
        offset = struct.unpack_from("<I", efl, 4)[0] >> 8
        params = efl[offset:offset + 320]
        assert params[3] == selector, layer
        mesh = params[80:144].split(b"\0", 1)[0].decode().replace("\\", "/")
        mrl = (NX / (mesh + ".mrl")).read_bytes()
        word = lambda offset: struct.unpack_from("<I", mrl, offset)[0]
        if layer == charge:
            assert mesh == "effect/frontier_gs/m66"
            assert b"effect\\frontier_gs\\t3\0" in mrl
        row = word(24)
        block = word(row + 52)
        attributes = {word(p + 8): (word(p) & 15, word(p + 4))
                      for p in range(block, block + 12 * (word(row + 24) & 4095), 12)}
        assert attributes[0x870175E8] == (0, 0x5BBC25EA)  # FEmissionConstantEflEmu
        assert attributes[0xFDB6F5A7] == (0, 0xD41875C0)  # FDiffuseVertexColorMD
        sampler = 0x11DAE184 if release <= layer < charge else 0x7100317E
        assert attributes[0xCAA79222] == (2, sampler)
        mod = (NX / (mesh + ".mod")).read_bytes()
        draw, vertices, indices = struct.unpack_from("<3I", mod, 48)
        # Match the retail cm153_000 model that supplies the shader material.
        assert struct.unpack_from("<I", mod, 120)[0] == 1
        assert struct.unpack_from("<H", mod, draw)[0] == 0xE4C1
        assert struct.unpack_from("<I", mod, draw + 8)[0] == 0x04189A01
        assert struct.unpack_from("<I", mod, draw + 20)[0] == 0x207D603B  # IANonSkinBC
        assert mod[draw + 10] == 24
        assert indices - vertices == 24 * struct.unpack_from("<I", mod, 12)[0]
        if layer in (charge, charge + 2):
            assert mod[vertices + 20:vertices + 24] == bytes((127, 127, 127, 255))
        material_offset = block + attributes[0x6C801214][1]
        material_data = mrl[material_offset:material_offset + 128]
        g.cpu.mem_write(base_color, material_data)
        g.put(flags, selector << 5)  # A9958C's per-particle blend selector.
        curve = offset + struct.unpack_from("<H", params, 66)[0]
        keys = g.alloc(len(efl) - curve)
        g.cpu.mem_write(keys, efl[curve:])
        for age, rgba in samples:
            g.run(0xAF938C, color, keys, age, 128)
            assert bytes(g.cpu.mem_read(color, 4)) == bytes(rgba), (layer, age)
            g.cpu.reg_write(arm.UC_ARM_REG_S0, 0)  # No extra specular gain.
            g.run(0xC90B50, effect, ctx, 0, matrix, color, 256, flags, 0, uv)
            assert g.get(ctx + 280) == blend_states[selector], (layer, age, selector)
            # The retail model-particle program binds CBPrimEflEmu (0x2ad).
            tint = struct.unpack("<4f", g.cpu.mem_read(g.get(ctx + 5996), 16))
            assert all(abs(a - b / 255) < 1e-6 for a, b in zip(tint, (*rgba[:3], 255)))
            opacity = struct.unpack("<f", g.cpu.mem_read(g.get(ctx + 3524) + 16, 4))[0]
            assert abs(opacity - rgba[3] / 255) < 1e-6, (layer, age, opacity, rgba)
            if layer == ready + 1:
                # Each soft-particle sample comes through C90B50 as a
                # translucent packet with the native depth-write bits clear.
                g.put(ctx + 8 * 443 + 516, 443)
                g.cpu.mem_write(color + 3, b"\x80")
                g.run(0xC90B50, effect, ctx, 0, matrix, color, 256, flags, 0, uv)
                assert g.get(ctx + 284) == 443
                assert g.get(ctx + 280) == blend_states[selector]
    print("GU native draw: material tint, opacity and blend state passed")
    radiance_texels(directory, release + 2, 33, 3, 12, 0, (0, 0),
                    [(4, (255,150,72,255)), (6, (255,72,0,124)), (12, (255,0,0,0))])
    radiance_texels(directory, ready + 1, 234, 8, 64, 0, (-0.5/48, 0),
                    [(0, (255,240,48,0)), (24, (255,200,124,124))])
    print("Radiance release and third-guard TEX colours, alpha and scrolling passed")


if __name__ == "__main__":
    rendering(Game((REPO / "mhgu/main.elf").read_bytes()))
