"""Compare combat-choices CSV against the Vita ARM interpreter (no running game).

The fixture sets player geometry and the random counter. Calls for world-space
target transforms are intercepted; the interpreter still evaluates every
branch and follows the original script pointers. Each 05 request is recorded
and immediately resumed, without playing the action or advancing world time.
"""

import argparse
import csv
import struct
from pathlib import Path

from unicorn import UC_ARCH_ARM, UC_HOOK_CODE, UC_MODE_ARM, Uc
from unicorn import arm_const as arm
from game_files import VITA_ELF, elf


def verify(elf_path, csv_path):
    game, segments = elf(elf_path)
    cpu = Uc(UC_ARCH_ARM, UC_MODE_ARM)
    for _, p, v, _, n, ms, *_ in segments:
        lo = v & ~4095
        cpu.mem_map(lo, ((v + ms + 4095) & ~4095) - lo)
        cpu.mem_write(v, game[p : p + n])
    actor = 0x1001000
    sp = 0x1060000
    cpu.mem_map(0x1000000, 0x80000)
    read, write = cpu.reg_read, cpu.reg_write
    write(arm.UC_ARM_REG_C1_C0_2, read(arm.UC_ARM_REG_C1_C0_2) | 0xF00000)
    write(arm.UC_ARM_REG_FPEXC, 0x40000000)

    def u32(p):
        return int.from_bytes(cpu.mem_read(p, 4), "little")

    desc = u32(u32(0x82BB0A80) + 4 * 80)
    state = u32(u32(desc) + 12)
    thresholds = cpu.mem_read(u32(0x8257F99C + 40 * 80), 16)

    def at(pc, fn):
        cpu.hook_add(UC_HOOK_CODE, lambda *_: fn(), begin=pc, end=pc)

    def ret(value):
        write(arm.UC_ARM_REG_R0, value)
        write(arm.UC_ARM_REG_PC, read(arm.UC_ARM_REG_LR))

    # Geometry only: force the exact turn angle, leaving the native threshold comparison.
    for pc in (0x816950BA, 0x8183FD96):
        at(pc, lambda: ret(0))
    at(0x81694CF4, lambda: ret(angle))

    def fetch():
        ins = bytes(cpu.mem_read(read(arm.UC_ARM_REG_R7), 4))
        if ins[:2] == b"\xff\x00":
            cpu.emu_stop()
        elif ins[0] == 5:
            requests.append(f"{ins[1]}:{ins[2]}")
            assert ins[3] == 0
            write(arm.UC_ARM_REG_R7, read(arm.UC_ARM_REG_R7) + 4)
            write(arm.UC_ARM_REG_PC, 0x816AACE1)

    at(0x816AAACA, fetch)

    def set_actor(offset, fmt, value):
        cpu.mem_write(actor + offset, struct.pack("<" + fmt, value))

    checked = 0
    with csv_path.open() as f:
        for row in csv.DictReader(f):
            cpu.mem_write(actor, bytes(4176))
            cpu.mem_write(sp, bytes(160))
            cpu.mem_write(actor + 2560, bytes(thresholds))
            no_player = row["distance"] == "none"
            angle = int(row["angle_units"])
            for off, fmt, value in [
                (3, "B", 80),
                (2544, "I", desc),
                (2548, "I", state),
                (2726, "B", int(row["enraged"])),
                (1040, "B", 2 * int(row["airborne"])),
                (2612, "b", -1 if no_player else 0),
                (2740, "f", -1 if no_player else float(row["distance"])),
                (1096, "H", int(row["rng5"])),
            ]:
                set_actor(off, fmt, value)
            for reg, value in [
                (arm.UC_ARM_REG_R9, actor),
                (arm.UC_ARM_REG_R7, state),
                (arm.UC_ARM_REG_R10, 0),
                (arm.UC_ARM_REG_SP, sp),
            ]:
                write(reg, value)
            requests = []
            cpu.emu_start(0x816AAA6B, 0, count=100000)
            assert (
                read(arm.UC_ARM_REG_PC) == 0x816AAACA
                and requests == row["requests"].split()
            ), (row, requests, hex(read(arm.UC_ARM_REG_PC)))
            checked += 1
    assert checked == 2560, checked
    print(f"{checked} battle choices match the original interpreter")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("choices", type=Path)
    parser.add_argument("--elf", type=Path, default=VITA_ELF)
    args = parser.parse_args()
    verify(args.elf, args.choices)
