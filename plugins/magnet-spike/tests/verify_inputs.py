"""Replay the physical pad, retail combo resolver, early GU action table and w13 dispatcher."""
import itertools
import struct
import zlib

from verify_battle import A, B, R, SPECIAL, X, Y, ZL, ZR, replay
from verify_style import COMMAND_UPDATE, DISPATCH, quest_entries


def native_ground_table(g, suffix=b"_006d"):
    row = next(zlib.decompress(raw) for name, kind, raw in quest_entries("w07_m0.arc")
               if kind == 0x5139901D and name.endswith(b"plcmd_com_0x0001" + suffix))
    table, entries, entry, resource = [g.alloc(n) for n in (24, 4, 24, 124)]
    g.cpu.mem_write(table, struct.pack("<6I", 0, 0, 1, 1, 0, entries))
    g.put(entries, entry)
    g.cpu.mem_write(entry, struct.pack("<3I3f", 0, 1, resource, 0, 0, 999))
    fields = [struct.unpack_from("<H", row, at)[0] for at in (504, 510, 516, 522)]
    parameters = [struct.unpack_from("<I", row, at)[0] for at in (528, 536, 544, 552)]
    g.cpu.mem_write(resource + 100, struct.pack("<4H4I", *fields, *parameters))
    return table


class NativePad:
    def __init__(self, g, actor):
        self.g, self.actor, self.last = g, actor, 0
        if not g.get(0x188CA48):  # GU quest session read by 27ECCC.
            g.put(0x188CA48, g.alloc(512))
        vt = g.get(actor)
        for query in (0x18E50, 0x55FD88, g.get(vt + 384), g.get(vt + 392), 0x65D9E8):  # Offline touch/menu queries.
            if query not in g.traps:
                g.stub(query, lambda *_: 0)
        self.pad = g.get(0x188C670)
        self.settings = g.get(0x1897F78)
        if not self.settings:
            self.settings = g.alloc(17600)
            g.put(0x1897F78, self.settings)

    def poll(self, held, *, elapsed=2., strength=0):
        g, actor = self.g, self.actor
        g.cpu.mem_write(actor + 28, struct.pack("<f", elapsed))
        g.cpu.mem_write(self.pad + 27548, struct.pack("<III", held, self.last & ~held, held & ~self.last))
        g.put(self.pad + 27568, strength)
        self.last = held
        g.run(0x2A79CC, actor)
        assert g.get(actor + 5116) == held
        return g.get(actor + 5140)

    def dispatch(self):
        self.g.run(DISPATCH, self.actor)


def verify_inputs(g, actor, equipment, scratch, set_action, changes):
    pad = NativePad(g, actor)
    g.put(0x2126504, g.alloc(128))
    table = native_ground_table(g)
    g.put(actor + 1188, 1)
    g.cpu.hook_del(g.traps.pop(0x27DB78))
    g.cpu.hook_del(g.traps.pop(0x65FA04))
    g.cpu.hook_del(g.traps.pop(0x2A2444))
    g.cpu.mem_write(actor + 5088, struct.pack("<2f", 1000., 1000.))
    g.cpu.mem_write(actor + 10500, struct.pack("<f", 1.))

    def state():
        return g.half(actor + 9478), g.half(actor + 9482)

    def ground(*, drawn=True, walk=False):
        set_action(1, 0)
        g.put(actor + 9384, 0)
        pad.poll(0)
        pad.dispatch()
        set_action(2 if walk else 1, 30 if walk and drawn else 0)
        g.cpu.mem_write(actor + 10057, bytes([drawn]))
        g.cpu.mem_write(actor + 5, b"\x01")
        g.cpu.mem_write(actor + 9392, bytes(48))
        g.put(actor + 9384, table)
        changes.clear()

    # Same retail action table with an ordinary weapon and with w13 selected.
    g.put(actor + 5060, scratch)
    ground()
    native = pad.poll(B | ZL)
    assert native & (1 << 3), hex(native)
    g.run(COMMAND_UPDATE, actor)
    assert state() == (1, 6), (state(), hex(native))
    ground()
    assert pad.poll(R | X) & 0x80000001
    g.put(actor + 5060, equipment)
    for seq in ((ZL | B,), (B, ZL | B), (ZL, ZL | B)):
        ground()
        for keys in seq:
            assert pad.poll(keys) & (1 << 3) == 0
            g.run(COMMAND_UPDATE, actor)
            pad.dispatch()
            assert state() == (1, 0), (seq, state())
        assert not changes  # No marked target, no native roll either.

    def attack(seq, expected, *, drawn=True, walk=False, elapsed=2., impact=False, strength=0):
        set_action(1, 0)
        g.put(actor + 5060, scratch)
        pad.dispatch()
        g.put(actor + 5060, equipment)
        g.cpu.mem_write(actor + 5088, struct.pack("<2f", 1000., 1000.))
        ground(drawn=drawn, walk=walk)
        if impact:
            pad.poll(SPECIAL)
            pad.dispatch()
            pad.poll(0)
            pad.dispatch()
            assert state() == (16, 0x8000 + 90)
            g.cpu.mem_write(actor + 1206, bytes([4]))
            g.cpu.mem_write(actor + 1280, struct.pack("<f", 99.))
            pad.poll(0)
            pad.dispatch()
            g.cpu.mem_write(actor + 1206, bytes(1))
            g.cpu.mem_write(actor + 1280, bytes(4))
            ground(walk=walk)
        for keys in (*seq, *([seq[-1]] * 8)):
            native = pad.poll(keys, elapsed=elapsed, strength=strength)
            if drawn:
                assert native & (0x80000001 | 1 << 3 | 1 << 13) == 0, (seq, hex(native))
            if g.get(actor + 9384):
                g.run(COMMAND_UPDATE, actor)
            pad.dispatch()
            assert state() not in ((1, 2), (1, 3), (1, 6)), (seq, state())
            if state()[0] == 16 or state() == (1, 1):
                break
        if expected == (1, 1):
            assert state() == expected and all(c[0] != 16 for c in changes), changes
            return
        assert state() == (16, 0x8000 + expected), (seq, expected, state(), changes)
        assert len([c for c in changes if c[0] == 16]) == 1, changes

    # Retail tables keep a separate acceptance byte per row. A zeroed pad
    # alone cannot revoke it. Also feed a command arriving after 2A79CC.
    def stale_ground(seq, suffix, command, expected, *, impact=False):
        if impact:
            attack((SPECIAL,), 90)
            g.cpu.mem_write(actor + 1206, bytes([4]))
            g.cpu.mem_write(actor + 1280, struct.pack("<f", 99.))
            pad.poll(0)
            pad.dispatch()
            g.cpu.mem_write(actor + 1206, bytes(1))
        ground()
        g.put(actor + 9384, native_ground_table(g, suffix))
        g.cpu.mem_write(actor + 9392, bytes([1]))
        for keys in (*seq, seq[-1], seq[-1]):
            pad.poll(keys)
            latched = g.cpu.mem_read(actor + 9392, 1)[0]
            g.run(0x285DE4, actor, command)
            g.run(COMMAND_UPDATE, actor)
            assert state() == (1, 0) and not latched, (seq, state(), latched)
            pad.dispatch()
            if state()[0] == 16:
                break
        assert state() == (16, 0x8000 + expected), (seq, state())

    stale_ground((R | Y,), b"_002a", 13, 116, impact=True)
    stale_ground((B, R | B), b"_006d", 4, 158)
    # Start with an actual full gauge, spend all five segments without a quest reset.
    attack((R | X,), 11)
    for _ in range(4):
        ground()
        pad.poll(R | Y)
        pad.dispatch()
        assert state() == (16, 0x8000 + 107)
    for key, suffix, command in ((Y, b"_002a", 13), (B, b"_006d", 4),
                                 (A, b"_002a", 13), (X, b"_002a", 13)):
        ground()
        g.put(actor + 9384, native_ground_table(g, suffix))
        g.cpu.mem_write(actor + 9392, bytes([1]))
        for _ in range(4):
            pad.poll(R | key)
            g.run(0x285DE4, actor, command)
            g.run(COMMAND_UPDATE, actor)
            pad.dispatch()
            assert state() == (1, 0), (key, state())
    for layout in (0, 1):
        g.cpu.mem_write(pad.settings + 17516, bytes([2]))  # Shoulder/face Hunter Arts.
        g.cpu.mem_write(pad.settings + 17518, bytes([layout]))
        for elapsed in (2., 1.):
            for seq in ((R | X,), (R, R | X), (X, R | X)):
                attack(seq, 11, walk=True, elapsed=elapsed)
        for keys, expected in ((R | A, 108), (R | Y, 107), (R | B, 106), (ZL | A, 82)):
            modifier = keys & (R | ZL)
            face = keys & ~modifier
            for seq in ((keys,), (modifier, keys), (face, keys)):
                attack(seq, expected)
        for order in itertools.permutations((R, X, A)):
            seq = (order[0], order[0] | order[1], R | X | A)
            attack(seq, 12)
            attack(seq, 112, drawn=False)
        for seq in ((X | A,), (X, X | A), (A, X | A)):
            attack(seq, 2)
            attack(seq, 79, drawn=False)
        attack((X,), (1, 1), drawn=False)
        attack((X,), 78, drawn=False, strength=80)
        attack((X,), 78, drawn=False, walk=True, strength=80)
        ground()
        g.put(actor + 5180, 0)
        pad.poll(ZL | X | A | B)
        assert g.get(actor + 5180) & 7 == 0
    for face, slash, impact in ((X, 0, 18), (A, 6, 32), (X | A, 2, 26), (ZR, 14, 112),
                               (R | X, 11, 39), (R | A, 108, 117), (R | B, 106, 158),
                               (R | Y, 107, 116), (R | X | A, 12, 38), (ZL | A, 82, 84)):
        attack((face,), slash)
        attack((face,), impact, impact=True)
    attack((A,), 23, impact=True, strength=75)
    attack((X, R | X), 11, elapsed=0.5)
    for layout, special in ((0, 0x4000), (1, 0x2000)):
        g.cpu.mem_write(pad.settings + 17517, bytes([layout]))
        attack((special,), 90)
        for seq in ((R, R | special), (special, R | special), (R | special,)):
            ground()
            for keys in (*seq, 0):
                pad.poll(keys)
                pad.dispatch()
            assert not changes, (seq, changes)  # Neither a mode switch nor an unearned boost.
    for key, target, flag, row in ((B, 6, 3, b"_006d"), (Y, 2, 13, b"_002a")):
        ground()
        g.put(actor + 9384, native_ground_table(g, row))
        assert pad.poll(key) & (1 << flag) == 0
        g.run(COMMAND_UPDATE, actor)
        assert state() == (1, 0)
        assert pad.poll(0) & (1 << flag)
        g.run(COMMAND_UPDATE, actor)
        assert state() == (1, target)
    ground(drawn=False)
    g.cpu.mem_write(pad.settings + 17518, bytes(1))
    assert pad.poll(R) & 0x80000001 == 0x80000001
    # State 0 accepts X through frame 105 and closes both windows at 106.
    for first, expected in ((105., (16, 0x8003)), (106., (1, 0))):
        attack((X,), 0)
        g.cpu.mem_write(actor + 1280, struct.pack("<f", 10.))
        pad.poll(0)
        pad.dispatch()
        for index in range(3):
            g.cpu.mem_write(actor + 1280, struct.pack("<f", first + index))
            g.cpu.mem_write(actor + 1206, bytes([4 * (index > 0)]))
            pad.poll(X)
            pad.dispatch()
        assert state() == expected, (first, state())
        g.cpu.mem_write(actor + 1206, bytes(1))
    def motion_sample(frame, keys=0, ended=False):
        g.cpu.mem_write(actor + 1280, struct.pack("<f", frame))
        g.cpu.mem_write(actor + 1206, bytes([4 * ended]))
        pad.poll(keys)
        pad.dispatch()
        return g.half(actor + 1204)

    # Reach the handoffs via physical attacks. Their visual samples run in
    # verify_swing_frames against both weapon and hunter tracks.
    attack((X,), 0)
    for frame, keys, ended, clip in (
        (10, 0, False, 1049), (30, X, False, 1049), (32, 0, False, 1049),
        (64, 0, False, 1401), (12, X, False, 1401), (13, 0, False, 1401),
        (14, 0, True, 1050), (40, 0, False, 1047), (20, X | A, False, 1047),
        (22, 0, False, 1047), (42, 0, False, 1406), (42, 0, True, 1035),
    ):
        assert motion_sample(frame, keys, ended) == clip, (frame, keys, clip, state())
    attack((A,), 6)
    for frame, keys, ended, clip in (
        (0, 0, False, 1048), (52, A, False, 1048), (54, 0, False, 1048),
        (66, 0, True, 1051), (66, 0, False, 1041), (30, A, False, 1041),
        (32, 0, False, 1041), (56, 0, False, 1043),
    ):
        assert motion_sample(frame, keys, ended) == clip, (frame, keys, clip, state())
    attack((A,), 32, impact=True)
    for frame, keys, clip in ((0, 0, 1072), (12, A, 1072), (14, 0, 1072),
                              (52, 0, 1078), (20, A, 1078), (22, A, 1078),
                              (70, A, 1080)):
        assert motion_sample(frame, keys) == clip, (frame, keys, clip, state())
    # The two forward attacks have different bind windows and impact changes
    # clips first. Exercise the same delayed recognition inside both real rows.
    for impact, opener, accept, execute, target in (
        (False, 11, 44., 72., 97), (True, 39, 24., 66., 94)
    ):
        attack((R | X,), opener, impact=impact)
        if impact:
            g.cpu.mem_write(actor + 1280, struct.pack("<f", 28.))
            pad.poll(0)
            pad.dispatch()
            assert g.half(actor + 1204) == 1424
        for frame, keys in ((accept, 0), (accept, A), (accept + 1, A | X),
                            (accept + 2, R | A | X), (execute - 1, 0), (execute, 0)):
            g.cpu.mem_write(actor + 1280, struct.pack("<f", frame))
            pad.poll(keys)
            pad.dispatch()
            assert state() == (16, 0x8000 + (target if frame == execute else opener)), (
                opener, frame, state()
            )
    # GU 27E970 owns the ordinary L item panel. On the swapped layout the
    # same native timer belongs to physical ZL, which w13 uses for aim.
    for layout, key in ((0, 0x400), (1, ZL)):
        ground()
        g.cpu.mem_write(pad.settings + 17518, bytes([layout]))
        for _ in range(13):
            pad.poll(key)
            pad.dispatch()
        if layout:
            assert g.get(actor + 5172) & 0x40 == 0
            pad.poll(key | A)
            pad.dispatch()
            # The preceding impact bind keeps its form when returning to ground.
            assert state() == (16, 0x8000 + 84), state()
        else:
            assert g.get(actor + 5172) & 0x40
            pad.poll(key | X)
            assert g.half(actor + 5112) & X
            pad.dispatch()
            assert state() == (1, 0)
    # The directional art shortcuts survive the w13 filter during both runs,
    # their loops and braking. Let the real GU table consume each shortcut.
    g.cpu.mem_write(pad.settings + 17516, b"\0")
    g.cpu.mem_write(pad.settings + 17519, b"\1")
    g.cpu.hook_del(g.traps.pop(0x2A8B34))
    g.stub(0x2A8B34, lambda *_: int(bool(g.get(actor + 5180) & 7)))
    g.stub(0x2ACBEC, lambda *_: 0)

    def select_art(_, group, slot, flags):
        for ptr, value in ((group, 16), (slot, 85), (flags, 0x40)):
            g.cpu.mem_write(ptr, struct.pack("<H", value))
        return 1

    g.stub(0x2ACB60, select_art)
    for run in (40, 41, 42, 44, 45, 46):
        for key in (0x100000, 0x800000, 0x200000):
            ground()
            set_action(16, 0x8000 + run)
            pad.poll(0, strength=120)
            pad.dispatch()
            pad.poll(key, strength=120)
            assert g.get(actor + 5180) & 7
            pad.dispatch()
            assert state() == (16, 85), (run, key, state())
    g.cpu.hook_del(g.traps.pop(0x2A8B34))
    g.stub(0x2A8B34, lambda *_: 0)
    g.put(actor + 5180, 0)
    g.cpu.mem_write(pad.settings + 17519, b"\0")
    # Opening a GUI while a single is pending cancels it, even on button release.
    ground()
    pad.poll(X)
    pad.dispatch()
    g.cpu.hook_del(g.traps.pop(0x65D9E8))
    g.stub(0x65D9E8, lambda *_: 1)
    assert not pad.poll(0) & (1 << 30)
    pad.dispatch()
    assert state() == (1, 0)
    print("Physical GU input, native ground roll, both shoulder layouts and w13 chords passed")


if __name__ == "__main__":
    replay(inputs_only=True)
