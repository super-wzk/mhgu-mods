"""Run ordinary species 80's native Vita spawn/sleep/wake handlers in Unicorn.

HP reads, animation playback, quest configuration and command dispatch are
fixtures. The native code chooses the requests, thresholds, stages and motions.
"""

import argparse
import struct as s
from pathlib import Path

from unicorn import UC_ARCH_ARM, UC_HOOK_CODE, UC_MODE_ARM, Uc
from unicorn import arm_const as arm
from game_files import VITA_ELF, elf


def verify(path):
    cpu = Uc(UC_ARCH_ARM, UC_MODE_ARM)
    b, segs = elf(path)
    for _, off, va, _, n, ms, *_ in segs:
        lo = va & ~4095
        cpu.mem_map(lo, ((va + ms + 4095) & ~4095) - lo)
        cpu.mem_write(va, b[off : off + n])
    cpu.mem_map(0x1000000, 0x100000)
    actor = 0x1001000
    ctx = actor + 1296
    stack = 0x1080000
    stop = 0x1090000
    rr = [arm.UC_ARM_REG_R0, arm.UC_ARM_REG_R1, arm.UC_ARM_REG_R2, arm.UC_ARM_REG_R3]
    rd, wr = cpu.reg_read, cpu.reg_write
    wr(arm.UC_ARM_REG_C1_C0_2, rd(arm.UC_ARM_REG_C1_C0_2) | 0xF00000)
    wr(arm.UC_ARM_REG_FPEXC, 0x40000000)

    def setat(p, f, x):
        cpu.mem_write(actor + p, s.pack("<" + f, x))

    def getat(p, f):
        return s.unpack("<" + f, cpu.mem_read(actor + p, s.calcsize(f)))[0]

    def service(addr, fn):
        def call(*_):
            val = fn(*(rd(r) for r in rr))
            wr(rr[0], val or 0)
            wr(arm.UC_ARM_REG_PC, rd(arm.UC_ARM_REG_LR))

        cpu.hook_add(UC_HOOK_CODE, call, begin=addr, end=addr)

    def run(pc, *args):
        for reg, x in zip(rr, args):
            wr(reg, x)
        wr(arm.UC_ARM_REG_LR, stop | 1)
        wr(arm.UC_ARM_REG_SP, stack)
        try:
            cpu.emu_start(pc | 1, stop, count=30000)
        except Exception as e:
            raise RuntimeError(
                f"PC {rd(arm.UC_ARM_REG_PC):#x} LR {rd(arm.UC_ARM_REG_LR):#x}"
            ) from e
        assert rd(arm.UC_ARM_REG_PC) == stop, (hex(pc), hex(rd(arm.UC_ARM_REG_PC)))

    cpu.mem_write(
        0x82C3B320, s.pack("<I", 0x1020000)
    )  # quest context with no active players
    observed = []

    def request(p, g, i, flags):
        assert p == actor
        observed.append(("request", g, i))

    def motion(p, i, blend, start):
        assert p == actor
        observed.append(("motion", i))
        setat(812, "H", 1000 + i)

    def hp_read(key, field, p, _):
        assert (key, field, p) == (0xFFFFFFFD, 4, actor)
        return hp

    service(0x81849A84, hp_read)
    service(0x81CE4652, request)
    service(0x81695980, motion)
    # Isolate animation/UI/target maintenance, not the wake decision.
    for f in (
        0x8169CEEA,
        0x8169C46A,
        0x816A0320,
        0x81CC60D6,
        0x816B2CB8,
        0x816B2DC0,
        0x8168EA78,
        0x816A12B0,
    ):
        service(f, lambda *_: 0)
    for hp0 in (5000, 32000):
        hp = hp0
        cpu.mem_write(actor, bytes(4176))
        setat(3, "B", 80)
        observed.clear()
        run(0x81CC54EE, actor)
        assert observed == [("request", 0, 40)]
        assert getat(1398, "H") == 9000 and getat(2601, "B") == 0
        run(0x81CC6254, actor, ctx, 0)
        assert observed[-1] == ("motion", 39)
        run(0x81CC6254, actor, ctx, 0)
        assert observed[-1] == ("motion", 34)
        setat(1296 + 96, "h", 60)
        hp -= min(1000, hp * 2 // 100)
        n = len(observed)
        run(0x81CC6254, actor, ctx, 0)
        assert len(observed) == n
        hp -= 1
        run(0x81CC6254, actor, ctx, 0)
        assert observed[-1] == ("request", 0, 43)
        setat(20, "B", 43)
        setat(5, "B", 0)
        run(0x81CC6D7C, actor, ctx)
        assert observed[-1] == ("motion", 37)
        run(0x81CC6D7C, actor, ctx)
        assert observed[-1] == ("request", 0, 25)
        setat(20, "B", 25)
        setat(5, "B", 0)
        run(0x81CC6D7C, actor, ctx)
        assert observed[-1] == ("motion", 62)
    setat(1384, "B", 1)
    run(0x81CC54EE, actor)
    assert observed[-1] == ("request", 0, 1)
    print(
        "Vita: ordinary spawn 0:40, 39 -> 34; strict HP threshold, 0:43/37 -> 0:25/62; explicit awake override uses 0:1."
    )

    # Start in the sleeping stage. Include an active hunter in the same
    # area; the original handler chooses the pose from the hunter's side.
    player = 0x83A28E70
    cpu.mem_write(0x1020000 + 9210, bytes([1]))
    cpu.mem_write(player, bytes([1]))
    service(0x8183B4B2, lambda *_: 1)
    service(0x818FE0BE, lambda *_: roll)
    responses = []
    for z, roll in [(451.0, 25), (-451.0, 25), (451.0, 26), (100.0, 0), (1000.0, 0)]:
        hp = 5000
        cpu.mem_write(actor, bytes(4176))
        setat(3, "B", 80)
        setat(5, "B", 2)
        setat(1386, "h", hp)
        setat(1388, "h", hp)
        setat(1398, "h", 9000)
        cpu.mem_write(player + 180, s.pack("<f", z))
        observed.clear()
        run(0x81CC6254, actor, ctx, 0)
        responses.append(observed.copy())
    assert responses == [[("request", 0, 42)], [("request", 0, 41)], [], [], []]
    print(
        "Vita sleep proximity (front, rear, above roll, distance boundaries):",
        responses,
    )
    cpu.mem_write(0x1020000 + 9210, bytes([0]))
    # 81CE4220 prepares distance and timeout. Supply a level target and
    # unit size; the native 816A2C1E and 816A2FEA do the actual arithmetic.
    for f in (0x816A2928, 0x816B0A9E, 0x816991EC, 0x816B2CCC, 0x816B1C78, 0x816A2DDE):
        service(f, lambda *_: 0)
    binding = []

    def delta(_p, out, *_):
        cpu.mem_write(out, s.pack("<3f", 0.0, 0.0, step))

    service(0x81699568, delta)
    for action, recovery in ((1, 108), (2, 118)):
        for distance, tracked, initial, deadline in (
            (451.0, True, 1000.0, 63),
            (5001.0, True, 4501.0, 180),
            (0.0, False, 1000.0, 60),
        ):
            cpu.mem_write(actor, bytes(4176))
            setat(3, "B", 80)
            setat(3206, "H", 100)
            setat(3231, "B", int(tracked))
            setat(1826, "b", 0 if tracked else -1)
            setat(2860, "f", distance)
            run(0x81CE4220, actor, action, 0)
            assert (getat(3220, "f"), getat(8, "i")) == (initial, deadline), (
                distance,
                getat(3220, "f"),
                getat(8, "i"),
            )
            handler = 0x81CCE692 if action == 1 else 0x81CCE410
            observed.clear()
            run(handler, actor, ctx)
            assert observed == [("motion", 107)]
            step = 100.0
            expected = int(-(-initial // step)) if tracked else deadline
            for _ in range(expected - 1):
                run(handler, actor, ctx)
                assert len(observed) == 1
            run(handler, actor, ctx)
            assert observed == [("motion", 107), ("motion", recovery)]
            binding.append((action, distance, deadline, expected, recovery))
    print(
        "Vita charge (request, distance, timeout, steps until brake at 100 units/step, recovery):",
        binding,
    )


if __name__ == "__main__":
    a = argparse.ArgumentParser(description=__doc__)
    a.add_argument("--elf", type=Path, default=VITA_ELF)
    verify(a.parse_args().elf)
