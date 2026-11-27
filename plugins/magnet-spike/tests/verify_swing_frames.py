"""Compare packaged w13 sockets and event clocks to the Vita executable, offline."""
import math
import os
import struct
import subprocess
import sys
import tempfile
from pathlib import Path

from unicorn import UC_HOOK_CODE
from unicorn.arm_const import UC_ARM_REG_R1, UC_ARM_REG_S0
from verify_magnet_spike import ROOT, REPO, PLUGIN
from verify_style import api_for, boot, MANAGER
from verify_assets import MEM, Machine
sys.path.insert(0, str(REPO / "tools/tests"))
from verify_packed_release import assets


def durations(part=False):
    out = {1: (1, -1)}  # The table's only GU common idle, state 50, has no attack.
    prefix = r"weapon\frontier\magnet_spike\mot" if part else r"player\mot\frontier_ms"
    for (name, _), data in assets(PLUGIN.parents[1] / "arc/frontier/magnet_spike.arc").items():
        if name.startswith(prefix) and data.startswith(b"LMT\0"):
            bank = int(name.rsplit("\\", 1)[1])
            for slot in range(min(128, struct.unpack_from("<H", data, 6)[0])):
                offset, = struct.unpack_from("<I", data, 8 + slot * 4)
                if offset:
                    frames, restart = struct.unpack_from("<Ii", data, offset + 8)
                    out[1000 + 100 * bank + slot] = (frames - 2 * (restart >= 0), restart)
    return out


def socket_frames(fz, hunter, lengths):
    g = boot((REPO / "mhgu/main.elf").read_bytes(), None, ("registry",))
    bind = g.get(api_for(g, b"mhgu::WeaponPartMotion", b"WPMT"))
    descriptors = []
    g.cpu.hook_add(UC_HOOK_CODE, lambda cpu, *_: descriptors.append(cpu.reg_read(UC_ARM_REG_R1)),
                   begin=bind, end=bind)
    exports = g.load(PLUGIN, 0x9200000)
    for name in ("forge_onLoad", "forge_onInit"):
        g.run(exports[name], MANAGER)
    descriptor, = descriptors
    # The registered Motions contains the bank slice, clip selector, and socket selector.
    assert g.get(descriptor + 4) == 4
    socket = g.get(descriptor + 12)
    actor, result = g.alloc(14000), g.alloc(40)
    source_table = []
    for i in range(4):
        joint, scale, x, y, z, *angles = struct.unpack(
            "<B3x7f", fz.c.mem_read(0x824F0B68 + 32 * i, 32))
        sx, sy, sz = [math.sin(v / 2) for v in angles]
        cx, cy, cz = [math.cos(v / 2) for v in angles]
        quat = (sx*cy*cz - cx*sy*sz, cx*sy*cz + sx*cy*sz,
                cx*cy*sz - sx*sy*cz, cx*cy*cz + sx*sy*sz)
        source_table.append(({18: 12, 14: 8, 10: 2}[joint], x, y, z, *quat, scale))
    samples = 0
    for clip, (length, _) in lengths.items():
        if clip < 1000:
            continue
        fz.c.mem_write(hunter + 812, struct.pack("<H", clip))
        for drawn in (0, 1):
            fz.c.mem_write(hunter + 18, bytes([drawn]))
            for blending in (0, 1):
                fz.put(hunter + 516, blending)
                # The Vita mount sees the clock after the displayed pose was
                # sampled. Compare full source ticks; the authored key between
                # them is exercised through the actual GU part update below.
                for f in range(0, length, 1 if blending else 2):
                    frame = struct.pack("<f", f)
                    fz.c.mem_write(hunter + 476, struct.pack("<f", f + 2))
                    grip = fz.call(0x8121db38, hunter)
                    g.run(socket, result, actor, 0x10000 | clip, drawn,
                          int.from_bytes(frame, "little"), 0x3f800000 if blending else 0)
                    observed = struct.unpack("<I8f", g.cpu.mem_read(result, 36))
                    expected = source_table[grip]
                    assert all(abs(a - b) < 1e-4 for a, b in zip(observed, expected)), (
                        clip, f, drawn, blending, grip, observed, expected)
                    assert g.cpu.mem_read(result + 36, 1)[0] == (grip in (0, 1))
                    samples += 1
    print(f"{samples} packaged socket samples matched the Vita selector", flush=True)


def handoff_frames(g, actor, part, encode, update_part):
    # First incoming w13w key, outgoing joint, incoming joint. Preserve the
    # last outgoing sample right up to the cut, also on a repeated render.
    cuts = {
        1003: [(74, 8, 2)], 1005: [(84, 8, 2)], 1008: [(19, 8, 2)],
        1009: [(40, 8, 2)], 1016: [(12, 8, 12), (54, 12, 8)], 1023: [(18, 2, 8)],
        1035: [(103, 12, 8)], 1043: [(11, 8, 12), (140, 12, 8)],
        1061: [(9, 12, 8)], 1081: [(109, 12, 8)], 1083: [(28, 12, 8)],
        1402: [(13, 12, 8)], 1406: [(31, 8, 12)], 1416: [(26, 12, 8)],
        1417: [(20, 12, 8)], 1440: [(24, 2, 8)], 1446: [(21, 8, 12)],
        1462: [(30, 8, 2)], 1463: [(38, 8, 2)], 1467: [(21, 12, 8)],
        1468: [(33, 8, 12)], 1470: [(10, 12, 8)],
    }
    equipment = g.get(actor + 5060)
    original_sex = bytes(g.cpu.mem_read(equipment + 1240, 1))
    try:
        for sex in (0, 1):
            g.cpu.mem_write(equipment + 1240, bytes([sex]))
            g.cpu.mem_write(actor + 10057, bytes([1]))
            g.put(actor + 1268, 0)
            for clip, switches in cuts.items():
                g.put(actor + 1204, g.run(encode, actor, 0x10000 | clip))
                for cut, previous, following in switches:
                    for delta in (-2., -1.5, -1., -.75, -.5, -.25, 0., 0., .5, 1.):
                        frame = cut + delta
                        g.cpu.mem_write(actor + 1280, struct.pack("<f", frame))
                        g.run(update_part, part, actor, 16)
                        sampled, = struct.unpack("<f", g.cpu.mem_read(part + 1276, 4))
                        assert g.get(part + 5068) == (previous if delta < 0 else following), (
                            sex, clip, frame, g.get(part + 5068))
                        assert sampled == (cut - 1 if -1 < delta < 0 else frame), (
                            sex, clip, frame, sampled)
    finally:
        g.cpu.mem_write(equipment + 1240, original_sex)
    print(f"{sum(map(len, cuts.values()))} weapon handoffs matched on both skeletons including fractional frames")


def loop_frames(g, actor, part, encode, update_part):
    """Compare both native clocks across two wraps, including the complete part update."""
    fz = Machine(REPO / "mhfz/eboot.bin.elf", True)
    owner, rig, joint, requests, track = [MEM + i for i in (0x1000, 0x5000, 0x7000, 0x9000, 0xA000)]
    fz.put(owner + 1656, rig)
    fz.put(owner + 1664, rig)
    fz.put(rig + 36, joint)
    fz.put(rig + 68, joint)
    fz.put(rig + 84, joint)
    fz.put(joint + 208, joint)
    fz.put(requests + 4, track)
    fz.put(0x83AED500 + 72, 1)
    fz.c.mem_write(owner + 834, struct.pack("<H", 1))
    # Keep both playhead functions intact. Stub only FK and root-motion callbacks.
    fz.trap(0x8182D384, lambda actor_ptr, flag, *_: int(flag == 0x4000000))
    for pc in (0x8182D39E, 0x81008020, 0x81007BCE):
        fz.trap(pc, lambda *_: 0)
    body_clips, parts = durations(), durations(part=True)
    saved = [(addr, bytes(g.cpu.mem_read(addr, size))) for addr, size in ((actor, 14000), (part, 5152))]
    count = samples = 0
    g.cpu.hook_del(g.traps.pop(0x2A2444))  # Exercise the actual drawn flag.
    try:
        g.put(actor + 1188, 1)
        handoff_frames(g, actor, part, encode, update_part)
        blend_frames(g, actor, part, encode, update_part, fz, owner, rig, requests, body_clips)
        fz.put(requests + 4, track)
        for clip, (length, restart) in body_clips.items():
            if restart < 0:
                continue
            part_len, part_restart = parts[clip]
            fz.c.mem_write(owner + 476, struct.pack("<4fI", 0., 2., restart, length - 1, 1))
            fz.c.mem_write(track + 8, struct.pack("<5f", 0., 0., part_restart, part_len - 1, 0.))
            fz.put(track + 24, 1)
            packed = g.run(encode, actor, 0x10000 | clip)
            for delta in (2, 1):
                fz.c.mem_write(owner + 476, bytes(4))
                fz.c.mem_write(track + 8, bytes(4))
                g.put(part + 1204, 0xffff)
                g.put(actor + 1204, packed)
                g.put(actor + 1188, 1)
                g.put(actor + 1212, 0)
                # GU 952C7C sets end to the inclusive file index, loop to header[3].
                g.cpu.mem_write(actor + 1268, struct.pack("<7f", 0., 1., 0., 0., length+1, restart, 1.))
                g.cpu.mem_write(actor + 28, struct.pack("<f", delta))
                for elapsed in range(0, length + 2 * max(length-restart, part_len-part_restart) + 4, delta):
                    g.run(0x951A80, actor)
                    g.run(update_part, part, actor, 16)
                    if elapsed % 2:
                        continue
                    source = (struct.unpack("<f", fz.c.mem_read(owner + 476, 4))[0],
                              struct.unpack("<f", fz.c.mem_read(track + 8, 4))[0])
                    result = (struct.unpack("<f", g.cpu.mem_read(actor + 1280, 4))[0],
                              struct.unpack("<f", g.cpu.mem_read(part + 1276, 4))[0])
                    assert result == source, (clip, elapsed, delta, result, source)
                    fz.call(0x8173208A, owner)
                    fz.call(0x81732D8A, owner, requests, 1)
                    samples += 1
            count += 1
    finally:
        g.stub(0x2A2444, lambda *_: 1)
        for addr, data in saved:
            g.cpu.mem_write(addr, data)
    print(f"{count} looping hunter/part pairs, {samples} native samples matched at 30/60 Hz")


def blend_frames(g, actor, part, encode, update_part, fz, owner, rig, requests, clips):
    program = (ROOT / "target/assets/magnet_spike.fmsa").read_bytes()
    entries = set()
    for state in range(165):
        n, at = struct.unpack_from("<2I", program, 16 + 24 * state)
        for op, clip, start, blend, _, _ in struct.iter_unpack("<6H", program[at:at + n*12]):
            if op != 2 and blend >= 2:
                entries.add((clip, start, blend))
    arm = g.get(api_for(g, b"mhgu::WeaponPartMotion", b"WPMT") + 4)
    native_track = 0x83AED500 + 108
    fz.put(0x82C3B320, MEM + 0x20000)
    fz.put(requests + 4, native_track)
    fz.c.mem_write(owner, bytes([1]))
    fz.c.mem_write(owner + 3, bytes([13]))
    for pc in (0x8181DD02, 0x8182B166, 0x81D8D28C, 0x81D8F27A, 0x81007FA2, 0x81002782, 0x81007D54):
        fz.trap(pc, lambda *_: 0)
    fz.trap(0x818173BE, lambda *_: 13)
    fz.trap(0x8182BB66, lambda *_: 0)
    def frame(addr):
        return struct.unpack("<f", fz.c.mem_read(addr, 4))[0]
    def gu(addr):
        return struct.unpack("<f", g.cpu.mem_read(addr, 4))[0]
    for clip, start, blend in sorted(entries):
        length, restart = clips[clip]
        slot = clip % 100
        fz.put(0x83AF3640 + 4 * slot, rig)
        fz.put(0x83AF3CD0 + 4 * slot, rig)
        fz.trap(0x81007FF2, lambda *_: fz.c.reg_write(UC_ARM_REG_S0, struct.unpack("<I", struct.pack("<f", length-1))[0]))
        fz.trap(0x81007FFE, lambda *args: int(restart >= 0))
        fz.c.mem_write(owner + 812, struct.pack("<H", clip))
        for drawn in (0, 1):
            fz.c.mem_write(owner + 18, bytes([drawn]))
            fz.put(owner + 476, 0)
            fz.c.mem_write(owner + 480, struct.pack("<f", 2.))
            fz.call(0x817315F8, owner, start, blend // 2, 0)
            fz.call(0x8121E57E, owner, start, blend // 2)
            ticks, = struct.unpack("<I", fz.c.mem_read(owner + 520, 4))
            assert ticks == blend // 2 + 1
            packed = g.run(encode, actor, 0x10000 | clip)
            g.put(actor + 1204, packed | 0x01020000)
            g.put(actor + 1212, 4)
            g.cpu.mem_write(actor + 10057, bytes([drawn]))
            g.cpu.mem_write(actor + 28, struct.pack("<f", 2.))
            g.cpu.mem_write(actor + 1264, struct.pack("<8f", 2*ticks, 2*ticks, 1., start, start,
                                                      length-1, max(0,restart), 1.))
            g.run(arm, actor, packed, struct.unpack("<I", struct.pack("<f", 2*ticks))[0])
            for tick in range(ticks + 3):
                part_blend = bool(fz.c.mem_read(native_track + 48, 1)[0])
                expected_part = start if part_blend else frame(native_track + 8)
                g.run(g.hooks[0x951A80], actor)
                g.run(update_part, part, actor, 16)
                fz.call(0x8173208A, owner)
                fz.call(0x81732D8A, owner, requests, 1)
                remaining, = struct.unpack("<I", fz.c.mem_read(owner + 520, 4))
                assert gu(actor + 1268) == 2 * remaining, (clip, start, tick, remaining)
                expected_body = start if remaining else frame(owner + 476)
                assert gu(actor + 1276) == expected_body, (clip, start, tick, expected_body)
                assert gu(part + 1276) == expected_part, (
                    clip, start, tick, drawn, gu(part + 1276), expected_part)
    print(f"{len(entries)} distinct blended steps matched the Vita body and part clocks")


def event_frames(fz, hunter, lengths, program, output):
    data, bank, tables, callback = [MEM + n for n in (0x10000, 0x12000, 0x20000, 0xf000)]
    fz.put(0x82BB48C0, data)
    fz.put(data + 0x614, bank)
    fz.put(bank + 13 * 8, 165)
    fz.put(bank + 13 * 8 + 4, tables + 16)
    fz.c.mem_write(tables, program)
    fz.trap(callback, lambda *_: 0)
    count = states = 0
    for state in range(165):
        desc = 16 + 24 * state
        steps, commands = struct.unpack_from("<II", program, desc)
        total, events = struct.unpack_from("<II", program, desc + 16)
        fz.put(tables + desc + 20, tables + events)
        if not steps:
            continue
        states += 1
        fz.c.mem_write(hunter + 20, bytes([state]))
        fz.c.mem_write(hunter + 820, b"\1")
        for speed in (1, 2):
            fz.c.mem_write(hunter + 3994, b"\0\0")
            seen = 0
            for step in range(steps):
                op, clip, start, blend, until, cycles = struct.unpack_from(
                    "<6H", program, commands + 12 * step)
                if op == 2:
                    continue  # Inline state transition.
                fz.c.mem_write(hunter + 3990, struct.pack("<H", step + 1))
                length, restart = lengths[clip]
                passes = cycles + 1 if op == 4 else max(cycles, 1)
                if op == 5 and restart >= 0:
                    passes = 2  # Exercise an open hold through a full loop.
                for cycle in range(passes):
                    fz.c.mem_write(hunter + 3992, struct.pack("<H", cycle))
                    first = start if cycle == 0 else max(0, restart)
                    end = min(length - 1, until) if op == 4 and cycle == cycles else length - 1
                    ticks = blend // 2 + 1 if cycle == 0 and blend >= 2 else 0
                    samples = [(first, remaining) for remaining in range(ticks, 0, -1)]
                    samples.extend((frame, 0) for frame in range(first + (speed if ticks else 0), max(end, first) + 1, speed))
                    for frame, remaining in samples:
                        fz.put(hunter + 516, int(remaining > 0))
                        fz.put(hunter + 520, remaining)
                        fz.c.mem_write(hunter + 476, struct.pack("<f", frame))
                        fz.call(0x81816422, hunter, callback | 1)
                        seen, = struct.unpack("<H", fz.c.mem_read(hunter + 3994, 2))
                        output.write(struct.pack("<6H", state, step, cycle, frame, remaining, seen))
                        count += 1
            assert seen == total, (state, seen, total)
            output.write(bytes([255]) * 12)
    print(f"{count} Vita clock samples across {states} action states", flush=True)


def contact_frames(fz, trace):
    gu = Machine(REPO / "mhgu/main.elf", False)
    hit = MEM + 0x2000
    fz.put(0x83AC1AA8, hit)
    fz.trap(0x8166D520, lambda *_: 0)
    gu.put(0x2126764, MEM)
    gu.c.mem_write(MEM + 104, struct.pack("<f", 2.))
    windows = list(struct.iter_unpack("<6H", trace.read_bytes()))
    for state, row, delay, duration, gu_delay, gu_duration in windows:
        fz.c.mem_write(hit, bytes(352))
        fz.c.mem_write(hit, b"\1")
        fz.c.mem_write(hit + 15, b"\1")
        fz.c.mem_write(hit + 104, struct.pack("<2H", delay // 2, duration // 2))
        gu.c.mem_write(hit + 5, b"\1")
        gu.c.mem_write(hit + 92, struct.pack("<2f", gu_delay, gu_duration))
        for tick in range((delay + duration) // 2 + 2):
            fz.call(0x817006A6)
            gu.call(0x16919C, hit)
            assert bool(gu.call(0x169328, hit)) == (fz.c.mem_read(hit + 15, 1)[0] == 2), (
                state, row, tick, delay, duration)
    assert len(windows) == 132
    print(f"{len(windows)} melee and finisher windows matched through expiration")


if __name__ == "__main__":
    lengths = durations()
    fz = Machine(REPO / "mhfz/eboot.bin.elf", True)
    hunter = MEM + 0x4000
    fz.c.mem_write(hunter, b"\1")
    fz.c.mem_write(hunter + 3, b"\x0d")
    socket_frames(fz, hunter, lengths)
    program = (ROOT / "target/assets/magnet_spike.fmsa").read_bytes()
    assert program in PLUGIN.read_bytes(), "battle table differs from the packaged NRO"
    with tempfile.TemporaryDirectory() as tmp:
        trace = Path(tmp) / "events"
        with trace.open("wb") as out:
            event_frames(fz, hunter, lengths, program, out)
        contacts = Path(tmp) / "contacts"
        subprocess.run(
            ["cargo", "test", "--manifest-path", "crates/mhfz-sdk/Cargo.toml",
             "--test", "magnet_clock", "--", "--ignored"],
            cwd=REPO, env={**os.environ, "MAGNET_EVENT_TRACE": str(trace),
                           "MAGNET_CONTACT_TRACE": str(contacts)}, check=True)
        contact_frames(fz, contacts)
