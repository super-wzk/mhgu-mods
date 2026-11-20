"""Exercise the native draw, sheathe and drawn roll with Frontier clips and no contacts."""
from pathlib import Path
import struct
import sys

PLUGINS = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(PLUGINS / "great-sword-extreme/tests"))
from verify_style import DISPATCH, ENCODE, boot, hunters, load_registered_style, setup_controller


def replay(weapon, style, plugin, vt):
    elf = (PLUGINS.parent / "mhgu/main.elf").read_bytes()
    g = boot(elf, f"[Weapons]\n{weapon}={style}\n".encode(), ("registry",))
    dist = PLUGINS / plugin / "dist" / plugin / "romfs/nativeNX"
    load_registered_style(g, dist / "plugins" / (plugin.replace("-", "_") + ".nro"))
    actor, eq, _ = hunters(g)
    g.put(actor, vt)
    g.cpu.mem_write(eq + 1236, bytes([weapon]))
    setup_controller(g, actor)
    manager, table, loader, bank = [g.alloc(n) for n in (16, 64, 4, 112)]
    g.put(0x2126A64, manager)
    g.put(manager, table)
    g.put(table + 48, loader)
    g.stub(loader, lambda _, kind, *__: bank if kind == 0x2125210 else 0)
    g.put(bank + 80, 1)
    g.put(actor + 3844 + 15 * 4, bank)
    g.stub(0xB49E54, lambda *_: 1)
    g.cpu.mem_write(actor + 5078, struct.pack("<h", 100))
    g.cpu.mem_write(actor + 10057, b"\1")
    g.stub(0x27BCD0, lambda *_: 0)
    for off in (376, 1912):
        g.stub(g.get(vt + off), lambda *_: 0)
    exits = []
    g.stub(0x2B6E94, lambda *_: exits.append(True))  # Native shared return to locomotion.

    lmt = next((dist / "player/mot").rglob("m_0.lmt")).read_bytes()
    for slot, phase, clip in ((1, 1, 2), (2, 1, 3), (6, 2, 12), (6, 2, 13)):
        packed = g.run(ENCODE, actor, 5000 + clip)
        assert packed == 0xF00 + clip
        offset, = struct.unpack_from("<I", lmt, 8 + 4 * clip)
        frames, loop = struct.unpack_from("<Ii", lmt, offset + 8)
        assert loop == -1
        end = frames - 1
        g.cpu.mem_write(actor + 9478, struct.pack("<H2xH", 1, slot))
        g.cpu.mem_write(actor + 5, bytes([phase]))
        g.cpu.mem_write(actor + 1204, struct.pack("<H", packed))
        for previous, frame, delta, finished in (
                (end - 2, end - 2, 2, False), (end - 2, end, 2, False),
                (end, end, 0, False), (end, end, 2, True)):
            exits.clear()
            g.cpu.mem_write(actor + 28, struct.pack("<f", delta))
            g.cpu.mem_write(actor + 1206, b"\x02")
            g.cpu.mem_write(actor + 1276, struct.pack("<3f", previous, frame, end))
            g.run(g.hooks[DISPATCH], actor)
            assert bool(exits) == finished, (plugin, slot, clip, previous, frame, delta, exits)
    print(f"{plugin}: native draw, sheathe and both rolls exit only at a confirmed stopped end")


if __name__ == "__main__":
    replay(0, 1, "great-sword-extreme", 0x1749A88)
    replay(11, 2, "dual-blades-extreme", 0x174F2B4)
