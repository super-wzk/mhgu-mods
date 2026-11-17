"""Compare combat-choices CSV against the Vita ARM interpreter (no running game).

The fixture sets player geometry and the random counter. Calls for world-space
target transforms are intercepted; the interpreter still evaluates every
branch and follows the original script pointers. Each 05 request is recorded
and immediately resumed, without playing the action or advancing world time.
"""

import argparse
import csv
import struct
from itertools import chain
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
        if ins[:2] == terminal:
            cpu.emu_stop()
        elif ins[0] == 5:
            requests.append(f"{ins[1]}:{ins[2]}")
            assert ins[3] == 0
            write(arm.UC_ARM_REG_R7, read(arm.UC_ARM_REG_R7) + 4)
            write(arm.UC_ARM_REG_PC, 0x816AACE1)

    at(0x816AAACA, fetch)

    def set_actor(offset, fmt, value):
        cpu.mem_write(actor + offset, struct.pack("<" + fmt, value))

    terminal = b"\xff\x00"
    checked = 0
    # Two actual calm decisions in area 4, including the nearly head-on tail.
    tail_cases = [
        dict(enraged=0, airborne=0, distance=distance, angle_units=angle, rng5=roll, requests="3:12")
        for distance, angle, roll in [(652, 6439, 5), (547, 22, 6)]
    ]
    # Three consecutive decisions from the same enraged tower hunt.
    repeat_cases = [
        dict(enraged=1, airborne=0, distance=d, angle_units=a, rng5=r, requests="3:0")
        for d, a, r in [(335, 29949, 30), (352, 22675, 30), (1486, 23883, 31)]
    ]
    with csv_path.open() as f:
        for row in chain(csv.DictReader(f), tail_cases, repeat_cases):
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
    assert checked == 2560 + len(tail_cases) + len(repeat_cases), checked
    print("2560 battle choices, both front-tail decisions and all three repeated 3:0 draws match Vita.")

    stop = 0x1070000

    def call(pc, arg):
        for reg, value in [(arm.UC_ARM_REG_R0, arg), (arm.UC_ARM_REG_SP, sp),
                           (arm.UC_ARM_REG_LR, stop | 1)]:
            write(reg, value)
        cpu.emu_start(pc | 1, stop, count=1000)
        assert read(arm.UC_ARM_REG_PC) == stop
        return read(arm.UC_ARM_REG_R0)

    cpu.mem_write(actor, bytes(4176))
    # This is a witness seed, not a reconstruction of the GU quest RNG state.
    cpu.mem_write(0x84369624, struct.pack("<H", 61765))
    assert [call(0x818FE0BE, 0) & 31 for _ in range(3)] == [30, 30, 31]

    # Deliver the bit set by 8169B0D6 to the original event dispatcher.
    set_actor(3, "B", 80)
    set_actor(2544, "I", desc)
    set_actor(2726, "B", 1)
    set_actor(2823, "B", 8)
    entry = call(0x816A5474, actor)
    assert entry == u32(u32(desc + 11 * 4)) and call(0x816A5474, actor) == 0
    terminal = b"\xff\xf8"
    for posture, expected in [(0, ["3:3"]), (2, ["2:7", "2:1", "3:3"])]:
        requests = []
        set_actor(1040, "B", posture)
        cpu.mem_write(sp, bytes(160))
        for reg, value in [(arm.UC_ARM_REG_R9, actor), (arm.UC_ARM_REG_R7, entry),
                           (arm.UC_ARM_REG_R10, 0), (arm.UC_ARM_REG_SP, sp)]:
            write(reg, value)
        cpu.emu_start(0x816AAA6B, 0, count=10000)
        assert read(arm.UC_ARM_REG_PC) == 0x816AAACA and requests == expected, requests
    print("Vita rage entry: ground roars; airborne approaches, lands and then roars.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("choices", type=Path)
    parser.add_argument("--elf", type=Path, default=VITA_ELF)
    args = parser.parse_args()
    verify(args.elf, args.choices)
