"""Run the packaged magnetic effects through GU's native loader and draw path."""

import math
from pathlib import Path
import struct
from tempfile import TemporaryDirectory

from verify_battle import MANAGER, PLUGIN, REPO, assets, boot
from verify_native_fx import model_renderer


def verify():
    g = boot((REPO / "mhgu/main.elf").read_bytes(), None, ("registry",))
    exports = g.load(PLUGIN, 0x9200000)
    for callback in ("forge_onLoad", "forge_onInit"):
        g.run(exports[callback], MANAGER)
    resources = assets(PLUGIN.parents[1] / "arc/frontier/magnet_spike.arc")
    with TemporaryDirectory() as staging:
        for (name, kind), data in resources.items():
            if kind == 0x58A15856 and name.startswith("effect\\frontier_ms\\"):
                path = Path(staging) / (name.replace("\\", "/") + ".mod")
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(data)
        load, camera, draw = model_renderer(g, Path(staging), depth_scene=True)
        clock, unit, point = (g.alloc(n) for n in (128, 416, 16))
        g.put(0x2126764, clock)
        g.put(clock + 56, 0x3F800000)
        for layer in range(105):
            child = g.alloc(528)
            g.run(0x9B8DF8, child)
            name = rf"effect\frontier_ms\e{layer}"
            efl = resources[name, 0x6D5AE854]
            g.put(child + 244, load(efl))
            assert g.run(0x9BDA64, child) == 1
            generator = g.get(child + 496)
            # Running world EFL with one GU frame per update.
            g.put(child + 280, 0x01000000)
            for offset in (28, 248, 256):
                g.put(child + offset, 0x3F800000)
            g.put(unit + 336, child)

            def tick(x):
                g.cpu.mem_write(point, struct.pack("<4f", x, 120., 50., 0.))
                g.cpu.mem_write(camera + 48, struct.pack("<4f", x, 120., 1050., 1.))
                g.run(0x32A45C, unit, point, 0)
                g.run(0x9B90F8, child)

            if layer >= 13:
                life = 48 + (struct.unpack_from("<I", efl, 56)[0] >> 8)
                frames, = struct.unpack_from("<I", efl, life + 4)
                joint = 48 + struct.unpack_from("<I", efl, 64)[0]
                delay, = struct.unpack_from("<I", efl, joint + 100)
                if layer >= 93:
                    slash = layer < 99
                    index = layer - (93 if slash else 99)
                    if slash:
                        phase = ((8, 30., -100.), (12, 60., 10.), (16, 30., 100.))
                    else:
                        phase = ((0, 0., 40.), (12, 0., 100.), (24, 0., 170.))
                    start, x, z = phase[index % 3]
                    second = index >= 3
                    assert delay == start + 4 * second  # The second source layer starts later.
                    assert struct.unpack_from("<3f", efl, joint + 32) == (
                        x + (-30. if second else 30.), 0., z,
                    ), name
                visible = False
                for age in range(delay + frames):
                    tick(100.)
                    if any(packet.alpha and math.hypot(*packet.basis_x) > 0.0001
                           for packet in draw(generator)):
                        assert age >= delay - 1, (name, age, delay)
                        visible = True
                        break
                assert visible, name
                continue

            tick(100.)
            first, = draw(generator)
            tick(350.)
            moved, = draw(generator)
            offset = tuple(new - old for old, new in zip(first.position, moved.position))
            assert math.dist(offset, (250., 0., 0.)) < 0.01, (name, offset)
            assert first.alpha == moved.alpha > 0, name
            if layer == 12:  # The rotating mark layer must still turn after a full cycle.
                samples = {}
                for frame in range(2, 601):
                    tick(350.)
                    if frame in (60, 120, 600):
                        packet, = draw(generator)
                        samples[frame] = packet.basis_x
                assert math.dist(samples[60], samples[120]) > 0.01
                assert math.dist(samples[60], samples[600]) < 0.002
    print("Magnet Spike: native shot, rotating mark and both guards draw with their authored timing")


if __name__ == "__main__":
    verify()
