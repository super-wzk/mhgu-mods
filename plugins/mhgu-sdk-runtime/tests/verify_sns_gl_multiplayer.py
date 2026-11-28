"""Replay Extreme SnS and Gunlance through GU's native packets and monster HP."""
import struct
import zlib

from unicorn import arm_const as arm

from verify_gs_db_multiplayer import network_scene
from verify_style import quest_arc, quest_entries


def half(g, address, value):
    g.cpu.mem_write(address, struct.pack("<H", value))


def number(g, address, value):
    g.cpu.mem_write(address, struct.pack("<f", value))


def action(g, actor, slot):
    g.cpu.mem_write(actor + 9478, struct.pack("<H2xH", 16, slot))


def replay_frame(g, actor, frame):
    number(g, actor + 1280, frame)
    g.run(0x282DA8, actor)


def configure_target(g, target):
    # Supply a stationary overlap for the native monster contact dispatcher.
    # GU still expands the hit shapes, deduplicates serials and commits HP.
    monster, status, _, _ = target
    g.stub(0x5CEB4, lambda _, index, *__: monster if index == 0 else 0)
    for pc in (0x70824, 0x81A14, 0x169878, 0x2A67F4):
        g.stub(pc, lambda *_: 1)
    for pc in (0x258FFC, 0x17260C, 0x169350, 0x16938C):
        g.stub(pc, lambda *_: 0)
    g.cpu.mem_write(status + 416, b"\1")
    g.cpu.mem_write(monster + 46585, b"\1")


def settle(g, target, hit, player):
    monster, status, collision, credit = target
    g.put(collision + 10288, 0)
    assert g.run(g.hooks[0x1696FC], hit) > 0
    xyz = g.cpu.mem_read(collision + 8248, 12)
    g.cpu.mem_write(collision + 28, struct.pack("<B3xf", 1, 2) + bytes(xyz))
    before = g.get(status + 880)
    g.put(collision + 8224, hit)
    g.run(0x16B15C, collision)
    damage = g.half(status + 1160)
    assert damage > 0 and credit[-1] == (0, player, damage), (damage, credit)
    g.run(0x97DDC, monster)
    assert g.get(status + 880) == before - damage
    g.run(0x9D67C, monster)
    previous_credit = len(credit)
    g.run(0x16B15C, collision)
    g.run(0x97DDC, monster)
    assert g.get(status + 880) == before - damage and len(credit) == previous_credit
    return damage


def sword_shield():
    vtable = 0x174A354
    g, _, _, manager, session, packet, local, peer, _, sender, target = network_scene(
        1, "sword-shield-extreme", vtable
    )
    configure_target(g, target)
    g.cpu.hook_del(g.traps.pop(0x2B76CC))
    g.stub(g.get(vtable + 440), lambda *_: 0)
    g.stub(g.originals[0x16ABAC], lambda *_: 0)

    def submit_waves():
        g.put(target[2] + 8224, 0)
        g.run(g.hooks[0x16ABAC], target[2])

    def update_status():
        # The per-frame hunter update clears these words before collecting
        # each weapon's active condition bits.
        g.put(sender + 10336, 0)
        g.put(sender + 10340, 0)
        g.run(0x1168EEC, sender)

    def oil(item, chaos):
        g.put(sender + 13096, item)
        number(g, sender + 13104, 100 if item else 0)
        g.put(sender + 13100, chaos)
        number(g, sender + 13108, 100 if chaos else 0)
        update_status()

    condition = g.alloc(32)
    g.stub(0x9FBA6C, lambda *_: 0)
    g.stub(g.originals[0x9FC3D8], lambda _, __, payload, ___:
           g.cpu.mem_write(condition, bytes(g.cpu.mem_read(payload, 32))) or 1)
    # Populate the packet RTTI identity normally registered at game startup.
    dti_getter = g.get(0x17431D8 + 20)
    dti = g.run(dti_getter, condition)
    g.put(0x185B154, 0x4160)
    g.put(dti + 28, 0x4160)

    def condition_update():
        g.run(0x201084, session, sender)
        assert g.run(0x20A9D0, session, condition, 1, 1) == 1

    shot = {}
    for item, chaos, expire, power in (
        (0, 0, None, 0), (4, 0, None, 8), (0, 5, None, 8),
        (3, 6, None, 8), (4, 7, None, 16),
        (4, 7, 13104, 8), (4, 7, 13108, 8), (4, 0, 13104, 0),
    ):
        oil(item, chaos)
        g.put(manager + 28, sender)
        action(g, sender, 0x8037)
        g.run(g.hooks[0x1FED90], packet, sender, 0)
        assert g.get(packet + 136) == (1 << (item - 1) if item else 0)
        assert g.get(packet + 140) == (1 << (chaos - 4) if chaos else 0)
        g.put(manager + 28, local)
        g.run(g.hooks[0x1FF308], packet, peer)
        assert g.half(peer + 9482) == 0x8037
        assert (g.get(peer + 10336), g.get(peer + 10340)) == (
            g.get(packet + 136), g.get(packet + 140)
        )
        replay_frame(g, peer, 0)
        if expire is not None:
            g.put(sender + expire, 0)
            update_status()
            condition_update()
            assert (g.get(peer + 10336), g.get(peer + 10340)) == (
                g.get(sender + 10336), g.get(sender + 10340)
            )
        replay_frame(g, peer, 30)
        replay_frame(g, peer, 30)
        submit_waves()
        hit = g.get(target[2] + 8224)
        assert bool(hit) == bool(power), (item, chaos, expire)
        if power:
            assert g.get(hit + 12) == peer and g.get(hit + 44) == 0
            assert g.half(hit + 56) == power
            damage = settle(g, target, hit, 1)
            assert shot.setdefault(power, damage) == damage
        # Let the previous independent wave expire before taking the next sample.
        for _ in range(20):
            submit_waves()
    assert shot == {8: 8, 16: 16}

    # Run GU's equipment modifier on the peer as well. Both the fire value
    # and a successful status roll must stay at 50% with the two oils active.
    g.cpu.hook_del(g.traps.pop(g.get(vtable + 348)))
    eq = g.get(peer + 5060)
    half(g, eq + 804, 21)
    g.stub(0x302208, lambda *_: 1)
    for kind, attack_type in ((1, 16), (6, 2)):
        half(g, eq + 834, kind)
        oil(4, 7)
        g.put(manager + 28, sender)
        g.run(g.hooks[0x1FED90], packet, sender, 0)
        g.put(manager + 28, local)
        g.run(g.hooks[0x1FF308], packet, peer)
        replay_frame(g, peer, 0)
        replay_frame(g, peer, 30)
        submit_waves()
        hit = g.get(target[2] + 8224)
        assert hit and g.get(hit + 44) == 0 and g.half(hit + 56) == 16
        assert g.get(hit + 72) == attack_type and g.half(hit + 80) == 10, (kind, g.get(hit + 72), g.half(hit + 80))
        for _ in range(20):
            submit_waves()
    half(g, eq + 804, 0)
    half(g, eq + 834, 0)
    # Round Force I uses a stock action slot; its marker must survive the
    # same native action packet before the retail PMA spawns an extra wave.
    raw = next(zlib.decompress(raw) for name, kind, raw in quest_entries("w01.arc")
               if kind == 0x18A9225C and name.endswith(b"015"))
    cues = list(struct.iter_unpack("<fhBB", raw[8:]))
    clock, obj, cue_data = (g.alloc(n) for n in (18 * 4, 112, len(cues) * 12))
    g.put(peer + 12704, clock)
    g.put(clock + 15 * 4, obj)
    g.put(obj + 100, len(cues))
    g.put(obj + 108, cue_data)
    for i, cue in enumerate(cues):
        g.cpu.mem_write(cue_data + 12 * i + 4, struct.pack("<fhBB", *cue))
    for pc in (0x2A8D10, 0x2B72D4):
        g.stub(pc, lambda *_: 0)
    for marked in (False, True):
        action(g, sender, 43)
        g.put(manager + 28, sender)
        g.run(g.hooks[0x1FED90], packet, sender, 0x40)
        flags = g.get(packet + 16)
        assert flags >> 16 & 15 == 9
        if not marked:
            g.put(packet + 16, flags & ~(15 << 16))
        g.put(manager + 28, local)
        g.run(g.hooks[0x1FF308], packet, peer)
        number(g, peer + 5328, 0)
        g.run(0x1169A7C, peer)
        number(g, peer + 5328, cues[0][0])
        g.run(0x1169A7C, peer)
        g.run(0x1169A7C, peer)
        submit_waves()
        ring = g.get(target[2] + 8224)
        assert bool(ring) == marked
        if marked:
            assert g.get(ring + 44) == 0 and g.half(ring + 56) == 16
            assert settle(g, target, ring, 1) == 16
    print("SnS: native oil packets, independent expiration, forward and Round Force HP, 50% element/status passed")


def shell_factory(g):
    path = quest_arc("w09.arc").parents[2] / "shell/pl/w09.arc"
    stem = b"shell\\pl\\pl_w09_000\\pl_w09_000"
    assets = {(name[len(stem):].lower(), kind): zlib.decompress(raw)
              for name, kind, raw in quest_entries(path) if name.startswith(stem)}
    hds = assets[b"_hitdata", 0x4EB68CEE]
    rows, table, hds_vtable = (g.alloc(n) for n in (len(hds), 112, 72))
    g.cpu.mem_write(rows, hds)
    g.put(table, hds_vtable)
    g.put(table + 100, rows)
    g.put(table + 104, rows + 16)
    g.put(hds_vtable + 64, 0x270D8C)
    g.put(hds_vtable + 68, 0x270D98)

    raw = assets[b"_hitsize", 0x5653C1B0]
    hts, data, hts_vtable = (g.alloc(n) for n in (112, len(raw), 72))
    g.cpu.mem_write(data, raw)
    g.put(hts, hts_vtable)
    g.put(hts + 100, data)
    assert g.run(0x16A1D8, hts) == 1
    getter = g.alloc(4)
    g.stub(getter, lambda obj, i, *_: g.get(g.get(obj + 104) + 4 * i))
    g.put(hts_vtable + 68, getter)

    params = struct.unpack_from("<13f", assets[b"", 0x496F8F22], 20)
    hit_params = {i: struct.unpack_from("<8i", assets[f"_hit{i:03}".encode(), 0x496F8F22], 20)
                  for i in range(42)}

    def parameter(_, index, *__):
        number_reg(g, params[index])

    def hit_parameter(shell, index, *_):
        kind = g.get(g.get(shell + 4972) + 8)
        return hit_params[kind][index]

    # Populate the native shell manager with retail HDS/HTS and FUP data.
    manager, resource, cls, cls_vt, allocator = (g.alloc(n) for n in (29000, 256, 36, 16, 4))
    g.put(0x1892550, manager)
    g.put(manager + 28 + 4 * 48, resource)
    g.put(resource + 188, table)
    g.put(resource + 192, hts)
    class_row = 0x1763448 + 12 * 48
    g.put(class_row, cls)
    g.put(cls, cls_vt)
    g.put(cls_vt + 8, allocator)
    task_type = g.run(g.get(0x176128C + 20))
    g.put(class_row + 4, task_type)

    projectiles = []
    vtable = 0x1761114
    dti = g.run(g.get(vtable + 20))
    g.put(0x1898674, g.get(dti + 4))

    def allocate(*_):
        projectile = g.alloc(5712)
        g.put(projectile, vtable)
        g.put(projectile + 12, 1)
        g.cpu.mem_write(projectile + 5038, b"\xff")
        number(g, projectile + 28, 2)
        projectiles.append(projectile)
        return projectile

    g.stub(allocator, allocate)
    g.stub(0x475850, g.alloc)
    g.stub(0x4A3060, hit_parameter)
    g.stub(0x4A2ED0, parameter)
    zeroes = (0x29C368, 0x4A1D34, 0x4A2F5C, 0xC067EC, 0x4A8F94, 0x476014,
              g.get(vtable + 148), g.get(vtable + 164), g.get(vtable + 304))
    for pc in zeroes:
        g.stub(pc, lambda *_: 0)
    resource_vt, no_geometry = g.alloc(68), g.alloc(168)
    g.put(resource, resource_vt)
    g.put(resource_vt + 64, g.alloc(4))
    g.stub(g.get(resource_vt + 64), lambda *_: no_geometry)
    g.put(no_geometry + 16, g.alloc(42 * 4))
    assert g.get(vtable + 316) == 0x4758C0 and g.get(vtable + 320) == 0x4A2790
    return projectiles


def gunlance():
    vtable = 0x174E110
    g, _, _, manager, _, packet, local, peer, other, sender, target = network_scene(
        9, "gunlance-extreme", vtable
    )
    configure_target(g, target)
    g.stub(0x53A058, lambda *_: 5001)
    g.stub(g.get(vtable + 440), lambda *_: 0)
    g.stub(g.get(vtable + 2072), lambda *_: 0)
    g.stub(g.get(vtable + 2076), lambda *_: 0x16AEF5C)
    g.stub(0x282870, lambda *_: number_reg(g, 0.))
    g.stub(0x282850, lambda *_: 0)
    for pc in (0x2A063C, 0x2AD288, 0x11AA480, 0x11B0C38, 0x11B0E94):
        g.stub(pc, lambda *_: 0)
    g.cpu.hook_del(g.traps.pop(g.get(vtable + 348)))
    g.stub(g.get(vtable + 348), lambda actor, idx, *_: g.half(g.get(actor + 5060) + 800 + 2 * idx))
    for actor in (sender, peer, other):
        half(g, g.get(actor + 5060) + 800 + 2 * 28, 3)
        g.cpu.mem_write(actor + 11140, b"\x05")
        g.put(actor + 12, 1)

    shells = shell_factory(g)

    def send(actor, slot, ammo, marked=True, state_value=None):
        action(g, sender, 0x8000 + slot)
        g.cpu.mem_write(sender + 11138, bytes([ammo]))
        g.put(manager + 28, sender)
        g.run(g.hooks[0x1FED90], packet, sender, 0)
        flags = g.get(packet + 16)
        assert flags >> 16 & 15 == 8 + (ammo if state_value is None else state_value)
        if not marked:
            g.put(packet + 16, flags & ~(15 << 16))
        g.put(manager + 28, local)
        g.run(g.hooks[0x1FF308], packet, actor)
        assert g.half(actor + 9482) == 0x8000 + slot
        replay_frame(g, actor, 0)

    def ammo(actor):
        return g.cpu.mem_read(actor + 11138, 1)[0]

    def consume_shots(actor, frame):
        start = len(shells)
        replay_frame(g, actor, frame)
        g.run(0x11A9A54, actor)
        batch = shells[start:]
        for shell in batch:
            task = g.get(shell + 4972)
            assert g.get(task + 12) == actor and g.get(shell + 5144) == actor
        return batch

    def apply(shell, player):
        hit = shell + 5032
        g.run(0x16919C, hit)
        g.run(0x16919C, hit)
        return settle(g, target, hit, player)

    send(peer, 5, 2)
    assert ammo(peer) == 5
    assert not consume_shots(peer, 44) and ammo(peer) == 5
    send(peer, 1, 1, marked=False)
    assert not consume_shots(peer, 0) and ammo(peer) == 5
    send(peer, 1, 1)
    normal, = consume_shots(peer, 0)
    normal_damage = apply(normal, 1)
    assert not consume_shots(peer, 0) and ammo(peer) == 0
    send(peer, 1, 0)
    assert not consume_shots(peer, 8) and ammo(peer) == 0

    send(peer, 34, 2)
    assert not consume_shots(peer, 49)
    charged, = consume_shots(peer, 50)
    assert ammo(peer) == 1 and not consume_shots(peer, 50)
    charged_damage = apply(charged, 1)

    # Normal shells with Load Up span two native queue updates.
    g.cpu.mem_write(other + 11140, b"\x06")
    send(other, 30, 6)
    assert not consume_shots(other, 49)
    first, second = consume_shots(other, 50), consume_shots(other, 52)
    assert len(first) == len(second) == 3
    assert ammo(other) == 0 and not consume_shots(other, 54)
    burst_damage = [apply(shell, 2) for shell in first + second]
    assert ammo(peer) == 1

    # The sequential full burst sends one native shell each update.
    send(other, 31, 2)
    sequential = consume_shots(other, 50) + consume_shots(other, 52)
    assert len(sequential) == 2 and ammo(other) == 0 and not consume_shots(other, 54)
    assert [apply(shell, 2) for shell in sequential] == burst_damage[:2]

    # The first clip loads one round and the second opens the follow-up.
    send(peer, 39, 1)
    assert ammo(peer) == 2
    g.cpu.mem_write(peer + 1206, b"\x04")
    replay_frame(g, peer, 30)
    g.cpu.mem_write(peer + 1206, b"\0")
    assert not consume_shots(peer, 14) and ammo(peer) == 2
    send(peer, 1, 2)
    quick, = consume_shots(peer, 0)
    assert g.get(g.get(quick + 4972) + 8) == 18
    assert ammo(peer) == 1 and apply(quick, 1) > 0

    # The extra chamber consumes the first round at the action start. The
    # next action carries the current count after any additional inputs.
    send(peer, 47, 3)
    assert ammo(peer) == 2
    send(peer, 48, 1)
    short_wyvern, = consume_shots(peer, 2)
    assert ammo(peer) == 1 and apply(short_wyvern, 1) > 0

    send(other, 59, 3)
    assert not consume_shots(other, 1)
    wyvern, = consume_shots(other, 2)
    assert ammo(other) == 3 and not consume_shots(other, 2)
    pulses = []
    for _ in range(4):
        g.run(0x476564, wyvern)
        pulses.append(apply(wyvern, 2))
    last = g.half(wyvern + 5084)
    g.run(0x476564, wyvern)
    assert g.half(wyvern + 5084) == last  # one initial contact and three native pulses

    assert (normal_damage, charged_damage) == (15, 15)
    assert burst_damage == [18] * 6 and pulses == [40] * 4
    half(g, g.get(peer + 5060) + 800 + 2 * 27, 2)
    g.cpu.mem_write(peer + 11140, b"\x03")
    send(peer, 43, 3)
    wide = consume_shots(peer, 50)
    assert len(wide) == 3 and ammo(peer) == 0 and not consume_shots(peer, 52)
    assert len({apply(shell, 1) for shell in wide}) == 1
    if 0x2864C4 in g.traps:
        g.cpu.hook_del(g.traps.pop(0x2864C4))
    g.stub(0x2864C4, lambda actor, skill, *_: int(actor == sender and skill == 175))
    send(peer, 28, 2, state_value=7)
    assert ammo(peer) == 0
    assert struct.unpack("<f", g.cpu.mem_read(peer + 5296, 4)) == (1.5,)
    send(other, 28, 2, marked=False, state_value=7)
    assert struct.unpack("<f", g.cpu.mem_read(other + 5296, 4)) == (1.,)
    print("GL: mixed-weapon room, per-peer ammo, reload, normal/charged shells, full burst and Wyvern HP passed")


def number_reg(g, value):
    g.cpu.reg_write(arm.UC_ARM_REG_S0, struct.unpack("<I", struct.pack("<f", value))[0])


if __name__ == "__main__":
    sword_shield()
    gunlance()
