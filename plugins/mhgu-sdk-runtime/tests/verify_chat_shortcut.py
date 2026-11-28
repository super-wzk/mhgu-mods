"""Replay GU's controller shortcuts with and without the SDK runtime."""

import sys
from pathlib import Path

PLUGINS = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(PLUGINS / "great-sword-extreme/tests"))
from verify_style import CHAT_UPDATE, SHARED_MENU, TARGET_CAMERA, TOUCH_MENU, boot, hunters

UI_PRESSED, UI_HELD = 27556, 27548
PAD_GLOBAL, SETTINGS_GLOBAL = 0x188C670, 0x1897F78
AUXILIARY = 0x65FA04
L3, R3, ZL, A = 0x4000000, 0x8000000, 0x200, 0x100


def replay(elf, registry_enabled):
    g = boot(elf, None, ("registry",) if registry_enabled else ())
    actor, _, _ = hunters(g)
    pad, settings, menu, vtable, touch = [
        g.alloc(size) for size in (27800, 17600, 2400, 200, 8)
    ]
    g.put(PAD_GLOBAL, pad)
    g.put(SETTINGS_GLOBAL, settings)
    g.put(menu, vtable)
    calls = []
    item = g.alloc(12)
    g.put(item + 4, 255)
    g.put(menu + 192, item)

    def stub(address, body):
        if address in g.traps:
            g.cpu.hook_del(g.traps.pop(address))
        g.stub(address, body)

    def constant(address, value):
        stub(address, lambda *_: value)

    constant(0x227A98, 1)  # Online
    constant(0x276278, actor)
    constant(0x65EB0C, touch)
    constant(0x5CD09C, 1)  # Native chat is currently available.
    constant(0x65CB3C, g.alloc(256))
    zooms = []
    stub(0x666970, lambda *_: zooms.append(True) or 1)
    for addr in (0x54F260, 0x55EFE8, 0x56DFAC, 0x65EFC8, 0x5584F4,
                 0x790EFC, 0x62C078, 0x66F868):
        constant(addr, 0)

    for off, reply in ((76, 0), (88, 0), (96, 1), (132, 0)):
        dest = g.alloc(4)
        constant(dest, reply)
        g.put(vtable + off, dest)
    dest = g.alloc(4)
    g.stub(dest, lambda *_: calls.append("open") or 0)
    g.put(vtable + 196, dest)

    def check(route, control, shoulder, state, event, buttons, result, enters=False):
        g.cpu.mem_write(settings + 17516, bytes([control, 0, shoulder, 0]))
        g.cpu.mem_write(menu + 60, bytes(5))
        g.put(menu + 72, 0)
        g.put(menu + 148, state)
        g.put(menu + 1708, 2)
        g.put(touch, int(event is not None))
        g.put(touch + 4, event or 0)
        g.put(pad + UI_PRESSED, buttons)
        g.put(pad + UI_HELD, buttons)
        calls.clear()
        g.run(route, menu, 0)
        actual = g.get(menu + 148)
        assert actual == result, (hex(route), control, shoulder, event, result, actual)
        assert bool(calls) == enters, (hex(route), control, shoulder, calls)
        assert g.get(pad + UI_PRESSED) == g.get(pad + UI_HELD) == buttons

    for shoulder, extra in ((0, 0x800), (1, 0x40)):
        buttons = extra | 0x20
        for control in (0, 2):
            check(CHAT_UPDATE, control, shoulder, 0, None, buttons, int(not registry_enabled))
        check(SHARED_MENU, 3, shoulder, 0, None, buttons, 0, not registry_enabled)
        for state in (0, 16):
            check(TOUCH_MENU, 0, shoulder, state, None, buttons, state if registry_enabled else 6)
        assert g.run(AUXILIARY) == extra
    # The alternate controller layout uses a different binding, and the
    # manual chat request enters through the original menu/touch event.
    check(CHAT_UPDATE, 1, 0, 0, None, 1 | 0x800, 1)
    check(CHAT_UPDATE, 0, 0, 0, 3, 0x800, 1)
    for control in range(4):
        for stick_camera in (0, 1):
            g.cpu.mem_write(settings + 17403, bytes([stick_camera]))
            for buttons in (L3, R3, L3 | R3):
                zooms.clear()
                check(CHAT_UPDATE, control, 0, 0, None, buttons, 0)
                assert bool(zooms) == (bool(buttons & L3) and control != 3 and not stick_camera)

    # Confirm the physical input mapping before replaying the target entry.
    for physical, game in ((2, L3), (4, R3), (0x400, ZL)):
        assert g.run(0x3DE4B0, 0, physical) == game
    if registry_enabled:
        sampled = []
        stub(g.originals[TARGET_CAMERA],
             lambda *_: sampled.append((g.get(pad + UI_PRESSED), g.get(pad + UI_HELD), g.cpu.mem_read(settings + 17516, 1)[0])))
        for control, swapped, camera_pad, stick_camera, target in (
            (0, 0, 0, 0, ZL),
            (0, 1, 0, 0, 0x400),
            (1, 0, 0, 0, 2),
            (1, 0, 1, 0, 0x200000),
            (2, 0, 0, 0, R3),
            (3, 0, 0, 1, R3),
            (3, 0, 0, 0, L3),
        ):
            g.cpu.mem_write(settings + 17516, bytes([control, 0, swapped, camera_pad]))
            g.cpu.mem_write(settings + 17403, bytes([stick_camera]))
            for pressed in (0, L3, R3, target, L3 | R3, target | R3):
                buttons = pressed | A
                expected = buttons if pressed & R3 else buttons & ~target
                g.put(pad + UI_PRESSED, buttons)
                g.put(pad + UI_HELD, buttons)
                g.run(TARGET_CAMERA, menu)
                assert sampled[-1] == (expected, buttons, 2 if pressed & R3 else control), (control, stick_camera, buttons)
                assert g.cpu.mem_read(settings + 17516, 1)[0] == control
                assert g.get(pad + UI_PRESSED) == buttons
    if registry_enabled:
        # Let the original cUIOCPanelTargetCamera select its sequence and cache it.
        camera_manager = g.alloc(100)
        g.put(g.get(0x65734c + g.get(0x657570)), camera_manager)
        for address in (0x56E808, 0x6666D4, 0x56E528):
            constant(address, 0)
        sequences = []
        stub(0x56E538, lambda panel, group, sequence, _: sequences.append((group, sequence)))
        for layout in range(4):
            g.cpu.mem_write(settings + 17516, bytes([layout]))
            g.put(menu + 84, 0xffffffff)
            g.run(g.hooks[0x657338], menu)
            assert sequences == [(96, 0xf42ac)]
            sequences.clear()
            g.run(g.hooks[0x657338], menu)
            assert not sequences and g.cpu.mem_read(settings + 17516, 1)[0] == layout
    print(f"registry={registry_enabled}: chat, map zoom and target input passed")


if __name__ == "__main__":
    elf = (PLUGINS.parent / "mhgu/main.elf").read_bytes()
    replay(elf, False)
    replay(elf, True)
