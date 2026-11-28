"""Compare each shipped command angle with GU's native inclusive sectors."""
import struct
import subprocess
from pathlib import Path

from gu_replay import Game, REGS

ROOT = Path(__file__).resolve().parents[3]
BASE = 0x9200000


def verify(plugin, elf):
    nro, = (plugin / "dist").rglob("*.nro")
    library, = (plugin / "target/armv7a-none-eabi/release").glob("*.so")
    symbols = subprocess.check_output(["nm", str(library)], text=True)
    symbol, = (line for line in symbols.splitlines() if line.endswith("command_stick_yaw"))
    command = BASE + int(symbol.split()[0], 16)
    game = Game(elf)
    game.load(nro, BASE)
    actor, table = game.alloc(16000), game.alloc(720)
    game.put(actor, table)
    camera = 0
    for offset, callback in ((652, lambda *_: 1), (656, lambda *_: camera)):
        entry = game.alloc(4)
        game.put(table + offset, entry)
        game.stub(entry, callback)

    for setting, flags in ((0, 0), (1, 0), (0, 0x4000)):
        game.put(actor + 9536, setting)
        game.put(actor + 9488, flags)
        for camera, target in ((0, 0x4000), (0x7130, 0x2340)):
            game.put(actor + 5280, target)
            for stick in (0, 0x2000, 0x4000, 0x8000, 0xc000, 0xffff):
                for strength in (49, 50, 120):
                    game.cpu.mem_write(actor + 5132, struct.pack("<H2xH", stick, strength))
                    active = game.run(command, actor)
                    yaw = game.cpu.reg_read(REGS[1]) & 0xffff
                    assert active == (strength >= 50)
                    angle = (0x4000 - yaw) & 0xffff
                    for lo, hi in ((angle, angle), (angle+1, angle+1), (0xe000, 0x2000)):
                        lo, hi = lo & 0xffff, hi & 0xffff
                        expected = game.run(0x27e638, actor, 0, lo, hi, 0)
                        actual = active and (angle-lo) % 65536 <= (hi-lo) % 65536
                        assert actual == expected, (plugin.name, setting, flags, stick, strength, yaw)
    print(plugin.name, "matched GU directional command sectors")


if __name__ == "__main__":
    elf = (ROOT / "mhgu/main.elf").read_bytes()
    for name in ("great-sword-extreme", "dual-blades-extreme", "long-sword-extreme",
                 "switch-axe-extreme", "sword-shield-extreme", "gunlance-extreme", "magnet-spike"):
        verify(ROOT / "plugins" / name, elf)
