"""Replay Frontier evade slashes against GU's native Bladescale counter."""
import os
import struct
from pathlib import Path

from verify_weapon_arts import Scene, float_bits
from verify_style import DISPATCH, WEAPON_DISPATCH


def replay(weapon, name, vtable, states, ordinary, weight):
    scene = Scene(weapon, name, vtable)
    g, actor = scene.g, scene.actor
    # Let GU read the equipped skill bits, weapon property, drawn flag and
    # sharpness. Capture the feedback request independently of the gauge value.
    for addr in (0x2864C4, 0x2A2444, 0x2A2560, 0x27BCD0):
        g.cpu.hook_del(g.traps.pop(addr))
    eq = g.get(actor + 5060)
    buffs = g.alloc(1100)
    g.put(eq + 396, buffs)
    scene.stub(0x29CDFC, lambda *_: 0)
    notifications = []
    scene.stub(g.get(vtable + 916), lambda _, cue, *__: notifications.append(cue) or 1)
    scene.stub(g.get(vtable + 940), lambda *_: 0)
    scene.stub(0x887660, lambda *_: 0)
    # Decode the actual Naab Sword or Zakun Twins row and an ordinary weapon
    # using the game's row reader. +184 is the equipped weapon's innate effect.
    cat, slot, seregios, ctor, read = {
        1: (8, 21, 27, 0xF8158, 0xF8D90),
        11: (18, 30, 15, 0x11BAA8, 0x11C758),
    }[weapon]
    raw = (Path(os.environ["MHGU_ROMFS"]) / f"nativeNX/table/weapon{weapon:02}BaseData.w{weapon:02}d").read_bytes()
    count, = struct.unpack_from("<I", raw, 4)
    stride, rem = divmod(len(raw) - 8, count)
    assert rem == 0
    stream, buf, manager, table, rows = (g.alloc(n) for n in (24, len(raw), 256, 112, 40 * count))
    g.cpu.mem_write(buf, raw)
    g.put(stream, 0x1787AD8)
    g.put(stream + 8, buf)
    g.put(stream + 16, len(raw))
    g.put(0x1856B18, manager)
    g.put(manager + slot * 4, table)
    g.put(table + 100, count)
    g.put(table + 108, rows)
    def equip(index):
        row = rows + index * 40
        g.put(stream + 12, 8 + index * stride)
        g.run(ctor, row)
        g.run(read, row, stream)
        g.cpu.mem_write(eq + 28, struct.pack("<HH", cat, index))
        assert g.run(0xDE16C, eq + 24) == int(index == seregios), (name, index)

    def equipment(skill, innate):
        g.put(buffs + 988 + 4 * (95 // 32), (1 << (95 % 32)) if skill else 0)
        equip(seregios if innate else 1)

    equipment(True, False)
    g.put(actor + 5078, 100)
    g.put(actor + 10100, 50)
    g.put(actor + 10104, 100)

    def start(state):
        scene.action(16, 0x8000 + state)
        g.put(actor + 1280, float_bits(0))
        scene.tick()
        scene.tick()  # Same action and frame must not add progress twice.

    for state in states:
        g.put(actor + 10704, 0)
        start(state)
        assert g.get(actor + 10704) == weight, (name, state, g.get(actor + 10704))
    start(ordinary)
    assert g.get(actor + 10704) == weight
    for address in (0x1897F78, 0x188C670):
        if not g.get(address):
            g.put(address, g.alloc(30000))
    scene.stub(0x65F93C, lambda *_: 0x40)
    scene.stub(0x65FA04, lambda *_: 0x4000)
    scene.stub(0x27DB78, lambda *_: 0)
    scene.stub(0x27E608, lambda *_: int(stick))
    scene.stub(0x27F5D0, lambda *_: aim)
    scene.stub(0x27E638, lambda *_: 0)
    scene.stub(g.originals[DISPATCH], lambda *_: 0)
    previous = 0
    aim = 0
    stick = True

    def pad(frame, held):
        nonlocal previous
        g.put(actor + 1280, float_bits(frame))
        g.cpu.mem_write(actor + 5108, struct.pack("<3H", held, previous & ~held, held & ~previous))
        previous = held
        g.cpu.mem_write(actor + 5140, bytes(32))
        g.run(0x27CA28, actor)
        g.run(0x1169554 if weapon == 1 else 0x11BAEF0, actor)
        g.put(actor + 5136, 100 if stick else 0)
        custom = g.half(actor + 9478) == 16 and g.half(actor + 9482) >= 0x8000
        g.run(WEAPON_DISPATCH if custom else DISPATCH, actor)
        return g.half(actor + 9482) - 0x8000

    def dodge(i, phase, plain_b):
        nonlocal aim, stick
        stick = True
        g.put(actor + 5088, float_bits(900))
        g.put(actor + 4076, 0)
        if weapon == 1:
            aim = (0x8000, 0x4000, 0xc000)[i % 3]
            scene.action(16, 0x8000)
            pad(0, 0)
            if plain_b:
                direction = (None, 0x8000, 0, 0x4000, 0xc000)[i % 5]
                stick = direction is not None
                aim = direction or 0
                pad(23, 0)
                state = pad(24, 0x80)
                if direction in (None, 0x8000):
                    assert state == 57
                else:
                    assert (g.half(actor + 9478), g.half(actor + 9482)) == (1, 6)
                    g.run(0x2CBFCC, actor)
            else:
                pad(23, 0x40)
                state = pad(24, 0xc0)
                assert state in (37, 38, 39), state
        else:
            aim = (0, 0x8000, 0x4000, 0xc000)[i % 4]
            scene.action(1, 0)
            g.put(scene.field("PHASE"), phase)
            pad(0, 0)
            state = pad(0, 0x80)
            if phase == 1:
                assert (g.half(actor + 9478), g.half(actor + 9482)) == (1, 6)
                # Run GU's group 1 dispatcher and its initial drawn roll frame.
                g.run(0x2CBFCC, actor)
            else:
                assert state in states, state
        pad(2, 0)

    for offset in (608, 1300, 1840, 1616):
        scene.stub(g.get(vtable + offset), lambda *_: 0)
    modes = ((1, False, 7), (2, False, weight), (3, False, weight)) if weapon == 11 else (
        (None, False, weight), (None, True, weight))
    for phase, plain_b, increment in modes:
        count = (35 + increment - 1) // increment
        for skill, innate, gain in ((False, False, 0), (True, False, 5), (False, True, 5), (True, True, 7)):
            equipment(skill, innate)
            g.put(actor + 10100, 50)
            g.put(actor + 10704, 0)
            notifications.clear()
            for i in range(count):
                dodge(i, phase, plain_b)
                due = i == count - 1
                assert g.get(actor + 10704) == (increment * (i + 1) if gain and not due else 0)
                assert g.get(actor + 10100) == (50 + gain if due else 50), (name, phase, i)
                assert notifications.count(129) == int(due and gain > 0)
        g.put(actor + 10100, 100)
        notifications.clear()
        for i in range(count):
            dodge(i, phase, plain_b)
        assert g.get(actor + 10704) == 0 and 129 not in notifications
    print(f"{name}: native evade progress, skill, innate weapon effect and stacking passed")


if __name__ == "__main__":
    replay(1, "sword-shield", 0x174A354, (37, 38, 39), 0, 7)
    replay(11, "dual-blades", 0x174F2B4, (*range(36, 41), 73, 74, 75, *range(83, 89)), 0, 9)
