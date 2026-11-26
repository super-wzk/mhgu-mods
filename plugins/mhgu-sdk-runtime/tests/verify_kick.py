"""Replay GU's shared kick with each packaged extreme style selected."""
from pathlib import Path
import struct
import sys
import zlib

from unicorn.arm_const import UC_ARM_REG_SP

PLUGINS = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(PLUGINS / "great-sword-extreme/tests"))
from verify_style import (
    character_save,
    STYLE_IDS,
    CHANGE_ACTION, COMMAND_UPDATE, DISPATCH, boot, hunters, load_registered_style,
    quest_entries, setup_controller,
)


def replay(elf, row, weapon, plugin, vtable):
    g = boot(elf, character_save({weapon: STYLE_IDS[plugin]}), ("registry",))
    nro = PLUGINS / plugin / "dist" / plugin / "romfs/nativeNX/plugins" / (
        plugin.replace("-", "_") + ".nro"
    )
    load_registered_style(g, nro)
    actor, eq, _ = hunters(g)
    g.put(actor, vtable)
    g.put(actor + 240, g.alloc(128))
    g.cpu.mem_write(eq + 1236, bytes([weapon]))
    set_action = setup_controller(g, actor)
    for address in (0x2A2444, COMMAND_UPDATE):
        g.cpu.hook_del(g.traps.pop(address))

    def change(_, group, slot, flags):
        set_action(group, slot)
        g.put(actor + 9384, 0)
        return 1

    motions, hits = [], []
    g.stub(g.originals[CHANGE_ACTION], change)
    g.stub(0x281998, lambda _, clip, *__: motions.append(clip) or 0)
    g.stub(0x2B72D4, lambda *_: hits.append(g.get(g.cpu.reg_read(UC_ARM_REG_SP) + 12)) or 0)
    for address in (0x27D5C4, 0x3A5ED0, 0x5584A0, 0x27FE18):
        g.stub(address, lambda *_: 0)
    if weapon == 9:
        g.stub(0x27F728, lambda *_: 0)
        g.put(0x1897F78, g.alloc(17600))
    for offset in (752, 756, 792, 796, 808, 812, 1992):
        g.stub(g.get(vtable + offset), lambda *_: 0)
    g.put(actor + 5068, 1)
    g.put(actor + 1188, 1)
    g.cpu.mem_write(actor + 5078, struct.pack("<h", 100))
    g.cpu.mem_write(actor + 28, struct.pack("<f", 2))

    table, entries, entry, resource = [g.alloc(n) for n in (24, 4, 24, 124)]
    g.cpu.mem_write(table, struct.pack("<6I", 0, 0, 1, 1, 0, entries))
    g.put(entries, entry)
    g.cpu.mem_write(entry, struct.pack("<3I3f", 0, 1, resource, 0, 0, 999))
    fields = [struct.unpack_from("<H", row, at)[0] for at in (504, 510, 516, 522)]
    parameters = [struct.unpack_from("<I", row, at)[0] for at in (528, 536, 544, 552)]
    g.cpu.mem_write(resource + 100, struct.pack("<4H4I", *fields, *parameters))

    for drawn in (False, True):
        set_action(1, 0)
        g.cpu.mem_write(actor + 5, b"\x01")
        g.cpu.mem_write(actor + 10057, bytes([drawn]))
        g.put(actor + 9384, table)
        g.cpu.mem_write(actor + 9392, bytes(48))
        g.cpu.mem_write(actor + 5140, bytes(32))
        g.cpu.mem_write(actor + 5108, struct.pack("<3H", 0x1000, 0, 0x1000))
        g.run(0x27CA28, actor)  # Map the physical key to common command 8.
        g.run(g.get(vtable + 556), actor)
        g.run(COMMAND_UPDATE, actor)
        current = (g.half(actor + 9478), g.half(actor + 9482))
        assert current == ((1, 0) if drawn else (16, 800)), (plugin, drawn, current)
        if not drawn:
            g.run(DISPATCH, actor)  # Run GU 2B81A0's common kick handler.
            assert motions == [50] and hits == [36], (plugin, motions, hits)

    print(f"{plugin}: sheathed native kick and drawn command ownership passed")


if __name__ == "__main__":
    elf = (PLUGINS.parent / "mhgu/main.elf").read_bytes()
    row = next(
        zlib.decompress(raw) for name, kind, raw in quest_entries("com_m0.arc")
        if kind == 0x5139901D and name.endswith(b"\\plcmd_com_0x0010_800a")
    )
    for weapon, plugin, vtable in (
        (0, "great-sword-extreme", 0x1749A88),
        (11, "dual-blades-extreme", 0x174F2B4),
        (7, "long-sword-extreme", 0x174CF64),
        (8, "switch-axe-extreme", 0x174D838),
        (9, "gunlance-extreme", 0x174E110),
    ):
        replay(elf, row, weapon, plugin, vtable)
