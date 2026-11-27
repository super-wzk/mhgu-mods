"""Replay the packaged Magnet Spike through GU's normal hunter dispatcher."""

import json
import math
import struct
import sys

from unicorn.arm_const import UC_ARM_REG_S0, UC_ARM_REG_S1, UC_ARM_REG_SP

from verify_magnet_spike import PLUGIN, REPO
from verify_peer import capture_action, capture_position, verify_peer
from damage_target import damage_target
from verify_style import (
    BLOCK, CHANGE_ACTION, DECODE, DISPATCH, ENCODE, INIT_HUNTER, MANAGER, SAVE, SETTLE_GUARD,
    WEAPON_DISPATCH, boot, character_save, select_character, setup_controller,
)

sys.path.insert(0, str(REPO / "tools/tests"))
from verify_packed_release import assets

PRIVATE = 0x10000
WEAPON = 16
X, A = 0x20, 0x100
SPECIAL = 0x1000


def replay():
    saved = json.loads(character_save({}))
    saved["data"]["inventory"] = {
        "instances": {"1": {"equipment": 0x1001, "upgrade": 0}}, "equipped": 1, "sets": {},
    }
    g = boot((REPO / "mhgu/main.elf").read_bytes(), json.dumps(saved).encode(), ("registry",))
    exports = g.load(PLUGIN, 0x9200000)
    for callback in ("forge_onLoad", "forge_onInit"):
        g.run(exports[callback], MANAGER)
    assert SAVE in g.files
    select_character(g, 0)

    manager, actor, scratch = (g.alloc(n) for n in (40032, 14000, 1376))
    equipment = manager + 124
    g.put(0x1885B58, manager)
    g.put(manager + 28, actor)
    vtable = g.alloc(2224)
    # Exercise GU's shared weapon dispatch with the fallback's original virtual table.
    g.cpu.mem_write(vtable, bytes(g.cpu.mem_read(0x174D838, 2224)))
    g.put(actor, vtable)
    g.put(actor + 240, g.alloc(128))
    g.put(actor + 5060, equipment)
    g.put(actor + 5068, 1)
    g.put(actor + 5104, 1)
    g.cpu.mem_write(actor + 5078, struct.pack("<h", 100))
    g.stub(0x3F8398, lambda *_: 0)
    g.stub(0x65FA04, lambda *_: 0x800)
    set_action = setup_controller(g, actor)
    verify_weapon_hud(g, actor, equipment, scratch)
    meter = setup_magnet_hud(g)
    assert meter() == ([16.] * 5, 0)
    assert meter(flashes=True) == []
    g.put(actor + 5060, scratch)
    assert meter() == ([], 0)
    g.put(actor + 5060, equipment)
    assert meter() == ([16.] * 5, 0)
    g.cpu.hook_del(g.traps.pop(0x2A2560))
    g.cpu.hook_del(g.traps.pop(0x2E9D54))
    g.stub(0x2A8B34, lambda *_: 0)  # No Hunter Art queued in this replay.
    retail, changes, motions, finishes = [], [], [], []
    ground = g.alloc(4)
    g.put(vtable + 792, ground)
    g.stub(ground, lambda *_: retail.append("ground"))
    g.stub(g.originals[WEAPON_DISPATCH], lambda *_: retail.append("weapon"))

    def change(_, group, slot, flags):
        changes.append((group, slot, flags))
        set_action(group, slot)
        return 1

    g.stub(g.originals[CHANGE_ACTION], change)

    def motion(_, layer, request, flags):
        assert layer == flags == 0
        start = g.cpu.reg_read(UC_ARM_REG_S1)
        blend = g.cpu.reg_read(UC_ARM_REG_S0)
        motions.append((request ^ PRIVATE, *struct.unpack("<2f", struct.pack("<2I", start, blend))))
        g.put(actor + 1204, request & 0xffff)
        return 1

    g.cpu.hook_del(g.traps.pop(0x53A06C))
    g.stub(0x53A06C, motion)

    def finish(*_):
        finishes.append(g.half(actor + 9482))
        set_action(1, 0)

    g.stub(0x2B6E5C, finish)

    g.put(0x188C670, g.alloc(27704))
    for address in (0x27D5C4, 0x3A5ED0, 0x5584A0, 0x27FE18):
        g.stub(address, lambda *_: 0)

    def tick(buttons=0, *, held=0, common=0, frame=0., ended=False, fallback=0, native_pad=False):
        g.put(actor + 5112, buttons)
        g.put(actor + 5140, common)
        g.put(actor + 5156, fallback)
        g.cpu.mem_write(actor + 1280, struct.pack("<f", frame))
        g.cpu.mem_write(actor + 1206, bytes([4 * ended]))
        if native_pad:
            g.cpu.mem_write(actor + 5108, struct.pack("<3H", held | buttons, 0, buttons))
            g.put(actor + 5188, 0)
            g.run(0x27CA28, actor)
        before = len(retail)
        g.run(DISPATCH, actor)
        return len(retail) == before

    set_action(1, 0)
    g.run(CHANGE_ACTION, actor, WEAPON, 3, 0x80)
    assert not changes
    tick(fallback=1 << 14)
    assert retail == ["ground"] and not motions

    tick(X)
    assert changes[-1] == (WEAPON, 0x8000 + 78, 0x80)
    assert motions == [(1440, 0., 0.)] and g.get(actor + 10057) & 1, motions
    tick(X, frame=44.)  # Pre-input on the first clip of draw state 78.
    assert motions == [(1440, 0., 0.)]
    tick(frame=60., ended=True)
    assert motions[-1] == (1049, 38., 0.)
    tick(frame=63.)
    assert motions[-1] == (1049, 38., 0.)
    tick(frame=64.)
    assert g.half(actor + 9482) == 0x8000 + 3 and motions[-1] == (1401, 0., 0.)
    tick(frame=13.)
    assert motions[-1] == (1401, 0., 0.)
    tick(frame=14., ended=True)
    assert motions[-1] == (1050, 4., 0.)
    tick(frame=110., ended=True)
    assert finishes == [0x8000 + 3] and g.half(actor + 9478) == 1

    tick(SPECIAL, native_pad=True)  # GU special attack maps to the FZ mode command.
    assert motions[-1] == (1456, 0., 0.)
    tick(frame=99., ended=True)
    tick(A)
    assert g.half(actor + 9482) == 0x8000 + 32 and motions[-1][0] == 1072
    tick(frame=100., ended=True)
    tick(SPECIAL, native_pad=True)
    assert motions[-1] == (1457, 0., 0.)
    tick(frame=99., ended=True)
    tick(A)
    assert g.half(actor + 9482) == 0x8000 + 6 and motions[-1][0] == 1048

    set_action(1, 0)
    tick(X)
    assert motions[-1] == (1049, 10., 2.)
    g.cpu.mem_write(actor + 10500, struct.pack("<f", 1.))  # GU stamina waiver.
    tick(common=1 << 3, frame=61.)
    assert g.half(actor + 9478) == WEAPON
    tick(common=1 << 3, frame=62.)
    assert (g.half(actor + 9478), g.half(actor + 9482)) == (1, 6)

    set_action(1, 0)
    g.cpu.mem_write(actor + 10057, b"\0")
    tick(X | A, native_pad=True)
    assert changes[-1] == (WEAPON, 0x8000 + 79, 0x80) and motions[-1][0] == 1023
    assert retail == ["ground"]

    # A fixed equipment record immediately returns to GU's action handlers.
    g.put(actor + 5060, scratch)
    set_action(1, 0)
    tick(X)
    assert retail == ["ground", "ground"]
    g.run(CHANGE_ACTION, actor, WEAPON, 3, 0x80)
    assert changes[-1] == (WEAPON, 3, 0x80)

    # Start with an empty resource manager to verify mount order. Keep the ARC
    # slot headers and rebase the pointers as GU does while loading rMotionList.
    g.put(actor + 5060, equipment)
    resources = assets(PLUGIN.parents[1] / "arc/frontier/magnet_spike.arc")
    shapes = {}
    for address in (0x7AF37C, 0x7AF3A0):
        g.stub(address, g.alloc)
    getter = g.alloc(4)
    g.stub(getter, lambda shape, index, *_: g.get(g.get(shape + 104) + 4 * index))
    for (name, kind), data in resources.items():
        if kind != 0x5653C1B0:
            continue
        obj, raw, vt = (g.alloc(n) for n in (112, len(data), 72))
        g.cpu.mem_write(raw, data)
        g.put(obj, vt)
        g.put(vt + 68, getter)
        g.put(obj + 80, 1)
        g.put(obj + 100, raw)
        assert g.run(0x16A1D8, obj) == 1
        shapes[name] = obj
    res_manager, res_vtable, load, release = (g.alloc(n) for n in (128, 64, 4, 4))
    g.put(res_manager, res_vtable)
    g.put(res_vtable + 48, load)
    g.put(res_vtable + 60, release)
    g.stub(release, lambda *_: 0)  # Fixture resources live through the replay.
    g.put(0x2126A64, res_manager)
    paths = []
    audio_banks = {}
    sounds = []
    pel_name = r"effect\frontier_ms\particles"
    flash_name = r"GUI\frontier_magnet_spike\flash"
    pel = g.alloc(176)
    g.put(pel + 80, 1)

    def resource(_, kind, path, flags):
        name = g.string(path)
        paths.append(name)
        if kind == 0x1889FDC:
            assert flags == 2 and name == pel_name
            assert resources[name, 0x5A525C16].startswith(b"PEL\0")
            return pel
        if kind == 0x21255D0:
            assert flags == 2 and name == flash_name
            data = resources[name, 0x241F5DEB]
            assert data[:4] == b"TEX\0"
            dimensions, = struct.unpack_from("<I", data, 8)
            obj = g.alloc(228)
            g.put(obj + 80, 1)
            g.put(obj + 220, dimensions >> 6 & 0x1fff)
            g.put(obj + 224, dimensions >> 19)
            return obj
        if kind == 0x2125BEC:
            assert flags == 2  # Hunt preload uses the asynchronous native request.
            request = resources[name, 0x1BCC4966]
            cues = {struct.unpack_from("<H", request, 52 + 132 * i)[0]
                    for i in range(struct.unpack_from("<I", request, 8)[0])}
            obj = g.alloc(176)
            g.put(obj + 80, 1)
            audio_banks[obj] = (name.rsplit("\\", 1)[-1], cues)
            return obj
        assert flags == 1
        if kind == 0x1857810:
            return shapes[name]
        obj = g.alloc(176)
        g.put(obj + 80, 1)
        if kind == 0x2124DF0:
            assert name == r"arc\frontier\magnet_spike"
        else:
            assert kind == 0x2125210
            lmt = resources[name, 0x76820D81]
            count = struct.unpack_from("<H", lmt, 6)[0]
            size = 8 + 4 * count
            directory = bytearray(lmt[:size])
            header = g.alloc(size + 16 * count)
            for slot in range(count):
                field = 8 + 4 * slot
                offset, = struct.unpack_from("<I", lmt, field)
                if offset:
                    struct.pack_into("<I", directory, field, header + len(directory))
                    directory.extend(lmt[offset:offset + 16])
            g.cpu.mem_write(header, bytes(directory))
            g.put(obj + 100, header)
        return obj

    def queue(_, kind, bank, cue):
        assert kind == 7
        name, allowed = audio_banks[bank]
        assert cue in allowed
        stack = g.cpu.reg_read(UC_ARM_REG_SP)
        owner, point = g.get(stack), g.get(stack + 4)
        sounds.append((owner, name, cue - 4096, struct.unpack("<3f", g.cpu.mem_read(point, 12))))
        return 1

    g.stub(load, resource)
    g.put(0x1896F5C, g.alloc(512))
    g.stub(0xB68280, lambda bank, cue, *_: int(cue in audio_banks[bank][1]))
    g.stub(0x4EEAA4, queue)
    g.stub(g.originals[INIT_HUNTER], lambda *_: 7)
    assert g.run(g.hooks[INIT_HUNTER], actor) == 7
    assert paths == ([r"arc\frontier\magnet_spike"]
                     + [rf"player\mot\frontier_ms\{g}" for g in (0, 2, 4)]
                     + [rf"player\hit\frontier_ms\melee_{i}" for i in range(1, 9)]
                     + [pel_name, flash_name]
                     + [rf"sound\se\pl\frontier_ms\{name}" for name in ("blade", "common")])
    packed = g.run(ENCODE, actor, PRIVATE | 1440)
    assert packed & 255 == 40 and g.run(DECODE, actor, packed) == PRIVATE | 1440
    for clip in (1014, 1015, 1016, 1017, 1022, 1407, 1408, 1429, 1444, 1451, 1452, 1460, 1461, 1471, 1472, 1473):
        encoded = g.run(ENCODE, actor, PRIVATE | clip)
        assert g.run(DECODE, actor, encoded) == PRIVATE | clip, clip

    part = g.alloc(5152)
    g.put(part + 48, actor)
    g.put(part + 240, g.alloc(128))
    g.put(actor + 9120, part)
    g.put(part + 1204, 0xffff)
    tracks = []

    def track(owner, layer, clip, flags):
        assert owner == part and flags == 0
        floats = struct.pack("<2I", *(g.cpu.reg_read(r) for r in (UC_ARM_REG_S0, UC_ARM_REG_S1)))
        tracks.append((layer, clip & 255, *struct.unpack("<2f", floats)))
        g.put(part + 1204 + 320 * layer, clip)
        return 1

    g.stub(0x954640, track)
    g.put(actor + 1204, packed)
    g.cpu.mem_write(actor + 1268, struct.pack("<f", 2.))
    g.cpu.mem_write(actor + 1280, struct.pack("<f", 12.5))
    update_part = g.hooks[0x3078B8]
    g.stub(g.originals[0x3078B8], lambda *_: 7)
    assert g.run(update_part, part, actor, WEAPON) == 7
    part_bank = r"weapon\frontier\magnet_spike\mot\4"
    assert paths[-1] == part_bank, paths
    assert tracks == [(0, 40, 2., 12.5), (1, 255, 0., 0.)], tracks
    assert g.half(part + 1524) == 0xffff and g.get(part + 1212) & 16
    g.cpu.mem_write(actor + 1280, struct.pack("<f", 14.))
    g.run(update_part, part, actor, WEAPON)
    assert len(tracks) == 2  # Sampling the same clip does not restart it.
    assert struct.unpack("<f", g.cpu.mem_read(part + 1276, 4)) == (14.,)
    assert paths.count(part_bank) == 1

    packed = g.run(ENCODE, actor, PRIVATE | 1435)
    g.put(actor + 1204, packed)
    # Exercise the part's own loop metadata by sampling past its end.
    g.cpu.mem_write(actor + 1280, struct.pack("<f", 100.))
    g.run(update_part, part, actor, WEAPON)
    assert tracks[-1] == (0, 35, 2., 61.), tracks
    verify_melee(g, actor, vtable, part, shapes, set_action, tick, meter, motions, sounds)


def verify_melee(g, actor, vtable, part, shapes, set_action, tick, meter, motions, sounds):
    def byte(address):
        return g.cpu.mem_read(address, 1)[0]

    # Keep the native HDP constructor and collision expansion. Supply the
    # matrices GU would have evaluated from the two moving blade joints.
    monster, status, monster_vt, hurt, collision, credit = damage_target(g, vtable, native_affinity=True)
    dti = g.run(g.get(vtable + 20))
    g.put(0x1898674, g.get(dti + 4))
    g.stub(0x13EFDC4, lambda ptr, size, *_: g.cpu.mem_write(ptr, bytes(size)))
    for address in (0x2AD288, 0x27B918, 0x2864E0, 0x27C90C):
        g.stub(address, lambda *_: 0)
    for address, fn in ((0x13EFDE8, math.sin), (0x13EFDF4, math.cos)):
        def trig(*_, fn=fn):
            angle, = struct.unpack("<f", struct.pack("<I", g.cpu.reg_read(UC_ARM_REG_S0)))
            bits, = struct.unpack("<I", struct.pack("<f", fn(angle)))
            g.cpu.reg_write(UC_ARM_REG_S0, bits)
        g.stub(address, trig)

    mapping, matrices = g.alloc(256), g.alloc(3 * 160)
    g.put(part + 1172, matrices)
    g.put(part + 1176, mapping)
    joints = bytearray([255] * 256)
    joints[4], joints[5], joints[7] = 0, 1, 2
    g.cpu.mem_write(mapping, bytes(joints))
    rotation = (1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0)
    for i, x in enumerate((60., 100., 240.)):
        g.cpu.mem_write(matrices + 16 + i * 160, struct.pack("<16f", *rotation, x, 90., 120., 1.))
    hit, second = actor + 12320, actor + 12504
    for contact in (hit, second):
        g.run(0x1687A8, contact)
    assert g.get(actor + 12696) == g.get(actor + 12700) == 0
    set_action(1, 0)
    g.cpu.mem_write(actor + 10057, b"\0")
    tick(X, native_pad=True)
    assert g.half(actor + 9482) == 0x8000 + 78
    tick(frame=60., ended=True)
    assert g.half(actor + 9482) == 0x8000 + 78
    tick(frame=39.)
    assert byte(hit + 5) == 0
    tick(frame=42.)
    assert byte(hit + 5) == 1 and g.get(hit + 12) == g.get(hit + 112) == actor
    actual = g.half(hit + 56), byte(hit + 86), byte(hit + 108)
    assert actual == (43, 0, 1), actual
    assert struct.unpack("<2f", g.cpu.mem_read(hit + 92, 8)) == (0., 12.)
    assert g.get(hit + 132) == shapes[r"player\hit\frontier_ms\melee_1"]
    assert g.get(actor + 12700) == 0
    serial = g.half(hit + 52)
    tick(frame=42.)
    assert g.half(hit + 52) == serial and byte(second + 5) == 0

    for x in (240., 300.):
        g.cpu.mem_write(matrices + 2 * 160 + 64, struct.pack("<f", x))
        g.put(collision + 10288, 0)
        assert g.run(0x1696FC, hit) == 2
        first = struct.unpack("<B3x7f", g.cpu.mem_read(collision + 8240, 32))
        other = struct.unpack("<B3x7f", g.cpu.mem_read(collision + 8272, 32))
        assert first == (2, 50., 60., 90., 120., 100., 90., 300.), first
        assert other == (2, 50., x + 40., 90., 120., x, 90., 300.), other

    # 169548 must scale by the effective melee attack from the equipment
    # calculation. It would otherwise return a constant 1.0.
    equipment = g.get(actor + 5060)
    g.cpu.mem_write(equipment + 800, struct.pack("<h", 150))
    g.run(0x169548, hit)
    assert struct.unpack("<f", struct.pack("<I", g.cpu.reg_read(UC_ARM_REG_S0))) == (1.5,)
    xyz = g.cpu.mem_read(collision + 8248, 12)
    g.cpu.mem_write(collision + 28, struct.pack("<B3xf", 1, 2.) + bytes(xyz))
    g.run(0x16C0C0, collision, hit, monster)
    damage = g.half(status + 1160)
    assert damage > 0 and credit[-1] == (0, 0, damage), credit
    g.run(0x97DDC, monster)
    assert g.get(status + 880) == 1000 - damage

    # The scene clock advances by two frames at playback speed 1.0.
    g.cpu.mem_write(actor + 1292, struct.pack("<f", 1.))
    for _ in range(6):
        g.run(0x16919C, hit)
        assert byte(hit + 5) == 1
    g.run(0x16919C, hit)
    assert byte(hit + 5) == 0
    set_action(1, 0)
    tick(SPECIAL, native_pad=True)
    tick(frame=99., ended=True)
    tick(X | A, native_pad=True)
    assert g.half(actor + 9482) == 0x8000 + 26
    tick(frame=120., ended=True)
    tick(frame=88.)
    tick(frame=102.)
    assert (g.half(hit + 56), g.half(second + 56)) == (96, 121)
    assert (byte(hit + 86), byte(second + 86)) == (50, 65)
    assert g.get(hit + 64) & 3 == g.get(second + 64) & 3 == 1
    assert g.half(hit + 52) != g.half(second + 52)
    shot_data, flight_packets = verify_shot(
        g, actor, monster, status, monster_vt, hurt, collision, credit, set_action, tick, meter, sounds,
    )
    verify_magnetic_charge(g, actor, monster, hurt, collision, set_action, tick, meter)
    verify_deflection(g, actor, vtable, monster, hurt, set_action, tick, motions)
    boost_packets = verify_magnetic_boost(g, actor, monster, collision, set_action, tick, meter)
    guard_packets, effects = verify_guard(g, actor, set_action, tick, meter, motions, sounds)
    verify_peer(g, actor, vtable, part, monster, status, collision, credit, set_action, tick, meter,
                flight_packets, boost_packets, guard_packets, shot_data, sounds, effects)


def verify_guard(g, actor, set_action, tick, meter, motions, sounds):
    shoulder = 1 << 27
    skills = set()
    g.cpu.hook_del(g.traps.pop(0x2864C4))
    g.stub(0x2864C4, lambda _actor, skill, *_: int(skill in skills))
    damage = g.alloc(8)
    effects = []
    wet = False

    def submit(_, unit, handle, group):
        assert handle == group == 1
        selector = g.get(g.cpu.reg_read(UC_ARM_REG_SP))
        parent = g.get(unit + 320)
        if parent:
            assert g.half(unit + 194) == 8 and g.get(unit + 132) & 0x80
        offset = unit + (144 if parent else 304)
        point = struct.unpack("<3f", g.cpu.mem_read(offset, 12))
        effects.append((selector, parent, point))
        return 1

    def ground(_, owner, point, previous):
        assert point != 0 and owner != 0
        sp = g.cpu.reg_read(UC_ARM_REG_SP)
        g.cpu.mem_write(g.get(sp + 4), struct.pack("<f", -100.))
        g.cpu.mem_write(g.get(sp + 8) + 20, struct.pack("<f", 0. if wet else -100001.))
        return 1

    get_parent = g.alloc(4)
    g.put(g.get(actor) + 304, get_parent)
    g.stub(get_parent, lambda owner, *_: owner + 600)
    for address, callback in ((0x3267D0, submit), (0x18192C, ground)):
        g.cpu.hook_del(g.traps.pop(address))
        g.stub(address, callback)

    def state():
        return g.half(actor + 9482)

    def at(frame, buttons=0):
        # Poison the stock weapon command bank each time through the dispatcher.
        assert tick(buttons, frame=frame, native_pad=True, fallback=1 << 14)

    def start(*, sheathed=False):
        set_action(1, 0)
        g.cpu.mem_write(actor + 10057, bytes([not sheathed]))
        tick(native_pad=True)
        assert tick(X if sheathed else 0, common=shoulder, held=0x400, native_pad=True,
                    fallback=1 << 14)
        g.cpu.mem_write(actor + 1280, struct.pack("<f", 0.))
        g.put(actor + 11072, 0)

    def probe(frame, *, angle=0, flags=0, kind=0, power=100, accepted=1, settle=True):
        g.cpu.mem_write(actor + 1280, struct.pack("<f", frame))
        g.put(actor + 5280, 0)
        g.cpu.mem_write(actor + 12820, struct.pack("<HBxHHh", flags, kind, angle, 0xffff, power))
        assert g.run(BLOCK, actor, angle) == accepted
        g.put(damage, power & 0xffffffff)
        g.put(damage + 4, 0xff)
        if settle:
            assert g.run(SETTLE_GUARD, actor, kind, damage, damage + 4) == accepted
            assert g.get(damage + 4) == 0
            assert g.get(damage) == (0 if accepted else power & 0xffffffff)

    set_action(1, 0)
    g.cpu.mem_write(actor + 10057, b"\0")
    tick(native_pad=True)  # Reset to slash while sheathed.
    g.put(actor + 5088, 0)  # No stamina is needed for either window.
    g.put(actor + 10100, 100)
    hp, gauge = g.half(actor + 5078), meter()
    start()
    assert state() == 0x8000 + 14 and motions[-1] == (1034, 0., 6.), (state(), motions[-1])
    probe(4., angle=0x4000, settle=False)
    assert g.get(actor + 11072) == 0
    probe(4., angle=0x4001, accepted=0)
    probe(4., flags=0x20, accepted=0)
    probe(4., flags=0x100, accepted=0)
    probe(4., kind=0x19, accepted=0)
    probe(4., power=-1, accepted=0)
    probe(26., accepted=0)
    at(26.)
    assert state() == 0x8000 + 14  # Collision probe alone cannot trigger a counter.
    assert not effects
    at(32.)
    assert g.half(actor + 12320 + 56) == 30

    for frames, follow_up in (((4.,), 57), ((4., 14.), 76), ((4., 14., 24.), 77)):
        start()
        first_sound = len(sounds)
        first_effect = len(effects)
        for frame in frames:
            # Multiple queries of the same hit precede one native settlement.
            probe(frame, settle=False)
            probe(frame)
            assert struct.unpack("<f", g.cpu.mem_read(actor + 11072, 4)) == (8.,)
        at(25.)
        assert state() == 0x8000 + 14
        at(26.)
        assert state() == 0x8000 + follow_up and motions[-1] == (1034, 30., 0.)
        assert struct.unpack("<f", g.cpu.mem_read(actor + 11072, 4)) == (28.,)
        assert [cue for _, name, cue, _ in sounds[first_sound:]
                if name == "blade" and cue in (12, 13)] == [13, 12]
        parent = actor + 600
        assert effects[first_effect:] == (
            [(i, parent, (30., 0., -70.)) for i in range(13, 26)]
            + [(i, parent, (40., 0., 100.)) for i in range(26, 37)]
            + [(i, parent, (0., 0., 0.)) for i in range(93, 99)]
        )
        at(32.)
    slash_counter = capture_action(g, actor)

    for skill, closes in ((169, 28.), (168, 30.)):
        start()
        skills.add(skill)
        probe(closes - 1.)
        probe(closes, accepted=0)
        skills.clear()

    start(sheathed=True)
    assert state() == 0x8000 + 112 and motions[-1] == (1068, 0., 1.)
    assert g.get(actor + 10057) & 1
    probe(32., accepted=0)
    skills.add(171)
    probe(31., flags=0x20, settle=False)
    skills.clear()
    # Run GU's full received-hit handler, with a queued hit but no chip.
    g.cpu.mem_write(actor + 12768, b"\x01")
    g.cpu.mem_write(actor + 12820, struct.pack("<HBxHHh", 0, 0, 0, 0xffff, 100))
    g.cpu.mem_write(actor + 64, struct.pack("<3f", 400., 10., 500.))
    g.cpu.mem_write(actor + 4076, struct.pack("<H", 0x4000))
    first_effect = len(effects)
    assert g.run(0x2EE508, actor) == 1 and state() == 0x8000 + 112
    assert effects[first_effect:] == (
        [(i, 0, (500., 130., 500.)) for i in range(37, 56)]
        + [(i, 0, (500., 10., 500.)) for i in range(56, 70)]
        + [(i, actor + 600, (0., 0., 0.)) for i in range(99, 105)]
    )
    g.cpu.mem_write(actor + 12768, b"\0")
    at(31.)
    assert state() == 0x8000 + 113 and motions[-1] == (1490, 0., 0.)
    impact_guard = capture_action(g, actor)
    at(25., X)
    assert state() == 0x8000 + 113
    at(26., X)
    at(55.)
    assert state() == 0x8000 + 113
    at(56.)
    assert state() == 0x8000 + 114 and motions[-1] == (1410, 0., 0.)

    start()
    assert state() == 0x8000 + 112  # Still in impact mode with the weapon drawn.
    skills.add(168)
    probe(36., accepted=0)
    wet = True
    first_effect = len(effects)
    probe(35.)
    assert [selector for selector, _, _ in effects[first_effect:]] == (
        list(range(37, 56)) + list(range(70, 93)) + list(range(99, 105))
    )
    wet = False
    skills.clear()
    at(35.)
    at(26., X | A)
    at(56.)
    assert state() == 0x8000 + 115 and motions[-1] == (1410, 0., 0.)
    impact_assault = capture_action(g, actor)
    assert g.half(actor + 5078) == hp
    assert g.get(actor + 5088) == 0 and g.get(actor + 10100) == 100
    assert meter() == gauge

    start()
    assert state() == 0x8000 + 112
    tick(frame=170., ended=True, common=shoulder)
    assert state() == 0
    tick(common=shoulder)
    assert state() == 0  # Holding R across recovery does not start another guard.
    tick()
    tick(common=shoulder)
    assert state() == 0x8000 + 112
    return (slash_counter, impact_assault, impact_guard), effects


def verify_shot(g, actor, monster, status, monster_vt, hurt, collision, credit, set_action, tick, meter, sounds):
    def put_vec(address, *vec):
        g.cpu.mem_write(address, struct.pack(f"<{len(vec)}f", *vec))

    def position(address):
        return struct.unpack("<3f", g.cpu.mem_read(address, 12))

    def matrix(address, x, y, z):
        put_vec(address, 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., x, y, z, 1.)

    handle = g.alloc(16)
    g.put(handle + 4, 1)
    g.put(0x188A0D4, g.alloc(4))
    g.put(0x2126F48, g.alloc(4))
    g.stub(0x3268D8, lambda *_: handle)
    g.stub(0x326C78, lambda *_: 0)
    g.stub(0x3288C4, lambda size, *_: g.alloc(size))
    g.stub(0x328900, lambda *_: 0)
    layers = {}
    repel_effects = []
    get_parent = g.alloc(4)
    g.put(g.get(actor) + 304, get_parent)
    g.stub(get_parent, lambda owner, *_: owner + 600)

    def spawn(_, unit, handle_id, group):
        assert handle_id == group == 1
        selector = g.get(g.cpu.reg_read(UC_ARM_REG_SP))
        if selector >= 105:
            parent = g.get(unit + 320)
            if parent:
                assert g.half(unit + 194) == 8
            repel_effects.append(
                (selector, parent, position(unit + (144 if parent else 304)), position(unit + 176))
            )
            return 1
        child = g.alloc(80)
        g.put(unit + 336, child)
        put_vec(child + 64, *position(unit + 304))
        layers[unit] = (selector, child + 64)
        return 1

    def stop(unit, immediate, *_):
        assert immediate == 1
        del layers[unit]

    def visuals():
        return [(selector, position(child)) for selector, child in layers.values()]

    g.stub(0x3267D0, spawn)
    g.stub(0x32A400, stop)

    mapping, matrices = g.alloc(256), g.alloc(160)
    g.put(actor + 1172, matrices)
    g.put(actor + 1176, mapping)
    g.cpu.mem_write(mapping, bytes([255] * 12 + [0] + [255] * 243))
    matrix(matrices + 16, 25., 100., 50.)
    camera_manager, camera = g.alloc(128), g.alloc(128)
    g.put(0x2126504, camera_manager)
    g.put(camera_manager + 84, camera)
    put_vec(camera + 64, 0., 0., 0.)
    put_vec(camera + 96, 0., 0., 1000.)

    # Keep the engine's monster and joint lookup, sphere expansion and
    # sphere/capsule intersection. Isolate background and scene scheduling.
    # GU may still be settling a melee hit. Geometric marks are independent.
    roster = g.alloc(9520)
    g.put(0x1851FE0, roster)
    g.put(roster + 28, monster)
    for address in (monster + 46585, monster + 30290, status + 416):
        g.cpu.mem_write(address, b"\1")
    put_vec(status + 428, 1., 1.)
    g.cpu.mem_write(status + 438, struct.pack("<H", 0x1234))
    g.cpu.mem_write(status + 23744, b"\7")
    pose, resolve_joint = g.alloc(64), g.alloc(4)
    matrix(pose, 25., 100., 1100.)
    joints = []
    g.put(monster_vt + 208, resolve_joint)
    def part_joint(owner, bone, *_):
        joints.append((owner, bone))
        return pose
    g.stub(resolve_joint, part_joint)
    g.cpu.mem_write(hurt, struct.pack("<7H", 5, 0, 0, 0, 0, 0, 3))
    put_vec(hurt + 16, 65., 0., 0., 0.)

    rays = []
    wall = False
    water = None
    g.put(0x21270E0, g.alloc(4))
    def terrain(owner, start, end, area, *_):
        assert owner == actor and area == 0
        rays.append((position(start), position(end)))
        return wall
    g.stub(0x49C0C, terrain)
    def surface(_, owner, point, previous):
        assert owner == actor and position(point) == position(previous)
        if water is None:
            return 0
        sp = g.cpu.reg_read(UC_ARM_REG_SP)
        assert g.get(sp) == 0
        put_vec(g.get(sp + 4), -100.)
        put_vec(g.get(sp + 8) + 20, water)
        return 1
    g.stub(0x18192C, surface)
    g.stub(g.originals[0x16ABAC], lambda *_: 3)

    def update():
        assert g.run(g.hooks[0x16ABAC], collision) == 3

    # Use the native process lookup, position sender and SDK packet hook.
    previous_network = g.get(0x18843F0)
    network, room, process = (g.alloc(n) for n in (1604, 240, 48))
    g.put(0x18843F0, network)
    g.put(network + 520, room)
    g.put(room + 232, 2)
    g.put(network + 992 + 4 * 6, process)
    g.put(network + 992 + 4 * 68, process)
    g.put(process + 16, 1)
    g.put(process + 36, 1)
    g.stub(0x49694, lambda *_: 1)
    events = []
    mark_events = []
    transport = g.originals[0x9FC3D8]

    def send(sender, target, packet, priority):
        assert (sender, target) == (process, 1)
        assert g.get(g.cpu.reg_read(UC_ARM_REG_SP)) == 4160
        if priority == 16:
            assert g.get(packet) == 0x1741604
            mark = bytes(g.cpu.mem_read(packet, 48))
            assert mark[11:24] == b"\x09\x34\x12\x07\x00MHT1\x10\x00\x00\x00"
            assert struct.unpack_from("<HB", mark, 36) == (3, 0)
            mark_events.append(mark)
            return
        assert priority == 4 and g.get(packet) == 0x17423E4
        assert g.get(packet + 44) >> 31 == 1
        assert g.cpu.mem_read(packet + 16, 12) == g.cpu.mem_read(actor + 64, 12)
        events.append(bytes(g.cpu.mem_read(packet, 48)))

    g.stub(transport, send)
    hp, credit_count = g.get(status + 880), len(credit)
    held = 0x800
    set_action(1, 0)
    first_sound = len(sounds)
    tick(A, held=held, native_pad=True)
    assert g.half(actor + 9482) == 0x8000 + 84
    shot_packet = capture_action(g, actor)
    tick(frame=6.)  # Snapshots the camera direction even if it moves before release.
    put_vec(camera + 96, 1000., 0., 0.)
    tick(frame=11.)
    update()
    assert not rays and not joints
    assert not visuals()
    assert not events
    assert meter()[1] == 0
    assert [cue for _, bank, cue, _ in sounds[first_sound:]] == [8, 74]
    tick(frame=12.)
    tick(frame=12.)
    assert len(events) == 1
    assert [cue for _, bank, cue, _ in sounds[first_sound:]] == [8, 74, 9]
    assert sounds[-1] == (actor, "blade", 9, (25., 100., 50.))
    assert visuals() == [(i, (25., 100., 50.)) for i in range(8)]
    update()
    assert rays == [((25., 100., 50.), (25., 100., 550.))], rays
    assert visuals() == [(i, (25., 100., 550.)) for i in range(8)]
    update()
    assert rays[-1] == ((25., 100., 550.), (25., 100., 1050.)), rays
    assert len(events) == 2
    assert joints.count((monster, 5)) == 4, joints  # Sweep, attach and sample the mark.
    assert meter()[1] == 1
    hit_point = visuals()[0][1]
    assert hit_point[:2] == (25., 100.) and 1000. <= hit_point[2] <= 1100.
    assert visuals() == [(i, hit_point) for i in range(8, 13)]
    assert len(mark_events) == 1
    assert struct.unpack_from("<3f", mark_events[0], 24) == (0., 0., hit_point[2] - 1100.)
    rays.clear()
    joints.clear()
    matrix(pose, 100., 95., 2150.)
    update()
    assert not rays and joints == [(monster, 5)]
    assert meter()[1] == 2
    follow = (100., 95., hit_point[2] + 1050.)
    assert visuals() == [(i, follow) for i in range(8, 13)]
    # A release from the engine may precede the end of the mark.
    unit = next(iter(layers))
    del layers[unit]
    g.stub(g.originals[0x32A104], lambda *_: 0)
    g.run(g.hooks[0x32A104], unit)
    matrix(pose, 100., 95., 4150.)
    update()
    assert meter()[1] == 0
    g.put(status + 880, 0)
    joints.clear()
    update()
    g.put(status + 880, hp)
    update()
    assert not joints
    assert not visuals()

    set_action(1, 0)
    tick(SPECIAL, native_pad=True)
    tick(frame=99., ended=True)
    tick(A, held=held, native_pad=True)
    assert g.half(actor + 9482) == 0x8000 + 82
    tick(frame=13.)
    update()
    assert not rays
    tick(frame=14.)
    wall = True
    update()
    assert rays == [((25., 100., 50.), (525., 100., 50.))], rays
    assert not visuals()
    update()
    assert len(rays) == 1 and not joints
    assert g.get(status + 880) == hp and len(credit) == credit_count
    assert len(events) == 4

    # A stopped scene freezes flight and marks. A live mark lasts 9000 FZ
    # simulation ticks, and the scene clock counts 60 Hz frames.
    wall = False
    rays.clear()
    matrix(pose, 500., 100., 50.)
    tick(frame=60., ended=True)
    tick(A, held=held, native_pad=True)
    tick(frame=14.)
    clock = g.get(0x2126764) + 104
    put_vec(clock, 0.)
    update()
    assert not rays and not joints
    assert visuals() == [(i, (25., 100., 50.)) for i in range(8)]
    put_vec(clock, 2.)
    update()
    assert len(rays) == 1 and joints == [(monster, 5)] * 3
    assert meter()[1] == 1
    rays.clear()
    joints.clear()
    put_vec(clock, 17998.)
    update()
    assert not rays and joints == [(monster, 5)]
    joints.clear()
    put_vec(clock, 2.)
    update()
    assert not rays and not joints
    assert meter()[1] == 0
    assert not visuals()

    water = 110.
    put_vec(camera + 96, 0., -1000., 1000.)
    tick(frame=60., ended=True)
    tick(A, held=held, native_pad=True)
    tick(frame=14.)
    update()
    assert len(rays) == 1 and not joints
    assert len(events) == 8  # Two launches that hit, one wall, one water.
    assert len(mark_events) == 2
    g.cpu.hook_del(g.traps.pop(transport))
    g.put(0x18843F0, previous_network)
    assert not visuals()
    update()
    assert len(rays) == 1 and not joints

    # Keep GU 539F4C and 2A84D8. Supply a flat floor and control the
    # native virtual movement multiplier without using the game renderer.
    water = None
    vtable = g.get(actor)
    rate_entry, land_entry = (g.alloc(4) for _ in range(2))
    g.put(vtable + 144, rate_entry)
    g.put(vtable + 768, land_entry)
    rate = 1.
    landings = []

    def movement_rate(owner, *_):
        assert owner == actor
        g.cpu.reg_write(UC_ARM_REG_S0, struct.unpack("<I", struct.pack("<f", rate))[0])

    def floor(owner, height, *_):
        assert owner == actor
        put_vec(height, 0.)
        return 1

    g.stub(rate_entry, movement_rate)
    g.stub(land_entry, lambda owner, mode, *_: landings.append((owner, mode)))
    g.stub(0x181EF0, lambda *_: 0)
    g.stub(0x2E7AA0, lambda *_: 0)  # Collision cache refresh on large steps.
    g.stub(0x2E8B04, floor)
    configure = []
    g.cpu.hook_del(g.traps.pop(0x282FD8))
    g.stub(0x282FD8, lambda owner, mode, *_: configure.append(mode))
    put_vec(actor + 28, 2.)
    put_vec(actor + 64, 0., 0., 0.)
    put_vec(actor + 5088, 1000.)
    put_vec(camera + 96, 0., 0., 1000.)
    contacts = (actor + 12320, actor + 12504)
    for contact in contacts:
        g.run(0x1687A8, contact)

    def state():
        return g.half(actor + 9482)

    def attract():
        set_action(1, 0)
        assert tick(held=held, common=1 << 3, native_pad=True, fallback=1 << 14)
        return state()

    def mark_at(distance, shot_frame, *, side=False):
        point = (distance, 100., 50.) if side else (25., 100., distance)
        matrix(pose, *point)
        put_vec(camera + 96, *(1000., 0., 0.) if side else (0., 0., 1000.))
        set_action(1, 0)
        tick(A, held=held, native_pad=True)
        tick(frame=shot_frame)
        for _ in range(math.ceil(distance / 500.)):
            update()

    def repel():
        set_action(1, 0)
        assert tick(SPECIAL, held=held, native_pad=True, fallback=1 << 14)
        return state()

    put_vec(actor + 11072, 0.)
    assert repel() == 0
    mark_at(2250., 14.)
    assert repel() == 0
    put_vec(actor + 64, 0., 0., 50.)
    mark_at(500., 14., side=True)
    put_vec(actor + 5088, 0.)
    assert repel() == 0 and g.get(actor + 11072) == 0
    put_vec(actor + 5088, 1000.)
    g.put(status + 880, 0)
    assert repel() == 0  # Recheck the native monster even between collision ticks.
    g.put(status + 880, hp)
    assert repel() == 0x8000 + 66 and g.half(actor + 1204) == 1429
    repel_packet = capture_action(g, actor)
    facing = g.half(actor + 4076)
    # The selected point is on the hurt sphere, so it can be off the centerline.
    assert facing == g.half(actor + 5280) and abs(facing - 0x4000) < 0x800, facing
    assert configure[-1] == 0
    assert g.get(actor + 11072) == struct.unpack("<I", struct.pack("<f", 28.))[0]

    def check_repel_effect(weapon_offset, pulse_offset, scale):
        point = visuals()[0][1]
        root = position(actor + 64)
        assert not repel_effects
        put_vec(clock, 2.)
        update()
        put_vec(clock, 0.)
        update()
        assert not repel_effects
        put_vec(clock, 2.)
        update()
        pulse = repel_effects[:4]
        assert [selector for selector, _, _, _ in pulse] == list(range(142, 146))
        assert all(parent == 0 and math.isclose(size[0], scale, abs_tol=1e-5)
                   and size == (size[0],) * 3 for _, parent, _, size in pulse), (point, pulse)
        pulse_at = pulse[0][2]
        distance = math.dist(pulse_at, point)
        assert math.isclose(distance, pulse_offset, abs_tol=0.01) and pulse_at[1] < point[1]
        assert all(at == pulse_at for _, _, at, _ in pulse)
        assert [effect[:3] for effect in repel_effects[4:]] == (
            [(i, actor + 600, weapon_offset) for i in range(105, 126)]
            + [(i, 0, point) for i in range(126, 142)]
        )
        update()
        yaw = g.half(actor + 4076) * math.tau / 65536.
        rear = (root[0] - 100. * math.sin(yaw), root[1] + 100., root[2] - 100. * math.cos(yaw))
        assert [i for i, _, _, _ in repel_effects[41:]] == list(range(126, 142))
        assert all(parent == 0 and math.dist(at, rear) < 0.02
                   for _, parent, at, _ in repel_effects[41:])
        update()
        assert len(repel_effects) == 57
        repel_effects.clear()

    check_repel_effect((60., 0., 60.), 200., 1.)

    # Native root displacement is what moves the mark from near to far. A
    # follow-up queued after frame 10 executes at the source's frame 60.
    apply_repel_root(g, actor, 1429, 0, 45)
    tick(held=held, common=1 << 3, frame=45., native_pad=True)
    assert state() == 0x8000 + 66
    tick(frame=59., native_pad=True)
    assert state() == 0x8000 + 66
    apply_repel_root(g, actor, 1429, 45, 60)
    x, y, z = position(actor + 64)
    yaw = facing * 0.000095874
    distance = math.hypot(x, z - 50.)
    assert distance > 1900. and y == 0., (x, y, z)
    assert abs(x + math.sin(yaw) * distance) < 1., (x, y, z)
    assert abs(z - 50. + math.cos(yaw) * distance) < 1., (x, y, z)
    tick(frame=60.)
    assert state() == 0x8000 + 58

    put_vec(actor + 64, 0., 0., 0.)
    set_action(1, 0)
    tick(SPECIAL, native_pad=True)
    tick(frame=99., ended=True)
    mark_at(420., 12.)
    g.cpu.hook_del(g.traps.pop(0x2864C4))
    g.stub(0x2864C4, lambda _, skill, *__: int(skill == 163))
    g.cpu.hook_del(g.traps.pop(0x65FA04))
    g.stub(0x65FA04, lambda *_: 0x400)  # GU's exchanged shoulder layout.
    put_vec(actor + 11072, 0.)
    tick(SPECIAL, held=0x400, frame=61., native_pad=True)
    assert state() == 0x8000 + 84
    tick(SPECIAL, held=0x400, frame=62., native_pad=True)
    assert state() == 0x8000 + 67 and g.half(actor + 1204) == 1444
    g.cpu.hook_del(g.traps.pop(0x65FA04))
    g.stub(0x65FA04, lambda *_: held)
    assert g.get(actor + 11072) == struct.unpack("<I", struct.pack("<f", 48.))[0]
    check_repel_effect((0., 0., 170.), 100., 0.8)
    apply_repel_root(g, actor, 1444, 0, 40)
    assert position(actor + 64)[2] < -1500.
    put_vec(actor + 11072, 23.)
    tick(frame=40., native_pad=True)
    assert g.get(actor + 11072) == struct.unpack("<I", struct.pack("<f", 23.))[0]
    tick(frame=139., ended=True)
    assert state() == 0

    # Three accepts spent one segment apiece. Replenish before the existing
    # attraction and charge cases so they can check their own consumption.
    put_vec(actor + 64, 0., 0., 0.)
    for _ in range(2):
        tick(X, held=held, native_pad=True)
        assert state() == 0x8000 + 38
        tick(frame=200., ended=True)
        set_action(1, 0)
    assert repel() == 0
    put_vec(clock, 18000.)
    update()
    put_vec(clock, 2.)
    tick(SPECIAL, native_pad=True)
    tick(frame=99., ended=True)

    assert attract() == 0  # Expired or missed marks leave the ordinary pose.
    for distance in (250., 4400.):
        mark_at(distance, 14.)
        assert attract() == 0, distance
    mark_at(2250., 14.)
    put_vec(actor + 5088, 0.)
    assert attract() == 0

    put_vec(actor + 5088, 1000.)
    assert attract() == 0x8000 + 58 and g.half(actor + 1204) == 1472
    tick(frame=33., native_pad=True)
    assert position(actor + 64) == (0., 0., 0.) and configure[-1] == 0
    tick(frame=34.)
    assert position(actor + 64) == (0., 0., 0.) and configure[-1] == 2
    tick(frame=36.)
    assert position(actor + 64) == (0., 0., 0.)
    tick(frame=38.)
    first = position(actor + 64)
    assert first[1] > 0. and first[2] > 0. and configure[-1] == 2, first
    matrix(pose, 425., 100., 2250.)
    tick(frame=40.)
    steered = position(actor + 64)
    assert steered[0] - first[0] > 1. and steered[2] > first[2], steered
    rate = 0.
    tick(frame=40.)
    assert position(actor + 64) == steered
    rate = 1.

    tick(X, frame=49., native_pad=True)
    assert state() == 0x8000 + 58
    tick(X, frame=50., native_pad=True)  # The FZ aerial slash window opens at 50.
    assert (state(), g.half(actor + 1204)) == (0x8000 + 59, 1016), (state(), g.half(actor + 1204))
    put_vec(actor + 64, steered[0], 1000., steered[2])
    tick(frame=54.)
    power = [g.half(contact + 56) for contact in contacts]
    assert 200 in power, power  # FZ melee record 16.
    put_vec(actor + 64, steered[0], 0.25, steered[2])
    for frame in range(56, 130, 2):
        tick(frame=frame, ended=True)
        if state() == 0x8000 + 60:
            break
    assert state() == 0x8000 + 60 and g.half(actor + 1204) == 1015
    assert position(actor + 64)[1] == 0. and configure[-1] == 0

    put_vec(actor + 64, 0., 0., 0.)
    set_action(1, 0)
    tick(SPECIAL, native_pad=True)
    tick(frame=99., ended=True)
    mark_at(2250., 12.)
    assert attract() == 0x8000 + 68 and g.half(actor + 1204) == 1460
    tick(frame=34., native_pad=True)
    tick(frame=36.)
    tick(frame=38.)
    assert position(actor + 64)[2] > 0.
    flight_packet = capture_action(g, actor)
    guided_position = capture_position(g, actor)
    last = position(actor + 64)
    wall = True
    tick(frame=40.)
    assert position(actor + 64)[2] == last[2] and g.half(actor + 1204) == 1461
    coast_position = capture_position(g, actor)
    for frame in range(2, 46, 2):
        tick(frame=frame, ended=True)
        if state() == 0x8000 + 75:
            break
    assert state() == 0x8000 + 75 and g.half(actor + 1204) == 1022
    assert position(actor + 64)[1] == 0. and position(actor + 4112) == (0., 0., 0.)

    # Branch directly from impact shot recovery through the source's target
    # condition, then verify both flight attacks and the landing hit.
    wall = False
    for contact in contacts:
        g.run(0x1687A8, contact)
    mark_at(2250., 12.)
    tick(held=held, common=1 << 3, frame=26., native_pad=True)
    assert state() == 0x8000 + 84
    tick(frame=62., native_pad=True)
    assert state() == 0x8000 + 68
    tick(frame=34., native_pad=True)
    tick(frame=36.)
    tick(frame=38.)
    tick(X, frame=50., native_pad=True)
    assert state() == 0x8000 + 73 and g.half(actor + 1204) == 1017
    assert g.half(contacts[0] + 56) == 25
    x, _, z = position(actor + 64)
    put_vec(actor + 64, x, 2., z)
    tick(frame=40.)
    assert g.half(contacts[1] + 56) == 80
    for frame in range(42, 104, 2):
        tick(frame=frame)
        if state() == 0x8000 + 74:
            break
    assert state() == 0x8000 + 74 and g.half(actor + 1204) == 1014
    g.run(0x1687A8, contacts[0])
    tick(frame=8.)
    assert g.half(contacts[0] + 56) == 120

    # Plain B can branch during attraction without entering GU's ground roll.
    # Event 11 runs the native discrete stamina request once.
    spent = []

    def stamina(owner, *_):
        assert owner == actor
        spent.append(struct.unpack("<f", struct.pack("<I", g.cpu.reg_read(UC_ARM_REG_S0)))[0])

    g.stub(0x2A5EBC, stamina)
    put_vec(actor + 64, 0., 0., 0.)
    set_action(1, 0)
    tick(SPECIAL, native_pad=True)
    tick(frame=99., ended=True)
    mark_at(2250., 14.)
    assert attract() == 0x8000 + 58
    tick(frame=34., native_pad=True)
    tick(frame=36.)
    tick(frame=38.)
    tick(common=1 << 3, frame=50., native_pad=True)
    assert state() == 0x8000 + 63 and g.half(actor + 1204) == 1407
    for frame in range(2, 70, 2):
        tick(frame=frame)
        if state() == 0x8000 + 64:
            break
    assert state() == 0x8000 + 64 and g.half(actor + 1204) == 1408
    assert spent == [-150.] and landings == [(actor, 0)] * 4, (spent, landings)
    for contact in contacts:
        g.run(0x1687A8, contact)
    mark_data = mark_events, repel_packet, pose, repel_effects
    return (shot_packet, events, (spawn, stop, visuals), mark_data), (flight_packet, guided_position, coast_position)


def apply_repel_root(g, actor, clip, start, end):
    """Run GU's root-motion transform using samples from the packaged LMT."""
    assert g.half(actor + 1204) == clip
    bank = assets(PLUGIN.parents[1] / "arc/frontier/magnet_spike.arc")
    lmt = bank[r"player\mot\frontier_ms\4", 0x76820D81]
    record, = struct.unpack_from("<I", lmt, 8 + 4 * (clip % 100))
    table, count = struct.unpack_from("<2I", lmt, record)
    track = next(i for i in range(table, table + 36 * count, 36) if lmt[i + 1] == 4)
    size, samples = struct.unpack_from("<2I", lmt, track + 8)
    bounds, = struct.unpack_from("<I", lmt, track + 32)
    scale = struct.unpack_from("<3f", lmt, bounds)
    bias = struct.unpack_from("<3f", lmt, bounds + 16)
    assert lmt[track] == 4 and size >= (end + 1) * 8

    def sample(owner, output, *_):
        assert owner == actor
        frame, = struct.unpack("<f", struct.pack("<I", g.cpu.reg_read(UC_ARM_REG_S0)))
        point = struct.unpack_from("<3H", lmt, samples + int(frame) * 8)
        world = [unit * width / 65535. + offset for unit, width, offset in zip(point, scale, bias)]
        g.cpu.mem_write(output, struct.pack("<3f", *world))

    g.stub(0x956FC8, sample)
    delta = bytes(g.cpu.mem_read(actor + 28, 4))
    g.cpu.mem_write(actor + 28, struct.pack("<f", end - start))
    g.cpu.mem_write(actor + 96, struct.pack("<3f", 1., 1., 1.))
    g.cpu.mem_write(actor + 1280, struct.pack("<f", end))
    g.cpu.mem_write(actor + 1292, struct.pack("<f", 1.))
    for register in (UC_ARM_REG_S0, UC_ARM_REG_S1):
        g.cpu.reg_write(register, 0x3F800000)
    try:
        g.run(0x53A5BC, actor)
    finally:
        g.cpu.mem_write(actor + 28, delta)
        g.cpu.hook_del(g.traps.pop(0x956FC8))


def verify_weapon_hud(g, actor, equipment, scratch):
    update, draw = 0x544CC8, 0x544DB8
    vtable = 0x176C620  # GU uGUIResourceOwner in the main ELF.
    assert g.get(vtable + 40) == update and g.get(vtable + 48) == draw
    g.stub(g.originals[update], lambda *_: 17)
    g.stub(g.originals[draw], lambda *_: 23)
    layout, widgets, gauge, vital = (g.alloc(n) for n in (688, 8, 76, 76))
    g.put(layout + 608, 1)
    g.put(layout + 620, widgets)
    g.put(widgets, gauge)
    g.put(widgets + 4, vital)
    g.put(vital, 0x1777D58)  # cUIOVitalGauge in HUD0.

    for vtable in (0x1777DD4, 0x1777E5C, 0x1778A68, 0x1778AF0, 0x1778B78,
                   0x1778C20, 0x1778CBC, 0x1778D44, 0x1778DCC):
        assert g.get(vtable + 64) == 0x56E2D8
        g.put(gauge, vtable)
        assert g.run(update, layout) == 17
        assert g.run(draw, layout, 0) == 0

    # A layout shared with a standard status widget keeps drawing.
    g.put(layout + 608, 2)
    assert g.run(update, layout) == 17
    assert g.run(draw, layout, 0) == 23
    g.put(layout + 608, 1)
    g.put(widgets, vital)
    assert g.run(draw, layout, 0) == 23
    g.put(widgets, gauge)

    # The SDK selection is still present, but the hunt equipped a scratch
    # record. It must see the native HUD right away, with no visibility fixes.
    g.put(actor + 5060, scratch)
    assert g.run(update, layout) == 17
    assert g.run(draw, layout, 0) == 23
    g.put(actor + 5060, equipment)
    assert g.run(update, layout) == 17
    assert g.run(draw, layout, 0) == 0


def setup_magnet_hud(g):
    layout, widget_list, vital = (g.alloc(n) for n in (688, 4, 76))
    stock, dynamic, sharpness, root = (g.alloc(n) for n in (80, 36, 228, 228))
    g.put(vital, 0x1777D58)
    g.put(widget_list, vital)
    g.put(layout + 608, 1)
    g.put(layout + 620, widget_list)
    g.put(layout + 248, stock)
    g.put(layout + 660, dynamic)
    g.put(layout + 664, 20)
    g.put(stock + 4 * 5, sharpness)
    g.put(stock + 4 * 10, root)
    g.put(root + 84, 3)
    g.cpu.mem_write(sharpness + 144, struct.pack("<2f", 80., 46.))
    groups = []

    def clone(owner, resource, template, parent):
        assert (owner, resource, template, parent) == (layout, 0xffffffff, 5, 10)
        assert g.get(g.cpu.reg_read(UC_ARM_REG_SP)) == 0x2123290
        node, members, animation = (g.alloc(n) for n in (228, 12, 176))
        bars = [g.alloc(256), g.alloc(256)]
        g.put(node + 212, template)
        g.put(node + 216, members)
        g.put(node + 220, animation)
        g.put(animation + 104, bars[0])
        g.put(bars[0] + 108, bars[1])
        for i, bar in enumerate(bars):
            g.put(bar, 0x178FC48)
            g.put(bar + 112, animation)
            g.put(bar + 160, 2)
            g.put(members + 4 * (i + 1), bar)
        index = len(groups)
        assert index < 8, "meter creates five segments, two boost tracks and one distance group"
        groups.append((node, bars))
        g.put(dynamic + 4 * index, node)
        g.put(layout + 648, len(groups))
        return 20 + index

    g.stub(0x54501C, clone)
    g.stub(0xA344F4, lambda *_: 0)
    g.put(0x212668C, g.alloc(256))  # GUI renderer defaults used by cGUIObjTexture.
    # Run the original cGUIObjTexture constructor with a host allocation slot.
    g.run(0xA56B74)
    allocator, allocator_vt, allocate = (g.alloc(n) for n in (4, 36, 4))
    g.put(0x21234C8 + 24, (g.get(0x21234C8 + 24) & ~(63 << 23)) | (63 << 23))
    g.put(0x18A62E8 + 63 * 4, allocator)
    g.put(allocator, allocator_vt)
    g.put(allocator_vt + 32, allocate)
    g.stub(allocate, lambda _, size, *__: g.alloc(size))
    # Run GU's position, visibility, size and color setters. The replay has
    # no renderer, so stop at the native geometry invalidation callback.
    g.stub(g.get(0x178FC48 + 80), lambda *_: 0)
    g.stub(0xA4DF54, lambda *_: 0)

    def sample(*, boost_tracks=False, flashes=False):
        assert g.run(0x544CC8, layout) == 17
        assert g.get(root + 84) == 3
        fills = []
        tracks = []
        overlays = []
        signal = 0
        for node, bars in groups:
            if g.get(node + 84) & 3 != 3:
                continue
            position = struct.unpack("<2f", g.cpu.mem_read(node + 144, 8))
            assert all(g.get(bar + 160) & 15 == 0 for bar in bars)
            sizes = [struct.unpack("<2f", g.cpu.mem_read(bar + 180, 8)) for bar in bars]
            if position[1] == 60.:
                assert position[0] == 136. + 19 * len(tracks)
                assert math.isclose(sizes[0][0] + sizes[1][0], 16., abs_tol=.0001)
                assert sizes[0][1] == sizes[1][1] == 4.
                tracks.append((sizes[0][0], g.get(bars[0] + 164)))
            elif sizes == [(4., 4.), (4., 4.)]:
                assert position == (136. + 19 * len(fills), 49.)
                signal = sum(g.get(bar + 164) != 0 for bar in bars)
            else:
                assert position == (136. + 19 * len(fills), 49.)
                assert math.isclose(sizes[0][0] + sizes[1][0], 16., abs_tol=.0001)
                assert sizes[0][1] == sizes[1][1] == 8.
                assert struct.unpack("<2f", g.cpu.mem_read(bars[1] + 128, 8)) == (sizes[0][0], 0.)
                sprite = g.get(bars[1] + 108)
                while sprite:
                    assert g.get(sprite) == 0x178FCC8 and g.get(sprite + 116) == layout
                    assert g.get(sprite + 112) == g.get(node + 220)
                    if g.get(sprite + 84) & 1:
                        assert struct.unpack("<2f", g.cpu.mem_read(sprite + 128, 8)) == (-10., -12.)
                        assert struct.unpack("<2f", g.cpu.mem_read(sprite + 180, 8)) == (36., 32.)
                        assert g.get(g.get(sprite + 260) + 12) == (1024 << 16) | 72
                        left, top, right, bottom = struct.unpack("<4f", g.cpu.mem_read(sprite + 268, 16))
                        assert (right - left, bottom - top) == (36., 32.)
                        overlays.append((len(fills), int(top / 32 * 2 + left / 36)))
                    sprite = g.get(sprite + 108)
                fills.append(sizes[0][0])
        if flashes:
            return overlays
        return tracks if boost_tracks else (fills, signal)

    return sample


def verify_magnetic_charge(g, actor, monster, hurt, collision, set_action, tick, meter):
    def charge_attack():
        set_action(1, 0)
        tick(X, held=0x800, native_pad=True)
        return g.half(actor + 9482)

    def number(address, value):
        g.cpu.mem_write(address, struct.pack("<f", value))

    clock = g.get(0x2126764) + 104

    def elapsed(frames):
        number(clock, frames)
        g.run(g.hooks[0x16_ABAC], collision)
        number(clock, 2.)

    def draw(affinity):
        for hit in (actor + 12320, actor + 12504):
            g.run(0x1687A8, hit)
        set_action(1, 0)
        g.cpu.mem_write(g.get(actor + 5060) + 802, struct.pack("<h", affinity))
        g.cpu.mem_write(actor + 10057, b"\0")
        tick(X, native_pad=True)
        assert g.half(actor + 9482) == 0x8000 + 78
        tick(frame=60., ended=True)
        tick(frame=40.)
        return next(actor + at for at in (12320, 12504) if g.half(actor + at + 56) == 43)

    def damage(hit, affinity, roll, critical):
        assert g.get(hit + 48) & 0x20000
        assert g.half(hit + 144) == affinity & 0xffff
        g.cpu.mem_write(hit + 146, struct.pack("<H", roll))
        g.run(0x16C0C0, collision, hit, monster)
        assert g.cpu.mem_read(hit + 84, 1) == bytes([critical])

    # Switch shape in place. Entering the same action from neutral spends only
    # once, and having no native stamina rejects it before spending charge.
    number(actor + 5088, 0.)
    assert charge_attack() == 0
    number(actor + 5088, 1000.)
    # Four accepted attraction states consumed one segment each.
    assert charge_attack() == 0x8000 + 12
    assert meter()[0][0] < 16. and meter()[0][1:] == [0.] * 4
    assert meter(flashes=True) == [(0, 1)]
    tick(frame=20., ended=True)
    elapsed(1.)
    assert meter(flashes=True) == [(0, 1)]  # Two GU frames per source tick.
    elapsed(1.)
    assert charge_attack() == 0
    assert meter(flashes=True) == [(0, 2)]
    elapsed(58.)
    assert meter(flashes=True) == [(0, 31)]
    elapsed(2.)
    assert meter(flashes=True) == []

    # Nine FZ recharge intervals recover 1800 units, just below one segment.
    elapsed(478.)
    assert charge_attack() == 0
    assert 14.4 - 0.001 <= meter()[0][0] < 16.
    assert meter(flashes=True) == []

    # Swinging and a stale serial leave the gauge unchanged. The same strike
    # handled by GU's damage dispatcher can credit these 43 x 6 units.
    hit = draw(0)
    before = meter()
    serial = g.half(hit + 52)
    factor = g.alloc(4)
    number(factor, 1.)
    g.cpu.mem_write(hit + 52, struct.pack("<H", serial ^ 1))
    g.run(0x17885C, 0, hit, monster, hurt, actor, factor)
    assert meter() == before
    g.cpu.mem_write(hit + 52, struct.pack("<H", serial))
    damage(hit, 0, 0, 0)
    assert math.isclose(sum(meter()[0]) - sum(before[0]), 258 * 16 / 2000, abs_tol=.001)
    assert meter(flashes=True) == [(0, 32)]
    assert charge_attack() == 0x8000 + 12
    assert meter(flashes=True) == [(0, 1)]
    assert charge_attack() == 0

    # Affinity is determined by native damage. Truncate the effective motion
    # power before applying the gauge rate and then the active charge skill.
    skills = ()
    g.cpu.hook_del(g.traps.pop(0x2864C4))
    g.stub(0x2864C4, lambda _, skill, *__: int(skill in skills))
    for skills, affinity, roll, critical, gain in (
        ((152,), 100, 0, 2, 381),   # floor(43 * 1.25) * 6 * 120%
        ((153,), -100, 0, 1, 153),  # floor(43 * 0.75) * 6 * 80%
        ((150,), 100, 0, 2, 360),   # GU Critical Boost, multiplier 1.4
        ((), 25, 99, 0, 258),       # No critical hit on this roll
    ):
        hit = draw(affinity)
        before = meter()[0][0]
        damage(hit, affinity, roll, critical)
        assert math.isclose(meter()[0][0] - before, gain * 16 / 2000, abs_tol=.001)
    g.cpu.mem_write(g.get(actor + 5060) + 802, bytes(2))

    # The other form uses a distinct original action and spends the same amount.
    elapsed(600.)
    set_action(1, 0)
    tick(SPECIAL, native_pad=True)
    tick(frame=99., ended=True)
    assert charge_attack() == 0x8000 + 38


def verify_deflection(g, actor, vtable, monster, part, set_action, tick, motions):
    skills = set()
    g.cpu.hook_del(g.traps.pop(0x2864C4))
    g.stub(0x2864C4, lambda _, skill, *__: int(skill in skills))
    g.stub(g.get(vtable + 440), lambda *_: 0)
    g.stub(g.get(vtable + 520), lambda *_: 0)
    g.stub(0xDF814, lambda *_: g.get(actor + 5060))
    g.stub(0xDD4C4, lambda *_: 0)
    factor, element = g.alloc(4), g.alloc(4)
    hit = actor + 12320

    def draw(buttons, state, frame):
        for slot in (hit, actor + 12504):
            g.run(0x1687A8, slot)
        set_action(1, 0)
        g.cpu.mem_write(actor + 10057, b"\0")
        tick(buttons, native_pad=True)
        assert g.half(actor + 9482) == 0x8000 + state
        tick(frame=60., ended=True)
        tick(frame=frame)
        assert g.cpu.mem_read(hit + 5, 1) == b"\x01"

    def contact(ratio, slot=hit):
        g.cpu.mem_write(factor, struct.pack("<f", ratio))
        g.run(g.hooks[0x178030], 0, slot, monster, part, part, actor, factor, element)

    # The previous replay left the impact-form magnetic attack active. Its
    # second strike re-arms deflection while the first strike is still live.
    second = actor + 12504
    for slot in (hit, second):
        g.run(0x1687A8, slot)
    tick(frame=40.)
    contact(.7)
    tick(frame=48.)
    assert g.cpu.mem_read(hit + 5, 1) == g.cpu.mem_read(second + 5, 1) == b"\x01"
    contact(.1, second)
    tick(frame=49.)
    assert g.half(actor + 9482) == 0x8000 + 93 and motions[-1][0] == 1223

    # A soft contact spends the window and a ghost contact from another serial
    # must leave it armed. A fresh hard contact chooses the active form's recoil.
    draw(X, 78, 40.)
    contact(.23)
    contact(.1)
    tick(frame=41.)
    assert g.half(actor + 9482) == 0x8000 + 78

    for buttons, state, frame, recoil, clip in ((X, 78, 40., 92, 1221), (X | A, 79, 30., 93, 1223)):
        draw(buttons, state, frame)
        serial = g.half(hit + 52)
        g.cpu.mem_write(hit + 52, struct.pack("<H", serial ^ 1))
        contact(.1)
        tick(frame=frame + 1.)
        assert g.half(actor + 9482) == 0x8000 + state
        g.cpu.mem_write(hit + 52, struct.pack("<H", serial))
        contact(.22)
        g.cpu.mem_write(actor + 10156, struct.pack("<2f", 3., 4.))
        tick(frame=frame + 2.)
        assert g.half(actor + 9482) == 0x8000 + recoil and motions[-1][0] == clip
        assert struct.unpack("<2f", g.cpu.mem_read(actor + 10156, 8)) == (0., 0.)

    skills.add(87)  # Mind's Eye.
    draw(X, 78, 40.)
    contact(.1)
    tick(frame=41.)
    assert g.half(actor + 9482) == 0x8000 + 78


def verify_magnetic_boost(g, actor, monster, collision, set_action, tick, meter):
    equipment = g.get(actor + 5060)
    g.put(actor + 5060, g.alloc(1376))
    set_action(1, 0)
    g.run(DISPATCH, actor)
    g.put(actor + 5060, equipment)
    tracks = lambda: meter(boost_tracks=True)
    slash, impact, active, cooling = 0xfff26752, 0xff71b6ef, 0xffffc76b, 0xff8d8d8d
    assert tracks() == [(0., slash), (0., impact)]

    effects, live, rotations = [], {}, {}
    part = g.get(actor + 9120)
    joint = g.get(part + 1172) + 16
    initial_pose = bytes(g.cpu.mem_read(joint, 64))

    def xyz(address):
        return struct.unpack("<3f", g.cpu.mem_read(address, 12))

    def emit(_, unit, handle, group):
        assert handle == group == 1
        selector = g.get(g.cpu.reg_read(UC_ARM_REG_SP))
        assert 146 <= selector < 180 and g.get(unit + 320) == 0
        child = g.alloc(96)
        g.put(unit + 336, child)
        effects.append((selector, xyz(unit + 304)))
        live[unit] = child
        return 1

    def rotate(child, radians, *_):
        rotations[child] = xyz(radians)

    def stop(unit, immediate, *_):
        assert immediate == 1
        live.pop(unit)

    for address, callback in ((0x3267D0, emit), (0x8A7DC4, rotate), (0x32A400, stop)):
        if address in g.traps:
            g.cpu.hook_del(g.traps.pop(address))
        g.stub(address, callback)

    def hit(buttons, expected, *, contact=True):
        for slot in (12320, 12504):
            g.run(0x1687A8, actor + slot)
        set_action(1, 0)
        g.cpu.mem_write(actor + 10057, b"\0")
        tick(buttons, native_pad=True)
        tick(frame=60., ended=True)
        tick(frame=40. if buttons == X else 30.)
        slot = actor + 12320
        assert g.half(slot + 56) == expected
        if contact:
            g.run(0x16C0C0, collision, slot, monster)
        return slot

    def boost(state, clip=None, *, staggered=False):
        set_action(1, 0)
        if staggered:
            assert tick(common=1 << 27, held=0x400, native_pad=True)
            assert g.half(actor + 9482) == 0x8000 + 14
        assert tick(SPECIAL, common=1 << 27, held=0x400, native_pad=True,
                    frame=2. if staggered else 0.)
        assert g.half(actor + 9482) == state
        if clip:
            assert g.half(actor + 1204) == clip
            tick(frame=170., ended=True)

    time = 0.

    def elapsed(frames):
        nonlocal time
        time += frames
        clock = g.get(0x2126764) + 104
        g.cpu.mem_write(clock, struct.pack("<f", frames))
        g.run(g.hooks[0x16_ABAC], collision)
        g.cpu.mem_write(clock, struct.pack("<f", 2.))

    hit(X, 43, contact=False)
    assert tracks() == [(0., slash), (0., impact)]
    for _ in range(29):
        hit(X, 43)
    assert 15. < tracks()[0][0] < 16. and tracks()[1] == (0., impact)
    boost(0)
    hit(X, 43)
    assert tracks() == [(16., slash), (0., impact)]
    boost(0x8000 + 163, 1451, staggered=True)
    slash_end = time + 4800.
    assert tracks() == [(16., active), (0., impact)]
    elapsed(35.)
    elapsed(0.)
    assert not effects
    elapsed(1.)
    assert effects == [(i, (-20., 90., 160.)) for i in range(146, 164)]
    flash, first_child = next(iter(live.items()))
    # Yaw and roll the moving weapon together. GU uses the full matrix for
    # the local offset, but receives only pitch and yaw for emission.
    g.cpu.mem_write(joint, struct.pack("<16f", 0, 1, 0, 0, 0, 0, 1, 0, 1, 0, 0, 0,
                                     300., 200., 100., 1.))
    elapsed(4.)
    assert xyz(first_child + 64) == xyz(flash + 304) == (340., 120., 100.)
    pitch, yaw, roll = rotations[first_child]
    assert pitch == roll == 0. and math.isclose(yaw, math.pi / 2, abs_tol=0.0001)
    del live[flash]
    rotations.clear()
    g.run(g.hooks[0x32A104], flash)
    g.cpu.mem_write(joint, initial_pose)
    elapsed(2.)
    assert first_child not in rotations
    assert effects[18:] == ([(i, (105., 90., 140.)) for i in range(168, 174)]
                           + [(i, (-20., 90., 160.)) for i in range(164, 166)])
    g.cpu.mem_write(actor + 10057, b"\0")
    elapsed(20.)
    assert len(effects) == 26
    g.cpu.mem_write(actor + 10057, b"\1")
    elapsed(20.)
    assert effects[26:] == [(i, (15., 90., 140.)) for i in range(168, 174)]
    set_action(1, 0)
    tick(X, held=0x800, native_pad=True)
    assert g.half(actor + 9482) == 0x8000 + 12
    slot = hit(X, 47, contact=False)
    boosted_slash = capture_action(g, actor), capture_position(g, actor)
    before = sum(meter()[0])
    g.cpu.mem_write(slot + 144, struct.pack("<hH", 100, 0))
    g.run(0x16C0C0, collision, slot, monster)
    assert g.cpu.mem_read(slot + 84, 1) == b"\x02"
    # floor(floor(43 * 1.25) * 1.10) * 6 = 348
    assert math.isclose(sum(meter()[0]) - before, 348 * 16 / 2000, abs_tol=.001)

    for _ in range(29):
        hit(X | A, 44)
    assert 15. < tracks()[0][0] < 16. and tracks()[0][1] == active
    assert tracks()[1] == (16., impact)
    elapsed(2.)
    boost(0x8000 + 164, 1452)
    impact_end = time + 4800.
    start = len(effects)
    elapsed(33.)
    assert len(effects) == start
    elapsed(1.)
    assert effects[start:] == [(i, (80., 90., 200.)) for i in range(146, 164)]
    elapsed(8.)
    assert effects[start + 18:] == ([(i, (60., 90., 260.)) for i in range(174, 180)]
                                   + [(i, (80., 90., 200.)) for i in range(166, 168)])
    hit(X | A, 48)
    boosted_impact = capture_action(g, actor), capture_position(g, actor)
    set_action(1, 0)
    idle_boost = capture_action(g, actor), capture_position(g, actor)
    elapsed(slash_end - time)
    assert tracks()[0] == (16., cooling) and tracks()[1][1] == active
    expired_slash = capture_position(g, actor)
    elapsed(impact_end - time)
    expired_impact = capture_position(g, actor)
    assert [color for _, color in tracks()] == [cooling, cooling]
    flags = [struct.unpack_from("<I", packet, 44)[0]
             for packet in (idle_boost[1], expired_slash, expired_impact)]
    assert [flag >> 24 for flag in flags] == [0x70, 0x60, 0x40]
    revisions = [(flag & 15) | (flag >> 10 & 0x30) for flag in flags]
    assert [((b - a) & 63) for a, b in zip(revisions, revisions[1:])] == [1, 1]
    stopped_at = len(effects)
    hit(X, 43)
    boost(0)
    elapsed(slash_end + 6000. - time)
    assert tracks()[0] == (0., slash) and tracks()[1][1] == cooling
    hit(X, 43)
    assert tracks()[0][0] > 0.
    elapsed(impact_end + 6000. - time)
    assert tracks()[1] == (0., impact)
    assert len(effects) == stopped_at
    for unit in list(live):
        g.run(g.hooks[0x32A104], unit)
    g.cpu.hook_del(g.traps.pop(0x8A7DC4))

    # Passive recharge flashes only the cell whose threshold was crossed.
    set_action(1, 0)
    tick(X, held=0x800, native_pad=True)
    assert g.half(actor + 9482) in (0x8000 + 12, 0x8000 + 38)
    assert meter()[0] == [16.] * 4 + [0.]
    elapsed(599.)
    assert meter()[0][-1] < 16. and meter(flashes=True) == []
    elapsed(1.)
    assert meter()[0] == [16.] * 5
    assert meter(flashes=True) == [(4, 33)]  # The refill tick already advanced the animation.
    elapsed(0.)
    assert meter(flashes=True) == [(4, 33)]
    elapsed(38.)
    assert meter(flashes=True) == [(4, 52)]
    elapsed(2.)
    assert meter(flashes=True) == []
    return boosted_slash, boosted_impact, idle_boost, expired_slash, expired_impact


if __name__ == "__main__":
    replay()
    print("Magnet Spike: dispatch, HUD, melee, deflection, guard, magnetic shot, charge, boost, flight and peer replay passed")
