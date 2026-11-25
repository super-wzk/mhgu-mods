"""Run the Vita style dispatcher and Switch LMT loader against the converted resources."""

import argparse
import struct
import zlib
from pathlib import Path

from unicorn import UC_ARCH_ARM, UC_HOOK_CODE, UC_MODE_ARM, Uc
from unicorn import arm_const as arm

ROOT = Path(__file__).resolve().parents[1]
REPO = ROOT.parents[1]
MEM = 0x6000000
STOP = MEM + 0x100
REG = [arm.UC_ARM_REG_R0, arm.UC_ARM_REG_R1, arm.UC_ARM_REG_R2, arm.UC_ARM_REG_R3]


class Machine:
    def __init__(self, path, thumb=False):
        self.c = c = Uc(UC_ARCH_ARM, UC_MODE_ARM)
        self.thumb = thumb
        c.ctl_set_cpu_model(arm.UC_CPU_ARM_MAX)
        b = path.read_bytes()
        (ph,) = struct.unpack_from("<I", b, 28)
        stride, count = struct.unpack_from("<2H", b, 42)
        for i in range(count):
            kind, off, va, _, size, mem, *_ = struct.unpack_from(
                "<8I", b, ph + i * stride
            )
            if kind == 1:
                c.mem_map(va, (mem + 4095) & ~4095)
                c.mem_write(va, b[off : off + size])
        c.mem_map(MEM, 0x400000)
        c.reg_write(
            arm.UC_ARM_REG_C1_C0_2, c.reg_read(arm.UC_ARM_REG_C1_C0_2) | 0xF00000
        )
        c.reg_write(arm.UC_ARM_REG_FPEXC, 0x40000000)
        self.hooks = {}

    def trap(self, address, callback):
        def stub(c, *_):
            value = callback(*(c.reg_read(r) for r in REG))
            if value is not None:
                c.reg_write(REG[0], value)
            c.reg_write(arm.UC_ARM_REG_PC, c.reg_read(arm.UC_ARM_REG_LR))

        if address in self.hooks:
            self.c.hook_del(self.hooks[address])
        self.hooks[address] = self.c.hook_add(
            UC_HOOK_CODE, stub, begin=address, end=address
        )

    def call(self, address, *params):
        c = self.c
        for r, v in zip(REG, params):
            c.reg_write(r, v)
        c.reg_write(arm.UC_ARM_REG_SP, MEM + 0x3FF000)
        c.reg_write(arm.UC_ARM_REG_LR, STOP | self.thumb)
        c.emu_start(address | self.thumb, STOP, count=900000)
        assert c.reg_read(arm.UC_ARM_REG_PC) == STOP, hex(address)
        return c.reg_read(REG[0])

    def put(self, addr, value):
        self.c.mem_write(addr, struct.pack("<I", value))


def source(states):
    assert states[:8] == b"FGSA\x02\0\x4a\0"
    assert states[-84:-80] == b"FX08" and struct.unpack_from(
        "<3H", states, len(states) - 80
    ) == (0, 12, 27)
    m = Machine(REPO / "mhfz/eboot.bin.elf", True)

    def pointer(address):
        return int.from_bytes(m.c.mem_read(address, 4), "little")

    # Check the identity independently of the extracted program: these are the
    # retail Japanese names and the path indices used by the native MOT loader.
    for key, name in [(0, "大剣"), (3, "ランス")]:
        label = name.encode("cp932") + b"\0"
        assert m.c.mem_read(pointer(0x82CB0558 + 4 * key), len(label)) == label
        path_index = pointer(0x8258A2EC + 4 * key)
        path = f"motion\\w{key:02}.mot\0".encode()
        assert m.c.mem_read(pointer(0x82BB30E8 + 4 * path_index), len(path)) == path
    data, bank, states_addr, player = (
        MEM + x for x in (0x1000, 0x2000, 0x4000, 0x20000)
    )
    m.put(0x82BB48C0, data)
    m.put(data + 0x614, bank)
    m.put(bank, 74)
    m.put(bank + 4, states_addr + 16)
    m.c.mem_write(states_addr, states)
    for i in range(74):
        for field in range(3):
            at = 16 + 24 * i + 8 * field
            count, off = struct.unpack_from("<2I", states, at)
            assert not count or off + count * (12, 40, 12)[field] <= len(states)
            if count:
                m.put(states_addr + at + 4, states_addr + off)
    # FZ resolves the player's weapon and style; let the first motion step
    # call the original paired-layer setter. Stop before the rest of the tick.
    route = pointer(0x8258FFC4 + 36) & ~1
    m.trap(0x81818BF8, lambda *_: 0)  # optional hunter extension
    result = []
    for state in range(74):
        if not struct.unpack_from("<I", states, 16 + 24 * state)[0]:
            continue  # w00 also has native actions with no table-driven opening.
        m.c.mem_write(player, bytes(4176))
        for offset, value in [(20, state), (21, 1), (3394, 3), (820, 1)]:
            m.c.mem_write(player + offset, bytes([value]))
        # Stop immediately after the native motion-step interpreter returns to the extreme dispatcher.
        c = m.c
        c.reg_write(REG[0], player)
        c.reg_write(arm.UC_ARM_REG_SP, MEM + 0x3FF000)
        c.reg_write(arm.UC_ARM_REG_LR, STOP | 1)
        c.emu_start(route | 1, 0x811D8528, count=10000)
        assert c.reg_read(arm.UC_ARM_REG_PC) == 0x811D8528
        lo, hi = struct.unpack("<2H", c.mem_read(player + 812, 4))
        (off,) = struct.unpack_from("<I", states, 20 + 24 * state)
        op, motion, start, blend, _, _ = struct.unpack_from("<6H", states, off)
        assert op in (3, 4, 5) and (lo, hi) == (motion, motion + 100), (
            state,
            op,
            lo,
            hi,
            motion,
        )
        assert struct.unpack("<2H", c.mem_read(player + 796, 4)) == (start, start)
        assert struct.unpack("<2H", c.mem_read(player + 804, 4)) == (
            blend // 2,
            blend // 2,
        )
        result.append((lo, start, blend))
    # Actual FZ event scheduler -> GS callback -> registration. Sample either
    # side of each threshold using the real frame-crossing predicate;
    # native hit registration itself and the monster are outside this test.
    event_callback = 0x811D4328
    m.put(0x823D43DC, 0x823D43E0)
    # For immediate callbacks unrelated to registering attacks, use a forwarding
    # shim: real events 16/17 call the GS handler; stop at the attack allocator.
    hits = []
    calls = []

    def event(actor, kind, param, _):
        calls.append((kind, param))
        if kind in (16, 17):
            m.c.reg_write(arm.UC_ARM_REG_PC, event_callback | 1)
        else:
            m.c.reg_write(arm.UC_ARM_REG_PC, m.c.reg_read(arm.UC_ARM_REG_LR))

    cb = MEM + 0x300
    m.c.hook_add(
        UC_HOOK_CODE,
        lambda c, *_: event(*(c.reg_read(r) for r in REG)),
        begin=cb,
        end=cb,
    )
    m.trap(0x81DBC114, lambda _, row, *__: hits.append(row))
    m.trap(0x81DBC368, lambda _, row, *__: hits.append(row))
    attack_count, attack_offset = struct.unpack_from("<2I", states, 8)
    # State 0's entry event must actually dispatch row 2, not a lance thrust.
    m.c.mem_write(player, bytes(4176))
    m.c.mem_write(player + 3990, struct.pack("<H", 1))
    m.call(0x81816422, player, cb | 1)
    assert result[0][0] == 1406 and hits == [2]
    # The common idle handler chooses 1001/1101 for an unsheathed great sword.
    m.c.mem_write(player, bytes(4176))
    m.c.mem_write(player + 18, b"\x01")
    c.reg_write(REG[0], player)
    c.reg_write(REG[1], 0)
    c.reg_write(REG[2], 0)
    c.reg_write(REG[3], 0)
    c.reg_write(arm.UC_ARM_REG_SP, MEM + 0x3FF000)
    c.emu_start(0x8182B6BB, 0x8182B7A8, count=10000)
    assert c.reg_read(arm.UC_ARM_REG_PC) == 0x8182B7A8
    assert struct.unpack("<2H", c.mem_read(player + 812, 4)) == (1001, 1101)
    result.append((1001, 0, 0))
    # The basic rising slash is in code rather than the action table.
    hits.clear()
    m.c.mem_write(player, bytes(4176))
    m.trap(0x81842804, lambda *_: None)
    c.reg_write(REG[0], player)
    c.reg_write(REG[1], 0)
    c.reg_write(arm.UC_ARM_REG_SP, MEM + 0x3FF000)
    c.emu_start(0x811D6A79, 0x811D6AF4, count=10000)
    assert c.reg_read(arm.UC_ARM_REG_PC) == 0x811D6AF4
    assert struct.unpack("<2H", c.mem_read(player + 812, 4)) == (
        1407,
        1507,
    ) and hits == [1]
    result.append((1407, 0, 4))
    count = 0
    for state in range(74):
        length, off = struct.unpack_from("<2I", states, 16 + 24 * state + 16)
        rows = [
            struct.unpack_from("<HBb4H", states, off + i * 12) for i in range(length)
        ]
        for i, (step, mode, countdown, frame, wait, event_id, param) in enumerate(rows):
            if event_id not in (16, 17):
                continue
            assert 0 <= param < attack_count and attack_offset + 40 * (
                param + 1
            ) <= len(states)
            if (mode, countdown, wait) != (0, 0, 0):
                continue
            for test_frame, crossed in [(frame - 0.5, False), (frame, True)]:
                hits.clear()
                calls.clear()
                m.c.mem_write(player, bytes(4176))
                m.c.mem_write(player + 20, bytes([state]))
                m.c.mem_write(player + 820, bytes([1]))
                m.c.mem_write(player + 3990, struct.pack("<3H", step + 1, wait, i))
                m.c.mem_write(player + 476, struct.pack("<f", test_frame))
                m.call(0x81816422, player, cb | 1)
                if crossed:
                    assert hits and hits[0] == param, (state, frame, param, hits)
                else:
                    assert not hits, (state, frame, hits)
            count += 1
    assert count
    print(f"FZ: {count} frame-triggered melee registrations verified")
    cfg = MEM + 0x380000
    m.c.mem_write(cfg, struct.pack("<f", 10.0))
    m.trap(0x817C3E88, lambda *_: cfg)
    m.trap(0x8182C996, lambda *_: 0)
    for skill, interval in [(0, 20), (0x12E, 16), (0x130, 24)]:
        m.trap(0x8114690C, lambda _, k, *__: int(k == skill))
        for ticks in (
            2 * interval - 1,
            2 * interval,
            3 * interval,
            4 * interval,
            5 * interval,
        ):
            m.c.mem_write(player + 2852, struct.pack("<H", ticks))
            for addr, cap in [(0x811D388A, 3), (0x811D39E2, 4)]:
                assert m.call(addr, player, cap) == min(
                    cap, max(0, ticks // interval - 1)
                )
        assert m.call(0x811D7546, player) == 4 * interval + 10
        assert m.call(0x811D5EF6, player, 4) == 5 * interval + 10

    # Execute the charge event and the timer update in the same dispatch.
    extension = MEM + 0x24000
    m.trap(0x81818BF8, lambda *_: extension)
    m.trap(0x8114690C, lambda *_: 0)
    for address in (0x81815D60, 0x81FC8398, 0x81815F30, 0x811D5A2A):
        m.trap(address, lambda *_: 0)
    for state, step, arm_frame in ((19, 0, 34), (20, 1, 56), (48, 0, 54)):
        m.c.mem_write(player, bytes(4176))
        m.c.mem_write(extension, bytes(672))
        m.c.mem_write(player + 20, bytes((state, 1)))
        m.c.mem_write(player + 820, b"\x01")
        m.c.mem_write(player + 3990, struct.pack("<3H", step + 1, 0, 4))
        m.c.mem_write(player + 476, struct.pack("<f", arm_frame))
        for tick in range(1, 41):
            m.call(0x811D84FE, player)
            assert struct.unpack("<H", m.c.mem_read(player + 2852, 2))[0] == tick
            assert m.c.mem_read(player + 2856, 1)[0] == int(tick == 40)
    print("FZ charge event counts as the first tick in standing, draw and uppercut charges")
    return result


def target(archive, sex, requests):
    m = Machine(REPO / "mhgu/main.elf")
    obj, stream, vt, heap, buf = (
        MEM + x for x in (0x1000, 0x2000, 0x3000, 0x4000, 0x10000)
    )
    for a in [obj, stream, heap]:
        m.put(a, vt)
    for slot in [28, 52, 60, 76]:
        m.put(vt + slot, MEM + 0x400 + slot)
    m.trap(0x7AA47C, lambda *_: heap)
    m.trap(MEM + 0x400 + 28, lambda *_: buf)
    m.trap(MEM + 0x400 + 60, lambda *_: 0)
    b = archive.read_bytes()
    assert b[:8] == b"ARC\0\x11\0\x03\0"
    found = set()
    loops = {}
    player = MEM + 0x320000
    m.put(player, 0x1749A88)  # GU uPlayerQuest00
    m.put(player + 240, MEM + 0x330000)  # model presence only; no rendering
    m.put(player + 1188, 1)
    played = 0
    for i, group in enumerate([0, 2, 4]):
        name, kind, cs, sz, off = struct.unpack_from("<64s4I", b, 12 + i * 80)
        assert name.rstrip(b"\0") == f"player\\mot\\frontier_gs\\{group}".encode()
        assert kind == 0x76820D81
        lmt = zlib.decompress(b[off : off + cs])
        assert len(lmt) == sz & 0x1FFFFFFF
        m.trap(MEM + 0x400 + 76, lambda *_, data=lmt: len(data))
        m.trap(
            MEM + 0x400 + 52,
            lambda _, addr, size, *__, data=lmt: m.c.mem_write(addr, data[:size]),
        )
        assert m.call(0xB49E78, obj, stream) == 1  # native Switch rMotionList::load
        (slots,) = struct.unpack_from("<H", lmt, 6)
        offsets = struct.unpack_from(f"<{slots}I", lmt, 8)
        def model_slot(slot):
            if sex == "f" and slot + 128 < slots and offsets[slot + 128]:
                return slot + 128
            return slot

        for slot in range(slots + 1):
            clip = m.call(0xB49E54, obj, slot)
            expected = (
                struct.unpack_from("<I", lmt, 8 + 4 * slot)[0] if slot < slots else 0
            )
            assert clip == (buf + expected if expected else 0)
            if not clip or slot != model_slot(slot & 127):
                continue
            ts, n, frames = struct.unpack("<3I", m.c.mem_read(clip, 12))
            assert frames > 0
            tracks = [tuple(m.c.mem_read(ts + j * 36, 4)) for j in range(n)]
            ids = {(bone, usage) for _, usage, _, bone in tracks}
            assert len(ids) == n and {(255, 4), (0, 0), (0, 1)} <= ids
            assert (8, 0) in ids and (12, 0) in ids, (group, slot)
            motion = 1000 + 100 * group + (slot & 127)
            (loop,) = struct.unpack("<i", m.c.mem_read(clip + 12, 4))
            if motion == 1465:
                # FZ wraps past 136 on the next two-frame tick, back to 108.
                assert (frames - 1, loop) == (138, 108), (sex, frames, loop)
            if loop >= 0:
                assert loop < frames
                loops[motion] = loop
            found.add(motion)
            if motion in (1002, 1003):
                # The original GU event resolver changes equipment at the
                # Frontier's draw / sheathe handoff frames, for both sexes.
                switch = 16 if motion == 1002 else 38
                first, last = (2, 1) if motion == 1002 else (1, 2)
                m.put(player + 3844 + 4 * 12, obj)
                m.c.mem_write(player + 1204, struct.pack("<H", 12 * 256 + slot))
                for frame, active in [
                    (0, first),
                    (switch - 1, first),
                    (switch, last),
                    (frames - 2, last),
                ]:
                    m.c.mem_write(player + 1280, struct.pack("<f", frame))
                    m.call(0x280984, player)
                    assert m.call(0x2824A8, player, active) == 1
                    assert m.call(0x2824A8, player, 3 - active) == 0
            if motion in (1001, 1406, 1407, 1473, 1474):
                assert (
                    frames
                    == {1001: 201, 1406: 193, 1407: 297, 1473: 73, 1474: 145}[motion]
                )
                # Exercise both the native event clock and gear attachment selector.
                # Missing the held event selects mode 20 / torso bone 2 (floating sword).
                gear = MEM + 0x350000
                m.put(player + 3844 + 4 * (12 + group // 2), obj)
                m.c.mem_write(
                    player + 1204, struct.pack("<H", (12 + group // 2) * 256 + slot)
                )
                switch = {1473: 52, 1474: 106}.get(motion)
                sample_frames = [0.0, float(frames // 2), float(frames - 2)]
                if switch is not None:
                    sample_frames.extend([float(switch - 1), float(switch)])
                for frame in sample_frames:
                    event, grip, wrist = (
                        (1, 0, 12) if switch is not None and frame < switch else (3, 1, 8)
                    )
                    m.c.mem_write(player + 1280, struct.pack("<f", frame))
                    m.call(0x280984, player)
                    assert m.call(0x2824A8, player, event) == 1
                    mode = m.call(0x308834, gear, player, 0)
                    m.put(gear + 5076, mode)
                    m.call(0x3084C4, gear, player, 0)
                    assert (
                        mode == grip
                        and int.from_bytes(m.c.mem_read(gear + 5068, 4), "little")
                        == wrist
                    )
        # Use three private bank numbers only in this isolated hunter. The actual
        # uPlayerQuest00 vtable converts decimal IDs, resolves the bank, starts
        # the clip and initializes its clock and events.
        private_bank = 12 + group // 2
        m.put(player + 3844 + 4 * private_bank, obj)
        for request, start, blend in requests:
            if (request - 1000) // 100 != group:
                continue
            slot = model_slot(request % 100)

            def float_bits(f):
                return int.from_bytes(struct.pack("<f", f), "little")

            m.c.reg_write(arm.UC_ARM_REG_S0, float_bits(blend // 2))
            m.c.reg_write(arm.UC_ARM_REG_S1, float_bits(start))
            m.c.reg_write(arm.UC_ARM_REG_S2, float_bits(1.0))
            assert m.call(0x53A06C, player, 0, 1000 * private_bank + slot, 0) == 1
            active = int.from_bytes(m.c.mem_read(player + 1204, 2), "little")
            assert active == private_bank * 256 + slot
            first, last = struct.unpack("<2f", m.c.mem_read(player + 1280, 8))
            assert first == min(float(start), last)
            played += 1
        m.put(player + 3844 + 4 * private_bank, 0)
    assert (
        len(loops) == 11
        and loops[1001] == 0
        and loops[1401] == 56
        and loops[1437] == 54
    )
    assert played == len(requests)
    assert len(found) == 81 and {r[0] for r in requests} <= found
    return len(found)


def guard_effects():
    m = Machine(REPO / "mhfz/eboot.bin.elf", True)
    player, effect, aux, extension, params = (MEM + n * 0x2000 for n in range(1, 6))
    waves = []
    m.trap(0x817BCD54, lambda _, selector, *__: waves.append(selector))
    for previous, current, blend, expected in [
        (89, 90, 0, []), (90, 91, 1, []), (90, 91, 0, [10, 16]),
        (92, 93, 0, [6]), (99, 109, 0, [5]),
        (100, 101, 0, [5]), (108, 109, 0, [5]),
    ]:
        waves.clear()
        m.c.mem_write(player + 476, struct.pack("<4f", previous, current - previous, 0, 128))
        m.put(player + 516, blend)
        m.c.reg_write(arm.UC_ARM_REG_R6, player)
        m.c.emu_start(0x817C08AB, 0x817BEB22, count=1000)
        assert m.c.reg_read(arm.UC_ARM_REG_PC) == 0x817BEB22
        assert waves == expected

    pulses = []
    m.put(effect + 28, aux)
    m.put(effect + 172, player)
    m.put(params + 16, 3)
    m.c.mem_write(extension + 457, b"\x03")
    m.trap(0x81818BF8, lambda *_: extension)
    m.trap(0x817C3E88, lambda *_: params)
    m.trap(0x81EC5B28, lambda *_: None)
    m.trap(0x81EC5B3A, lambda _, bank, selector, __: pulses.append((bank, selector)))
    m.call(0x8115A7CC, effect)
    assert pulses == [(4022, 21)]
    assert m.c.mem_read(aux + 88, 2) == b"\x12\0"
    m.c.mem_write(player + 18, b"\x01")
    for _ in range(12):
        m.call(0x8115B4D0, effect)
    assert len(pulses) == 1
    m.call(0x8115B4D0, effect)
    assert len(pulses) == 2
    m.c.mem_write(player + 18, b"\x00")
    m.call(0x8115B4D0, effect)
    assert len(pulses) == 2
    m.c.mem_write(player + 18, b"\x01")
    m.call(0x8115B4D0, effect)
    assert len(pulses) == 3 and pulses[-1] == (4022, 21)
    m.c.mem_write(extension + 457, b"\x00")
    for _ in range(13):
        m.call(0x8115B4D0, effect)
    assert len(pulses) == 3 and m.c.mem_read(effect + 9, 1) == b"\x09"
    print("FZ guard counter crossings and Shining Sword ready pulse lifecycle passed")


def radiance(program):
    shape = program[-40:]
    radius, *ends = struct.unpack_from("<7f", shape, 12)
    m = Machine(REPO / "mhgu/main.elf")
    obj, raw = MEM + 0x1000, MEM + 0x2000
    storage = iter(range(MEM + 0x3000, MEM + 0x4000, 64))
    for addr in (0x7AF3A0, 0x7AF37C):
        m.trap(addr, lambda *_: next(storage))
    hts = (
        ROOT
        / "dist/great-sword-extreme/romfs/nativeNX/player/hit/frontier_gs/radiance.hts"
    ).read_bytes()
    m.c.mem_write(raw, hts)
    m.put(obj + 100, raw)
    assert m.call(0x16A1D8, obj) == 1  # GU rHitSize parses the actual installed format
    word = lambda p: int.from_bytes(m.c.mem_read(p, 4), "little")
    group = word(word(obj + 104))
    assert word(group + 4) == 1
    capsule = word(word(group + 8))
    assert struct.unpack("<4H10f", m.c.mem_read(capsule, 48)) == (
        255,
        0,
        1,
        0,
        0.0,
        radius,
        *ends[:3],
        0.0,
        *ends[3:],
        0.0,
    )


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("assets", type=Path, nargs="?", default=ROOT / "target/assets")
    p = parser.parse_args().assets
    program = (p / "great_sword.fgsa").read_bytes()
    requests = source(program)
    guard_effects()
    radiance(program)
    for sex in "mf":
        print(f"{sex}: {target(p / 'motions.arc', sex, requests)} GU clips")
    print(
        f"FZ great sword: {len(requests)} verified openings, {len({r[0] for r in requests})} distinct clips"
    )
