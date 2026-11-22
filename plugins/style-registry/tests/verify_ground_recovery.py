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
    attack_bank = g.alloc(112)
    g.put(attack_bank + 80, 1)
    g.put(actor + 3844 + 14 * 4, attack_bank)
    def load(_, kind, path, flags):
        if kind == 0x2125210:
            return attack_bank if g.string(path).endswith("_4") else bank
        return 0
    g.stub(loader, load)
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

    cases = [(1, 1, 5002, 1002), (2, 1, 5003, 1003)]
    rolls = (1420, 1421) if weapon == 1 else (1012, 1013)
    cases.extend((6, 2, request, source) for request, source in zip((5012, 5013), rolls))
    if weapon == 1:
        cases.append((2, 1, 5003, 1450))  # native sheathe following sprint
    for slot, phase, request, source in cases:
        previous_slot = 0x8035 if source == 1450 else 0
        g.cpu.mem_write(actor + 9480, struct.pack("<H2xH", 16, previous_slot))
        packed = g.run(ENCODE, actor, request)
        group = (source - 1000) // 100
        clip = source % 100
        assert packed == (0xE00 if group == 4 else 0xF00) + clip
        lmt = next((dist / "player/mot").rglob(f"m_{group}.lmt")).read_bytes()
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
    replay(1, 5, "sword-shield-extreme", 0x174A354)
    replay(11, 2, "dual-blades-extreme", 0x174F2B4)
