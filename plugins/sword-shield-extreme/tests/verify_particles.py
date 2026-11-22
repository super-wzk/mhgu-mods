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
