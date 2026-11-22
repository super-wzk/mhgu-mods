"""Exercise w01 with GU's native button mappers, motion encoder and scene dispatcher."""

import struct

from verify_port import Scene, float_bits
from verify_style import DISPATCH, ENCODE, SNS_DRAW_ROUTE, service

W01 = 0x174A354


def mapped_inputs():
    s = Scene(1, 5, "sword-shield", W01)
    g, a = s.g, s.actor
    for address in (0x1897F78, 0x188C670):
        if not g.get(address):
            g.put(address, g.alloc(30000))
    g.put(0x2126504, g.alloc(128))
    s.stub(0x27E608, lambda *_: 0)
    s.stub(0x53A058, lambda *_: 5001)
    s.stub(0x65F93C, lambda *_: 0x40)
    s.stub(0x65FA04, lambda *_: 0x4000)
    x, a_button, shoulder = 0x20, 0x100, 0x40
    for sequence, action in [
        ([x, x, x], 0),
        ([a_button] * 3, 13),
        ([x | a_button], 1),
        ([x, x | a_button], 1),
        ([a_button, x | a_button], 1),
        ([shoulder | a_button], 43),
        ([shoulder, shoulder | a_button], 43),
        ([a_button, shoulder | a_button], 43),
    ]:
        s.action(1, 0)
        g.put(a + 5188, 0)
        g.put(a + 5192, 0)
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
            g.cpu.mem_write(s.inputs, bytes(g.cpu.mem_read(a + 5140, 32)))
            s.tick()
            slot = g.half(a + 9482)
            if slot >= 0x8000 and (not starts or starts[-1] != slot):
                starts.append(slot)
        assert starts == [0x8000 + action], (sequence, starts)
    print("w01 GU mapped X, A, simultaneous/staggered X+A and R+A, held buttons passed")


def roll_cancel():
    s = Scene(1, 5, "sword-shield", W01)
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
    for yaw, clip in [(0, 19), (0x4000, 20), (0xC000, 21)]:
        s.stub(0x27E608, lambda *_: 1)
        s.stub(0x27F5D0, lambda *_: yaw)
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
    s = Scene(1, 5, "sword-shield", W01)
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
    s = Scene(1, 5, "sword-shield", W01)
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
    assert g.run(selected, 1) == 5
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
    roll_cancel()
    sprint_landing()
    lobby()
