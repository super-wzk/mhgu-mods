"""Exercise w01 with GU's native button mappers, motion encoder and scene dispatcher."""

import struct
import zlib

from verify_port import Scene, float_bits
from verify_style import (
    CHANGE_ACTION, COMMAND_UPDATE, DISPATCH, ENCODE, SNS_DRAW_ROUTE, quest_entries, service,
)

W01 = 0x174A354


def mapped_inputs():
    s = Scene(1, "sword-shield", W01)
    g, a = s.g, s.actor
    for address in (0x1897F78, 0x188C670):
        if not g.get(address):
            g.put(address, g.alloc(30000))
    g.put(0x2126504, g.alloc(128))
    s.stub(0x27E608, lambda *_: int(g.half(a + 5136) >= 50))
    s.stub(0x27F5D0, lambda *_: g.half(a + 5132))
    s.stub(0x53A058, lambda *_: 5001)
    s.stub(0x65F93C, lambda *_: 0x40)
    s.stub(0x65FA04, lambda *_: 0x4000)
    # Replace scene/motion side effects and let the actual GU evaluator and
    # registry decide whether a native or custom action reaches the actor.
    g.cpu.hook_del(g.traps.pop(COMMAND_UPDATE))
    g.cpu.mem_write(a + 5078, struct.pack("<h", 100))
    for offset in (792, 808):
        s.stub(g.get(W01 + offset), lambda *_: 0)

    def change(_, group, slot, _flags):
        s.action(group, slot)
        g.put(a + 9384, 0)
        return 1

    s.stub(g.originals[CHANGE_ACTION], change)
    rows = {
        name.rsplit(b"\\", 1)[-1]: zlib.decompress(raw)
        for name, kind, raw in quest_entries("w01_m0.arc")
        if kind == 0x5139901D
    }

    def stock_table(*names):
        entries, table = g.alloc(4 * len(names)), g.alloc(24)
        g.cpu.mem_write(table, struct.pack("<6I", 0, 0, len(names), len(names), 0, entries))
        for i, name in enumerate(names):
            raw = rows[b"plcmd_w01_0x0010_" + name]
            fields = [struct.unpack_from("<H", raw, off)[0] for off in (504, 510, 516, 522)]
            params = [struct.unpack_from("<I", raw, off)[0] for off in (528, 536, 544, 552)]
            entry, resource = g.alloc(24), g.alloc(124)
            g.cpu.mem_write(resource + 100, struct.pack("<4H4I", *fields, *params))
            g.cpu.mem_write(entry, struct.pack("<3I3f", 0, 1, resource, 0, 0, 998))
            g.put(entries + i * 4, entry)
        return table

    # The Guild idle table 2000 can submit 16:14 or 16:15 before the
    # controller. Its guard table 2023 can submit 16:14 or 16:25.
    idle = stock_table(b"014a", b"006a", b"015a", b"004a", b"012a", b"000a")
    guard = stock_table(b"025a", b"011c", b"014b", b"009a")
    x, a_button, shoulder, special = 0x20, 0x100, 0x40, 0x1000
    for group, stick, sequence, action in [
        (1, None, [x], 0),
        (1, None, [a_button], 13),
        (1, None, [x | a_button], 1),
        (1, None, [x, x | a_button], 1),
        (1, None, [a_button, x | a_button], 1),
        (1, None, [shoulder | a_button], 43),
        (1, None, [shoulder, shoulder | a_button], 43),
        (1, None, [a_button, shoulder | a_button], 43),
        (1, None, [special], 12),
        (32, None, [shoulder, shoulder | x], 12),
        (32, None, [shoulder, shoulder | x | a_button], 14),
        (32, None, [shoulder, shoulder | a_button], 43),
        (32, 0xC000, [shoulder, shoulder | a_button], 47),
    ]:
        s.action(group, 0)
        g.cpu.mem_write(a + 5, b"\1")
        g.put(a + 5132, stick or 0)
        g.put(a + 5136, 100 if stick is not None else 0)
        g.put(a + 9384, idle if group == 1 else guard)
        g.put(a + 9440, 0)
        g.cpu.mem_write(a + 9392, bytes(48))
        g.put(a + 5188, 0)
        g.put(a + 5192, 0)
        g.put(a + 1280, 0)
        previous = 0
        starts = []
        for held in sequence + [sequence[-1]] * 3:
            g.cpu.mem_write(
                a + 5108, struct.pack("<3H", held, previous & ~held, held & ~previous)
            )
            previous = held
            g.put(a + 28, float_bits(1))
            g.cpu.mem_write(a + 5140, bytes(32))
            g.run(0x27CA28, a)
            g.run(0x1169554, a)
            if g.get(a + 9384):
                g.run(COMMAND_UPDATE, a)
                assert g.half(a + 9478) != 16, (group, sequence, g.half(a + 9482))
            g.run(DISPATCH, a)
            slot = g.half(a + 9482)
            if slot >= 0x8000 and (not starts or starts[-1] != slot):
                starts.append(slot)
        assert starts == [0x8000 + action], (sequence, starts)
    print("w01 native command evaluation and dispatch: idle/guard X, A, X+A, R+A and special passed")


def attack_followups():
    s = Scene(1, "sword-shield", W01)
    g, a = s.g, s.actor
    for address in (0x1897F78, 0x188C670):
        if not g.get(address):
            g.put(address, g.alloc(30000))
    s.stub(0x65F93C, lambda *_: 0x40)
    s.stub(0x65FA04, lambda *_: 0x4000)
    s.stub(0x53A058, lambda *_: 5001)
    s.stub(0x539F4C, lambda *_: 0)
    s.stub(0x27C6F4, lambda *_: 0)
    s.stub(0x2A8144, lambda *_: 0)
    s.stub(0x27DB78, lambda *_: 0)  # B's separate contextual scene actions
    grounded = False
    stick = False
    aim = 0xC000
    s.stub(0x2A84D8, lambda *_: int(grounded))
    s.stub(0x27E608, lambda *_: int(stick))
    s.stub(0x27F5D0, lambda *_: aim)
    previous = 0

    def tick(frame, held=0):
        nonlocal previous
        g.put(a + 1280, float_bits(frame))
        g.cpu.mem_write(
            a + 5108, struct.pack("<3H", held, previous & ~held, held & ~previous)
        )
        previous = held
        g.cpu.mem_write(a + 5140, bytes(32))
        g.run(0x27CA28, a)
        g.run(0x1169554, a)
        g.cpu.mem_write(s.inputs, bytes(g.cpu.mem_read(a + 5140, 32)))
        s.tick()
        return g.half(a + 9482) - 0x8000

    def start(state):
        nonlocal previous
        previous = 0
        g.put(a + 5188, 0)
        g.put(a + 5192, 0)
        g.put(a + 5088, float_bits(900))
        s.action(16, 0x8000 + state)
        assert tick(0) == state

    def tap_x(frame):
        # 1169554 waits out the X+A chord before resolving X.
        return [tick(frame + i, 0x20 if i == 0 else 0) for i in range(3)]

    # R+X loops from a sword slash through FZ command 14.
    x, r = 0x20, 0x40
    for first in (x | r, r, x):
        start(0)
        assert tick(10, first) == 0
        assert tick(11, x | r) == 0
        assert tick(40, x | r) == 28, first

    # FZ common 0:145 is the same backstep for neutral B and back+B.
    g.put(a + 5092, float_bits(1500))
    s.stub(0x2864C4, lambda *_: 0)  # no Evasion skill in this replay
    for state, opening in ((0, 24), (2, 40)):
        for stick in (False, True):
            g.put(a + 4076, 0)
            aim = 0x8000
            start(state)
            assert tick(opening - 2, 0x80) == state
            assert tick(opening - 1) == state
            assert tick(opening, 0x80) == 57, (state, stick, g.half(a + 9482))
            assert g.half(a + 1204) == 1422 and g.half(a + 4076) == 0
            assert g.get(a + 5088) == float_bits(750)
            assert g.get(a + 11072) == float_bits(14)
            stick = False
            assert tick(8) == 57
            assert tap_x(12) == [57] * 3
            assert tick(24) == 11 and g.half(a + 1204) == 1406
    # The shoulder chord still takes the w04 directional attack branch.
    stick = True
    start(0)
    assert tick(23, r) == 0
    assert tick(24, r | 0x80) == 37 and g.half(a + 1204) == 1453
    stick = False

    # Backstep -> X uses the source left/right sectors. A held rear stick
    # does not rotate the follow-up.
    for yaw, facing in ((0x4000, 0xE000), (0x8000, 0), (0xC000, 0x2000)):
        g.put(a + 4076, 0)
        start(0)
        assert tick(24, 0x80) == 57
        aim, stick = (-yaw) & 0xFFFF, True
        assert tick(8) == 57
        assert tap_x(12) == [57] * 3
        assert tick(24) == 11 and g.half(a + 4076) == facing, yaw
        stick = False

    # R+B uses the input selector's inclusive boundaries. Ordinary B uses
    # a separate directional dispatch table.
    for yaw, expected in ((0x6000, 39), (0x6001, 37), (0x9FFF, 37), (0xA000, 38)):
        g.put(a + 4076, 0)
        aim, stick = (-yaw) & 0xFFFF, True
        start(0)
        assert tick(23, r) == 0
        assert tick(24, r | 0x80) == expected, yaw

    # w04 shield destination 62 treats rear and lateral sectors separately
    # from forward; opposite diagonal edges belong to different sectors.
    for yaw, expected, facing in ((0x3000, 43, 0xFC00), (0xD000, 47, 0x0400)):
        g.put(a + 4076, 0)
        aim, stick = (-yaw) & 0xFFFF, True
        start(0)
        assert tick(10, r | 0x100) == 0
        assert tick(24, r | 0x100) == expected and g.half(a + 4076) == facing, yaw

    stick = False
    aim = 0xC000

    # Plain spin and Chaos Oil spin share w04 state 2.
    for state in (2, 55):
        start(state)
        assert g.half(a + 1204) == 1405
        for frame in (40, 54, 66, 80):
            assert tap_x(frame) == [state] * 3
        start(state)
        stick = True
        assert tick(39, 0x40) == state
        slot = tick(40, 0x40 | 0x80)
        assert slot == 39, (state, slot, g.get(a + 5140), g.get(a + 4076))
        stick = False
        assert tap_x(18) == [39] * 3
        assert tick(26) == 11 and g.half(a + 1204) == 1406

    # The source jumping spin accepts X before frame 72 and executes at 66.
    start(36)
    assert g.half(a + 1204) == 1447
    assert tap_x(48) == [36] * 3
    assert tick(66) == 36
    start(36)
    assert tap_x(54) == [36] * 3
    assert tick(65) == 36 and tick(66) == 4 and g.half(a + 1204) == 1406

    # A ledge jump keeps the pre-input until it lands inside the source window.
    for landing, expected in ((64, 4), (68, 4), (72, 56)):
        grounded = False
        start(56)
        assert tap_x(54) == [56] * 3
        assert tick(min(landing - 1, 66)) == 56
        grounded = True
        assert tick(landing) == (56 if landing < 66 else expected), (landing, expected)
        if landing < 66:
            assert tick(66) == expected and g.half(a + 1204) == 1406
    print("w04 R+X, backstep and its X follow-up, spins, sprint jump and landing passed")


def roll_cancel():
    s = Scene(1, "sword-shield", W01)
    g, a = s.g, s.actor
    mgr, vt, loader, bank = [g.alloc(n) for n in (16, 64, 4, 112)]
    g.put(0x2126A64, mgr)
    g.put(mgr, vt)
    g.put(vt + 48, loader)
    g.put(bank + 80, 1)
    g.put(a + 3844 + 15 * 4, bank)
    g.stub(0xB49E54, lambda *_: 1)
    g.stub(loader, lambda mgr, kind, *_: bank if kind == 0x2125210 else 0)
    s.stub(0x53A058, lambda *_: 5001)
    for yaw, clip in ((0, 19), (0x1FFF, 19), (0x2000, 21), (0x4000, 21),
                      (0xC000, 20), (0xDFFF, 20), (0xE000, 19)):
        g.put(a + 4076, 0)
        s.stub(0x27E608, lambda *_: 1)
        s.stub(0x27F5D0, lambda *_: (-yaw) & 0xFFFF)
        s.action(16, 0x8000)
        g.put(s.inputs, 0)
        s.tick()
        g.put(a + 1280, float_bits(24))
        g.put(s.inputs, 8)
        s.tick()
        assert (g.half(a + 9478), g.half(a + 9482)) == (1, 6)
        assert g.cpu.mem_read(a + 5, 1) == b"\0"
        g.put(s.inputs, 0)
        s.stub(0x27E608, lambda *_: 0)  # release the stick before GU starts the roll
        s.tick()
        assert g.run(g.hooks[ENCODE], a, 28) == 0xF00 + clip, yaw
        s.action(1, 0)
        s.tick()
        s.action(1, 6)
        assert g.run(g.hooks[ENCODE], a, 28) == 28
    print(
        "w01 forward and side cancels retain their direction on GU's first roll frame and clear on exit"
    )


def sprint_landing():
    s = Scene(1, "sword-shield", W01)
    g, a = s.g, s.actor
    s.stub(0x53A058, lambda *_: 5001)
    s.stub(0x27F5D0, lambda *_: 0)
    s.stub(0x27E608, lambda *_: 1)
    s.stub(0x27F678, lambda *_: 0)  # fixture camera faces forward
    s.stub(0x2A8458, lambda *_: 1)  # cached terrain probe sees a ledge
    # The terrain fixture accepts GU's forced falling transition. The
    # production controller still chooses the group/slot and confirms acceptance.
    s.stub(
        g.get(W01 + 760), lambda owner, group, slot, flags: s.action(group, slot) or 1
    )
    for landing, stick, stamina, expected in (
        ((4, 3), True, 900, True),
        ((1, 138), True, 900, True),
        ((4, 3), False, 900, False),
        ((4, 3), True, 0, False),
        ((64, 1), True, 900, False),
    ):
        s.action(16, 0x8035)
        s.stub(0x27E608, lambda *_: 1)
        g.put(a + 5088, float_bits(900))
        g.put(a + 1280, float_bits(0))
        s.tick()
        s.tick()
        assert (g.half(a + 9478), g.half(a + 9482)) == (4, 14)
        s.tick()
        s.action(*landing)
        s.tick()
        s.action(1, 0)
        s.stub(0x27E608, lambda *_: int(stick))
        g.put(a + 5088, float_bits(stamina))
        s.tick()
        assert g.half(a + 9482) == (0x8035 if expected else 0)
    print(
        "w01 sprint ledge transition, landing/climb resume, stick release, depletion and damage passed"
    )


def lobby():
    s = Scene(1, "sword-shield", W01)
    g, a = s.g, s.actor
    equipment = g.get(a + 5060)
    lobby = g.alloc(14000)
    g.put(lobby, 0x1748974)
    g.put(lobby + 5060, equipment)
    g.put(0x18866F8 + 16, 0x1886678)
    calls = []
    s.stub(g.originals[DISPATCH], lambda actor, *_: calls.append(actor) or 7)
    manager = g.get(0x1885B58)
    s.action(1, 0)
    assert g.run(g.hooks[DISPATCH], a) == 7
    g.put(manager + 28, lobby)
    # Real uPlayerLobby allocation ends at 10048. Catch reads as well as writes.
    from unicorn import UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE

    invalid = []
    watch = g.cpu.hook_add(
        UC_HOOK_MEM_READ | UC_HOOK_MEM_WRITE,
        lambda cpu, _, addr, size, value, data: invalid.append(addr - lobby),
        begin=lobby + 10048,
        end=lobby + 13999,
    )
    assert g.run(g.hooks[DISPATCH], lobby) == 7
    assert g.run(g.hooks[ENCODE], lobby, 5001) == 0x501
    assert not invalid, invalid
    g.cpu.hook_del(watch)
    g.put(manager + 28, a)
    assert g.run(g.hooks[DISPATCH], a) == 7 and calls == [a, lobby, a]
    _, selected = service(g)
    assert g.run(selected, 1) == 14
    s.stub(g.get(W01 + 360), lambda *_: 0)
    for kind in (0, 1, 2, 3, 4):
        raw = g.run(g.originals[SNS_DRAW_ROUTE], a, kind)
        route = g.run(g.hooks[SNS_DRAW_ROUTE], a, kind)
        if kind in (0, 1, 3):
            assert struct.unpack("<3H", g.cpu.mem_read(route, 6))[:2] == (16, 0x8009)
        else:
            assert route == raw
    print("w01 quest/lobby/quest, ground draws, guard and terrain draw routes passed")


if __name__ == "__main__":
    mapped_inputs()
    attack_followups()
    roll_cancel()
    sprint_landing()
    lobby()
