"""Replay native netPlAction sheathing with missing or stale sprint history."""

import struct
from verify_gs_db_multiplayer import network_scene

PRESENT = 1 << 20
SPRINT = 1 << 21
SHEATHE_MASK = PRESENT | SPRINT

CASES = (
    (0, "great-sword", 0x1749A88, (200,), 201, 1032),
    (11, "dual-blades", 0x174F2B4, (89,), 90, 1032),
    (7, "long-sword", 0x174CF64, (85,), 86, 1412),
    (8, "switch-axe", 0x174D838, (103, 148, 178, 179), 105, 1499),
    (1, "sword-shield", 0x174A354, (53,), 54, 1450),
    (9, "gunlance", 0x174E110, (82,), 83, 1032),
)


def replay(weapon, name, vtable, sprints, brake, sprint_clip):
    g, nx, _, manager, _, packet, local, guest, second, sender, _ = network_scene(
        weapon, name + "-extreme", vtable
    )

    def half(addr, value):
        g.cpu.mem_write(addr, struct.pack("<H", value))

    def set_action(actor, group, slot):
        half(actor + 9478, group)
        half(actor + 9482, slot)
        g.cpu.mem_write(actor + 5, b"\0")

    def send(actor, previous, expected, legacy=False, group=1, slot=2):
        set_action(sender, group, slot)
        half(sender + 9480, previous[0])
        half(sender + 9484, previous[1])
        g.put(manager + 28, sender)
        g.run(0x1FED90, packet, sender, 0)
        assert g.get(packet + 16) & SHEATHE_MASK == expected
        if legacy:
            g.put(packet + 16, g.get(packet + 16) & ~SHEATHE_MASK)
        g.put(manager + 28, local)
        g.run(0x1FF308, packet, actor)
        assert (g.half(actor + 9478), g.half(actor + 9482)) == (group, slot)

    def expect_clip(actor, clip):
        packed = g.run(0x27C6A0, actor, 5003)
        group, slot = divmod(clip - 1000, 100)
        assert packed >= 0x800 and packed & 127 == slot, (name, clip, hex(packed))
        assert g.run(0x27C6D4, actor, packed) == 5003
        lmt = next((nx / "player/mot").rglob(f"{group}.lmt")).read_bytes()
        offset, = struct.unpack_from("<I", lmt, 8 + 4 * slot)
        assert offset
        frames, = struct.unpack_from("<I", lmt, offset + 8)
        return packed, frames - 1

    exits = []
    g.stub(0x2B6E94, lambda actor, *_: exits.append(actor) or 0)
    for state in sprints:
        set_action(guest, 1, 0)
        send(guest, (16, 0x8000 + state), PRESENT | SPRINT)
        # GU set no sprint history on this receiver.
        assert g.half(guest + 9480) == 1
        packed, end = expect_clip(guest, sprint_clip)

    # An ordinary sheathe beats the second receiver's stale sprint history.
    set_action(second, 16, 0x8000 + sprints[0])
    send(second, (16, 0x8000 + brake), PRESENT)
    expect_clip(second, 1003)
    expect_clip(guest, sprint_clip)

    # Use the packaged motion in the retail sheathe handler's last-frame check.
    g.cpu.mem_write(guest + 5, b"\1")
    half(guest + 1204, packed)
    g.cpu.mem_write(guest + 1206, b"\x02")
    g.cpu.mem_write(guest + 1276, struct.pack("<3f", end, end, end))
    g.run(0x282DA8, guest)
    assert exits == [guest], (name, exits)

    # A legacy sender can still derive the clip from a known sprint.
    set_action(guest, 16, 0x8000 + sprints[0])
    send(guest, (16, 0x8000 + sprints[0]), PRESENT | SPRINT, legacy=True)
    expect_clip(guest, sprint_clip)
    set_action(guest, 1, 0)
    send(guest, (16, 0x8000 + sprints[0]), PRESENT | SPRINT, legacy=True)
    assert g.run(0x27C6A0, guest, 5003) == 0x503

    # Clear a live marker on the next packet.
    set_action(guest, 1, 0)
    send(guest, (16, 0x8000 + sprints[0]), PRESENT | SPRINT)
    expect_clip(guest, sprint_clip)
    send(guest, (16, 0x8000 + sprints[0]), 0, group=64, slot=6)
    set_action(guest, 1, 2)
    half(guest + 9480, 64)
    half(guest + 9484, 6)
    assert g.run(0x27C6A0, guest, 5003) == 0x503

    # Native interruption with no new packet also expires the marker.
    set_action(guest, 1, 0)
    send(guest, (16, 0x8000 + sprints[0]), PRESENT | SPRINT)
    set_action(guest, 64, 6)
    g.run(0x282DA8, local)
    set_action(guest, 1, 2)
    assert g.run(0x27C6A0, guest, 5003) == 0x503
    print(f"{name}: synced sprint/ordinary clips, peer isolation, exit and legacy fallback passed")


if __name__ == "__main__":
    for case in CASES:
        replay(*case)
