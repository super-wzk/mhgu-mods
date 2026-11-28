"""Replay Frontier sprint sheathing and drawn recovery through the GU style hooks."""
from pathlib import Path
import struct
import sys

PLUGINS = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(PLUGINS / "great-sword-extreme/tests"))
from unicorn.arm_const import UC_ARM_REG_S0
from verify_style import character_save, STYLE_IDS, DECODE, DISPATCH, ENCODE, boot, hunters, load_registered_style, setup_controller


def replay(weapon, plugin, vt, sprint=None, recoveries=()):
    elf = (PLUGINS.parent / "mhgu/main.elf").read_bytes()
    g = boot(elf, character_save({weapon: STYLE_IDS[plugin]}), ("registry",))
    dist = PLUGINS / plugin / "dist" / plugin / "romfs/nativeNX"
    desc = load_registered_style(g, dist / "plugins" / (plugin.replace("-", "_") + ".nro"))
    actor, eq, _ = hunters(g)
    g.put(actor, vt)
    g.cpu.mem_write(eq + 1236, bytes([weapon]))
    set_action = setup_controller(g, actor)
    g.put(actor + 240, g.alloc(128))
    if weapon == 8:
        g.stub(0x2A4324, lambda *_: 0)
        g.run(0x11A8BBC, actor, 0)
    manager, table, loader, bank = [g.alloc(n) for n in (16, 64, 4, 112)]
    g.put(0x2126A64, manager)
    g.put(manager, table)
    g.put(table + 48, loader)
    attack_bank, terrain = (g.alloc(112) for _ in range(2))
    for resource in (bank, attack_bank, terrain):
        g.put(resource + 80, 1)
    g.put(actor + 3844 + 14 * 4, attack_bank)
    g.put(actor + 3844 + 13 * 4, terrain)
    def load(_, kind, path, flags):
        return {"0": bank, "2": terrain, "4": attack_bank}[g.string(path)[-1]] if kind == 0x2125210 else 0
    g.stub(loader, load)
    g.put(actor + 3844 + 15 * 4, bank)
    g.stub(0xB49E54, lambda *_: 1)
    g.cpu.mem_write(actor + 5078, struct.pack("<h", 100))
    g.cpu.mem_write(actor + 10057, b"\1")
    g.stub(0x27BCD0, lambda *_: 0)
    for off in (376, 1912):
        entry = g.get(vt + off)
        if entry not in g.traps:
            g.stub(entry, lambda *_: 0)
    exits = []
    g.stub(0x2B6E94, lambda *_: exits.append(True))  # Native shared return to locomotion.

    cases = [(2, 1, 5003, 1003, 0)] if sprint else []
    if sprint and weapon != 7:
        cases.insert(0, (1, 1, 5002, 1002, 0))
    if weapon in (0, 1, 11):
        rolls = (1420, 1421) if weapon == 1 else (1012, 1013)
        cases.extend((6, 2, request, source, 0) for request, source in zip((5012, 5013), rolls))
    if sprint:
        sprint_state, brake_state, clip = sprint
        cases.extend(((2, 1, 5003, clip, sprint_state), (2, 1, 5003, 1003, brake_state)))
    for slot, phase, request, source, previous in cases:
        set_action(1, slot)
        g.cpu.mem_write(actor + 9480, struct.pack("<H", 16))
        g.cpu.mem_write(actor + 9484, struct.pack("<H", 0x8000 + previous if previous else 0))
        packed = g.run(ENCODE, actor, request)
        group = (source - 1000) // 100
        clip = source % 100
        assert packed == (0xE00 if group == 4 else 0xF00) + clip
        assert g.run(DECODE, actor, packed) == request
        lmt = next((dist / "player/mot").rglob(f"{group}.lmt")).read_bytes()
        offset, = struct.unpack_from("<I", lmt, 8 + 4 * clip)
        frames, loop = struct.unpack_from("<Ii", lmt, offset + 8)
        assert loop == -1
        if source == 1032 and weapon in (9, 11):
            equipment = ([(7, 60), (5, 6)], 1) if weapon == 11 else ([(1, 100)], 2)
            prefix, sheathed = equipment
            events, = struct.unpack_from("<I", lmt, offset + 52)
            count, runs = struct.unpack_from("<2I", lmt, events + 64)
            remaining = frames - 1 - sum(length for _, length in prefix)
            assert list(struct.iter_unpack("<2I", lmt[runs:runs + 8 * count])) == [
                *prefix, (sheathed, remaining), (0, 1)
            ]
        end = frames - 1
        g.cpu.mem_write(actor + 5, bytes([phase]))
        g.cpu.mem_write(actor + 1204, struct.pack("<H", packed))
        for previous, frame, delta, finished in (
                (end - 2, end - 2, 2, False), (end - 2, end, 2, False),
                (end, end, 0, False), (end, end, 2, True)):
            exits.clear()
            g.cpu.mem_write(actor + 28, struct.pack("<f", delta))
            g.cpu.mem_write(actor + 1206, b"\x02")
            g.cpu.mem_write(actor + 1276, struct.pack("<3f", previous, frame, end))
            g.run(g.hooks[DISPATCH], actor)
            assert bool(exits) == finished, (plugin, slot, clip, previous, frame, delta, exits)
    if recoveries:
        replay_launch(g, desc, actor, set_action, recoveries)
    runs = (103, 148, 178, 179, 105, 151) if weapon == 8 else sprint[:2]
    if weapon == 7:
        runs = (*runs, 71, 82)
    replay_sprint_arts(g, desc, actor, set_action, runs)
    print(f"{plugin}: Frontier ground-action replay passed")


def replay_sprint_arts(g, desc, actor, set_action, states):
    inputs = g.alloc(40)
    tick = g.get(desc + 20)
    g.put(actor + 10068, 3)
    g.cpu.mem_write(actor + 10057, b"\1")
    g.cpu.mem_write(inputs + 34, struct.pack("<h", 120))
    g.put(actor + 5180, 0)
    g.cpu.hook_del(g.traps.pop(0x2E9D54))

    def transition(*_):
        table = g.get(actor + 9384)
        entry = g.get(g.get(table + 20))
        command = g.get(entry + 8) + 100
        group, slot = g.half(command), g.half(command + 2)
        if (group, slot) == (0x8010, 241):
            if not g.get(actor + 5180) & 7:
                return 0
            set_action(16, 241)
        elif group & 0x8000:
            return 0
        else:
            set_action(group, slot)
        return 1

    g.stub(0x2E9D54, transition)
    for state in states:
        for slot in range(3):
            g.put(actor + 5180, 0)
            set_action(16, 0x8000 + state)
            g.put(actor + 5088, struct.unpack("<I", struct.pack("<f", 900))[0])
            g.run(tick, actor, inputs)
            g.put(actor + 5180, 1 << slot)
            g.run(tick, actor, inputs)
            assert g.half(actor + 9482) == 241, (state, slot, g.half(actor + 9482))


def replay_launch(g, desc, actor, set_action, cases):
    inp = g.alloc(40)
    g.put(inp, 1 << 3)  # B resolved by GU's common input.
    tick = g.get(desc + 20)
    stamina = []

    def spend(*_):
        (amount,) = struct.unpack("<f", struct.pack("<I", g.cpu.reg_read(UC_ARM_REG_S0)))
        stamina.append(amount)
        return 0

    g.stub(0x2A5EBC, spend)
    g.stub(0x2864D4, lambda *_: 0)
    g.cpu.hook_del(g.traps.pop(0x2A2444))  # Read GU's actual drawn flag.
    for form_action, directions, invulnerability in cases:
        if form_action:
            set_action(16, 0x8000 + form_action)
            g.run(tick, actor, g.alloc(40))
        for direction, (state, bank, clip) in zip((6, 7), directions):
            g.put(inp, 1 << 3)
            source = {13: 1200, 14: 1400, 15: 1000}[bank] + clip
            assert g.run(ENCODE, actor, 0x10000 | source) == 256 * bank + clip
            set_action(64, direction)
            stamina.clear()
            # Actor must be drawn, alive and eligible on the actual landing clip.
            for phase, health, reserve, motion, drawn, eligible in (
                (1, 100, 450., 0x10F, 1, 1),
                (2,   0, 450., 0x10F, 1, 1),
                (2, 100, 149., 0x10F, 1, 1),
                (2, 100, 450., 0x10E, 1, 1),
                (2, 100, 450., 0x10F, 0, 1),
                (2, 100, 450., 0x10F, 1, 0),
            ):
                if form_action and not drawn:
                    continue  # Sheathing would also drop the light-sword form.
                g.cpu.mem_write(actor + 5, bytes([phase]))
                g.cpu.mem_write(actor + 10057, bytes([drawn]))
                g.cpu.mem_write(actor + 11988, bytes([eligible]))
                g.cpu.mem_write(actor + 1204, struct.pack("<H", motion))
                g.cpu.mem_write(actor + 5078, struct.pack("<h", health))
                g.cpu.mem_write(actor + 5088, struct.pack("<f", reserve))
                g.run(tick, actor, inp)
                assert g.half(actor + 9482) == direction and not stamina
            # A press just before ground contact survives the native landing
            # transition. The launch still owns the hunter while in the air.
            g.put(inp, 1 << 3)
            g.cpu.mem_write(actor + 5, b"\1")
            g.cpu.mem_write(actor + 1204, struct.pack("<H", 0x10E if direction == 6 else 0x114))
            g.cpu.mem_write(actor + 68, struct.pack("<f", 149))
            g.cpu.mem_write(actor + 4116, struct.pack("<f", -1))
            g.run(tick, actor, inp)
            assert g.half(actor + 9482) == direction and not stamina
            g.put(inp, 0)
            g.cpu.mem_write(actor + 5, b"\2")
            g.cpu.mem_write(actor + 1204, struct.pack("<H", 0x10F))
            g.cpu.mem_write(actor + 11988, b"\1")
            g.run(tick, actor, inp)
            action = g.half(actor + 9478), g.half(actor + 9482)
            assert action == (16, 0x8000 + state), (action, state)
            assert g.half(actor + 1204) == source  # setup_controller captures the layer's request.
            assert stamina == [-150.]
            assert struct.unpack("<f", g.cpu.mem_read(actor + 11072, 4))[0] == invulnerability


if __name__ == "__main__":
    replay(0, "great-sword-extreme", 0x1749A88, (200, 201, 1032))
    replay(1, "sword-shield-extreme", 0x174A354, (53, 54, 1450))
    replay(11, "dual-blades-extreme", 0x174F2B4, (89, 90, 1032),
           ((0, ((77, 13, 12), (76, 13, 13)), 94.),))
    replay(7, "long-sword-extreme", 0x174CF64, (85, 86, 1412))
    replay(9, "gunlance-extreme", 0x174E110, (82, 83, 1032),
           ((0, ((69, 13, 12), (68, 13, 13)), 100.),))
    replay(8, "switch-axe-extreme", 0x174D838,
           recoveries=((0, ((101, 14, 77), (102, 14, 78)), 60.),
                       (148, ((167, 15, 50), (168, 15, 51)), 60.)))
