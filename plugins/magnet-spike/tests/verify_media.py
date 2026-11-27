"""Exercise every packaged w13 sound row and every executable motion particle row."""
from collections import defaultdict, Counter
from pathlib import Path
import struct
import subprocess

from unicorn.arm_const import UC_ARM_REG_SP

ROOT = Path(__file__).resolve().parents[1]
ASSETS = ROOT / "target/assets"


def verify_audio(g, actor, sounds, contact):
    symbols = subprocess.check_output(["nm", str(ROOT / "target/armv7a-none-eabi/release/libmagnet_spike.so")], text=True)
    address, = [int(line.split()[0], 16) + 0x9200000 for line in symbols.splitlines()
                if line.split()[-1].endswith("5sound6sample")]
    clock, pose = g.alloc(16), g.alloc(24)
    tracks = (ASSETS / "frontier_ms_sound.flse").read_bytes()
    rolls = [0]
    rng = 0x27C8F8
    g.stub(rng, lambda *_: rolls[0])
    equipment = g.get(actor + 5060)
    old_sex = bytes(g.cpu.mem_read(equipment + 1240, 1))
    tested, clips = 0, 0
    for index in range(500):
        offset, count = struct.unpack_from("<HH", tracks, 4 * index)
        if not count:
            continue
        clip = 1000 + index
        clips += 1
        by_frame = defaultdict(list)
        for i in range(count):
            frame, bank, variants, *pairs = struct.unpack_from("<H10B", tracks, offset + 12 * i)
            by_frame[frame].append((bank, list(zip(pairs[::2], pairs[1::2]))[:variants]))
        for frame, events in by_frame.items():
            total = max(sum(weight for _, weight in choices) for _, choices in events)
            for roll in range(total):
                rolls[0] = roll
                for sex in (0, 1):
                    g.cpu.mem_write(equipment + 1240, bytes([sex]))
                    g.cpu.mem_write(clock, bytes(16))
                    first, floor = len(sounds), len(contact)
                    g.cpu.mem_write(pose, struct.pack("<5fI", frame, frame + 1, 999., 0., 1., 0))
                    g.run(address, clock, actor, clip, pose)
                    assert (len(sounds), len(contact)) == (first, floor)
                    g.put(pose + 16, 0)
                    g.run(address, clock, actor, clip, pose)
                    g.run(address, clock, actor, clip, pose)
                    expected, native = [], []
                    for bank, choices in events:
                        draw = roll % sum(weight for _, weight in choices)
                        for cue, weight in choices:
                            if draw < weight:
                                break
                            draw -= weight
                        if cue == 255:
                            continue
                        if bank in (3, 4):
                            native.append((9, cue) if bank == 3 else (5, 2 if cue == 4 else 0))
                        else:
                            name = ("blade", "common", "weapon")[bank]
                            if bank == 2 and cue >= 32:
                                name = ("voice_m", "voice_f")[sex]
                            expected.append((name, cue))
                    assert [(b, c) for _, b, c, _ in sounds[first:]] == expected, (clip, frame, roll, sex, expected)
                    assert contact[floor:] == native, (clip, frame, native)
            tested += len(events)
    g.cpu.mem_write(equipment + 1240, old_sex)
    g.cpu.hook_del(g.traps.pop(rng))
    assert tested == 1313, tested
    print(f"{tested} sound rows in {clips} clips matched both actors, all weighted choices, blend and repeat", flush=True)


def verify_gauge(texture):
    width = (struct.unpack_from("<I", texture, 8)[0] >> 6) & 8191
    def pixel(frame, x, y):
        x = ((frame % (width // 192)) * 192 + x) * 4
        y += frame // (width // 192) * 32
        gob = (y // 128 * (width // 16) + x // 64) * 16 + y // 8 % 16
        at = 24 + 512 * gob + x % 64 // 32 * 256 + y % 8 // 2 * 64 + x % 32 // 16 * 32 + y % 2 * 16 + x % 16
        return tuple(texture[at:at+4])
    # B6 near/far/default share all texels and alpha; only their retail tint changes.
    for frame, tint in ((547, (11, 153, 255)), (548, (198, 41, 0))):
        for y in range(32):
            for x in range(32):
                base = pixel(507, x, y)
                ink = pixel(frame, x, y)
                assert ink[3] == base[3]
                assert all(abs(v - s * t / 255) <= 1 for v, s, t in zip(ink, base, tint))
        assert any(pixel(frame, x, y) == (*tint, 255) for y in range(32) for x in range(32))
    phases = Counter()
    for frame in range(531, 547):
        for y in range(10):
            for x in range(17):
                r, g, b, a = pixel(frame, x, y)
                if a:
                    assert max(r, g, b) > 1
                    base = pixel(100, x + 22, y + 11)
                    assert r >= base[0]
                    phases[frame] += 1
    assert 531 not in phases and phases


def verify_effect_tracks(g, sounds):
    symbols = subprocess.check_output(["nm", str(ROOT / "target/armv7a-none-eabi/release/libmagnet_spike.so")], text=True)
    address, = [int(line.split()[0], 16) + 0x9200000 for line in symbols.splitlines()
                if line.split()[-1].endswith("2fx6motion")]
    actor, vt, parent, pose, clock, latch, joint, unit, handle = (
        g.alloc(size) for size in (14000, 1600, 4, 24, 16, 4, 64, 416, 8))
    g.put(actor, vt)
    g.put(vt + 304, parent)
    g.stub(parent, lambda obj, *_: obj + 600)
    g.put(actor + 1172, joint)
    g.put(actor + 1176, joint)
    for key in (0x188A0D4, 0x2126F48, 0x21270E0):
        g.put(key, g.alloc(64))
    g.put(handle + 4, 1)
    selectors = {(b, s): i for b, s, i in struct.iter_unpack("<3H", (ASSETS / "frontier_ms_selectors.bin").read_bytes())}
    routes = list(struct.iter_unpack("<2H", (ASSETS / "frontier_ms_routes.bin").read_bytes()))
    rows = defaultdict(lambda: defaultdict(list))
    for clip, frame, *event in struct.iter_unpack("<5H2B3h", (ASSETS / "frontier_ms_motion_fx.bin").read_bytes()):
        rows[clip][frame].append(event)
    def ground(*_):
        sp = g.cpu.reg_read(UC_ARM_REG_SP)
        g.cpu.mem_write(g.get(sp + 4), struct.pack("<f", 0.))
        g.cpu.mem_write(g.get(sp + 8) + 20, struct.pack("<f", 2. if wet else -100001.))
        return 1
    layers = []
    def spawn(_, unit, handle, group):
        layers.append(g.get(g.cpu.reg_read(UC_ARM_REG_SP)))
        return 1
    for addr, fn in [(0xC1628, lambda *_: joint), (0x18192C, ground), (0x3288C4, lambda *_: unit),
                     (0x328900, lambda *_: 0), (0x3267D0, spawn), (0x3268D8, lambda *_: handle),
                     (0x326C78, lambda *_: 0)]:
        if addr in g.traps:
            g.cpu.hook_del(g.traps.pop(addr))
        g.stub(addr, fn)
    g.cpu.mem_write(joint, struct.pack("<16f", 1,0,0,0, 0,1,0,0, 0,0,1,0, 0,0,0,1))
    for wet in (False, True):
        for clip, frames in rows.items():
            g.cpu.mem_write(clock, bytes(16))
            g.put(latch, 0)
            first = len(sounds)
            accent = False
            for frame, events in frames.items():
                expected = []
                for kind, bank, selector, node, mode, *offset in events:
                    if kind == 1:
                        route = 46 if wet else (len(routes)-1 if bank == 14 else 48 if bank in (7, 12) else 47)
                    else:
                        route = selectors[bank + int(wet and kind == 5), selector]
                    start, count = routes[route]
                    expected.extend(range(start, start + count))
                    accent |= bank == 4037 and selector in (3, 4, 70, 71) and node in (14, 18)
                layers.clear()
                g.cpu.mem_write(pose, struct.pack("<5fI", frame, frame + 1, 999., 0., 1., 0))
                g.run(address, clock, latch, actor, clip, pose)
                assert not layers
                g.put(pose + 16, 0)
                g.run(address, clock, latch, actor, clip, pose)
                g.run(address, clock, latch, actor, clip, pose)
                assert layers == expected, (wet, clip, frame, layers, expected)
            assert [s[2] for s in sounds[first:] if s[1]=='blade'] == [13] * int(accent), (wet, clip)
    print("962 motion FX rows in 167 clips matched on dry ground and water, including once-per-action wrist sounds", flush=True)
