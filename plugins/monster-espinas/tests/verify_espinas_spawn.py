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

    # Use the original turn handler and its retail table. Isolate the input
    # bearing, animation clock and flag services; leave clip selection and
    # heading arithmetic in the Vita code.
    service(0x81719FC4, lambda *_: (heading + 0x4000) & 65535)
    service(0x8182B488, lambda *_: 0)
    service(0x8182D39E, lambda *_: 0)
    service(0x817333AA, lambda *_: 0)  # outside the calm turn's frame window
    for profile, heading, expected, duration in (
        (0, 4096, 6, 160), (1, 4096, 2, 58),
        (1, 8192, 2, 58), (1, 8193, 8, 48), (2, -25000, 7, 48),
    ):
        cpu.mem_write(actor, bytes(4176))
        setat(3, "B", 80)
        setat(2848, "f", 1.)
        observed.clear()
        run(0x81CD3DF2, actor, ctx, profile)
        assert observed == [("motion", expected)], (profile, heading, observed)
        setat(468, "I", 1)
        setat(488, "f", duration)
        run(0x81CD3DF2, actor, ctx, profile)
        step = int(32768 / duration) if profile else 0
        assert getat(164, "H") == (step if heading > 0 else -step) & 65535
    print("Vita turns: calm window and enraged short, left and right clips verified.")

    # The native flight controller uses the full animation step for velocity
    # and its integer part for the separate curve cursor. These reference values
    # also exercise the motion-rate assumptions in the shipping NRO regression.
    for kind, start in [(14, 76), (17, 70)]:
        cpu.mem_write(actor, bytes(4176))
        setat(1040, "B", 2)
        setat(1353, "B", 1)
        setat(1354, "B", 1)
        setat(1356, "B", kind)
        setat(1358, "h", start)
        setat(480, "f", 2.6)
        peak, steps = 0., []
        for t in range(200):
            z = getat(180, "f")
            run(0x81CE4E06, actor)
            steps.append(getat(180, "f") - z)
            assert getat(1358, "h") == start + 2 * (t + 1)
            peak = max(peak, getat(176, "f"))
            if peak > 0 and getat(176, "f") <= 0:
                break
        assert abs(peak - 596.7) < .1, (kind, peak)
        if kind == 17:
            assert all(abs(dz + 150.475) < .02 for dz in steps[5:10]), steps[5:10]
    print("Vita rage flight: both curves peak at 596.7; backhop reaches 150.475 units per tick.")

    # Ordinary 1:11 reads species 80's own area table. Use the real records
    # for field 226 and the empty sentinel for an unmapped field; only the
    # stage identity/count services are supplied by the surrounding quest.
    u32 = lambda addr: s.unpack("<I", cpu.mem_read(addr, 4))[0]
    routes = u32(0x8257F99C + 40 * 80 + 4)
    first = u32(routes + 4)
    field, count, points = s.unpack("<hhI", cpu.mem_read(first, 8))
    assert (field, count) == (226, 2)
    coords = [s.unpack("<3f", cpu.mem_read(points + i * 12, 12)) for i in range(count)]
    service(0x818E1AC4, lambda *_: 256)
    service(0x818E37EA, lambda field, *_: field)
    for area_id, roll, start, expected in [
        (999, 0, coords[0], None),
        (226, 0, coords[0], None),  # retries keep this roll, never switch to point 1
        (226, 1, coords[0], coords[1]),
    ]:
        cpu.mem_write(actor, bytes(4176))
        setat(3, "B", 80)
        setat(20, "B", 11)
        setat(2040, "H", area_id)
        setat(2900, "I", routes)
        setat(1096, "H", roll)
        cpu.mem_write(actor + 172, s.pack("<3f", *start))
        observed.clear()
        run(0x81CD3E9A, actor, ctx)
        if expected is None:
            assert getat(5, "B") == 7 and not observed
            run(0x81CD3E9A, actor, ctx)
            assert observed == [("request", 0, 1)]
        else:
            assert observed == [("request", 1, 12)]
            assert s.unpack("<3f", cpu.mem_read(actor + 2852, 12)) == expected
    print("Vita patrol: unmapped area and near point skip; mapped far point starts patrol.")

    for request_id, clip in [(3, 110), (0, 111)]:
        cpu.mem_write(actor, bytes(4176))
        setat(3, "B", 80)
        setat(20, "H", 3 << 8 | request_id)
        setat(2726, "B", 1)
        observed.clear()
        run(0x81CCE8E2, actor, ctx)
        assert observed == [("motion", clip)]
        setat(468, "I", 1)
        run(0x81CCE8E2, actor, ctx)
        assert observed == [("motion", clip)]
        setat(468, "I", 0)
        run(0x81CCE8E2, actor, ctx)
        assert observed == [("motion", clip), ("request", 0, 1)]
    print("Vita: rage entry plays 110; the repeatable battle request 3:0 plays 111.")

    # 3:12 is the calm stationary tail action, not either spinning-tail variant.
    cpu.mem_write(actor, bytes(4176))
    setat(3, "B", 80)
    setat(20, "H", 3 << 8 | 12)
    setat(164, "H", 7070)
    observed.clear()
    run(0x81CCE8E2, actor, ctx)
    assert observed == [("motion", 106)] and getat(164, "H") == 7070
    setat(468, "I", 1)
    run(0x81CCE8E2, actor, ctx)
    assert observed == [("motion", 106)] and getat(164, "H") == 7070
    setat(468, "I", 0)
    run(0x81CCE8E2, actor, ctx)
    assert observed == [("motion", 106), ("request", 0, 1)] and getat(164, "H") == 7070
    print("Vita 3:12: motion 106, unchanged facing until return to idle.")


    # Probe the actual idle handler and thought gate; animation and script
    # interpretation are boundary services, while the countdown remains native.
    decisions = []
    service(0x816A42E0, lambda *_: 0)
    service(0x818E37EA, lambda *_: 0)
    service(0x816AA9C0, lambda *_: decisions.append(tick))
    for request_id, idle in [(1, 1), (17, 51)]:
        cpu.mem_write(actor, bytes(4176))
        setat(3, "B", 80)
        setat(20, "H", request_id)
        setat(2601, "B", 1)
        observed.clear()
        decisions.clear()
        run(0x81CC6D7C, actor, ctx, 0, 0)
        assert observed == [("motion", idle)]
        assert getat(3228, "h") == 4
        clocks = []
        for tick in range(1, 6):
            run(0x816A135C, actor)
            clocks.append(getat(3228, "h"))
        assert clocks == [3, 2, 1, 0, 0] and decisions == [5]
        print(f"FZ 0:{request_id}: idle motion {idle}, 4 logic ticks suppress thinking, first decision at tick {decisions[0]}.")


    # Real FZ animation clock, with rendering services isolated. The pending
    # frame remains fixed throughout blending; the last tick transfers it to
    # the primary clock and advances by the source animation step.
    cpu.mem_write(actor, bytes(4176))
    model, rig, first, second, ref = (actor + n for n in (0x20000, 0x21000, 0x22000, 0x23000, 0x24000))
    def word(p, value):
        cpu.mem_write(p, s.pack("<I", value))
    setat(1656, "I", model)
    for offset, p in [(36, rig), (68, first), (84, second)]:
        word(model + offset, p)
    setat(528, "I", ref)
    word(ref, ref + 16)
    setat(834, "H", 1)
    motion_table = u32(0x8257DDF4 + 40 * 80 + 20)
    blend = cpu.mem_read(motion_table + 12 * 47 + 6, 1)[0] // 2
    assert blend == 5
    service(0x8182D384, lambda _, flags, *__: int(flags == 0x4000000))
    for address in (0x81008020, 0x81007BCE, 0x81002782, 0x81007D54, 0x810062A2, 0x8182D39E, 0x81007FA2):
        service(address, lambda *_: 0)
    for rate in (1., 1.3, 1.5):
        for off, value in [(476, 100.), (488, 100.), (480, 2*rate), (496, 0.), (508, 294.), (500, 1/(blend+1)), (524, 1/(blend+1))]:
            setat(off, "f", value)
        setat(516, "I", 1)
        setat(520, "I", blend + 1)
        for left in range(blend, -1, -1):
            run(0x8173208A, actor)
            assert getat(520, "I") == left and getat(496, "f") == 0.
            assert abs(getat(476, "f") - (100. if left else 2*rate)) < .0001
    print("Vita fireball: six blend ticks hold the incoming frame; final frames 2.0 / 2.6 / 3.0 at rates 1.0 / 1.3 / 1.5.")


if __name__ == "__main__":
    a = argparse.ArgumentParser(description=__doc__)
    a.add_argument("--elf", type=Path, default=VITA_ELF)
    verify(a.parse_args().elf)
