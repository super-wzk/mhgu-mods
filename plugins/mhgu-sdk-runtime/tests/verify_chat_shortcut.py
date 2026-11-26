"""Replay the native chat triggers with and without the SDK runtime."""

import sys
from pathlib import Path

PLUGINS = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(PLUGINS / "great-sword-extreme/tests"))
from verify_style import CHAT_UPDATE, SHARED_MENU, TOUCH_MENU, boot, hunters

UI_PRESSED, UI_HELD = 27556, 27548
PAD_GLOBAL, SETTINGS_GLOBAL = 0x188C670, 0x1897F78
AUXILIARY = 0x65FA04


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
    print(f"registry={registry_enabled}: native chat routes, manual chat and auxiliary input passed")


if __name__ == "__main__":
    elf = (PLUGINS.parent / "mhgu/main.elf").read_bytes()
    replay(elf, False)
    replay(elf, True)
