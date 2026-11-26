"""Replay the packaged Magnet Spike through GU's normal hunter dispatcher."""

import json
import math
import struct
import sys

from unicorn.arm_const import UC_ARM_REG_S0, UC_ARM_REG_S1, UC_ARM_REG_SP

from verify_magnet_spike import PLUGIN, REPO
from damage_target import damage_target
from verify_style import (
    CHANGE_ACTION, DECODE, DISPATCH, ENCODE, INIT_HUNTER, MANAGER, SAVE,
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
    vtable = g.alloc(2000)
    # Exercise GU's shared weapon dispatch with the fallback's original virtual table.
    g.cpu.mem_write(vtable, bytes(g.cpu.mem_read(0x174D838, 2000)))
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

    def resource(_, kind, path, flags):
        assert flags == 1
        name = g.string(path)
        paths.append(name)
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

    g.stub(load, resource)
    g.stub(g.originals[INIT_HUNTER], lambda *_: 7)
    assert g.run(g.hooks[INIT_HUNTER], actor) == 7
    assert paths == ([r"arc\frontier\magnet_spike"]
                     + [rf"player\mot\frontier_ms\{g}" for g in (0, 2, 4)]
                     + [rf"player\hit\frontier_ms\melee_{i}" for i in range(1, 9)])
    packed = g.run(ENCODE, actor, PRIVATE | 1440)
    assert packed & 255 == 40 and g.run(DECODE, actor, packed) == PRIVATE | 1440
    for clip in (1014, 1015, 1016, 1017, 1022, 1407, 1408, 1429, 1444, 1460, 1461, 1471, 1472, 1473):
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
    verify_melee(g, actor, vtable, part, shapes, set_action, tick, meter)


def verify_melee(g, actor, vtable, part, shapes, set_action, tick, meter):
    def byte(address):
        return g.cpu.mem_read(address, 1)[0]

    # Keep the native HDP constructor and collision expansion. Supply the
    # matrices GU would have evaluated from the two moving blade joints.
    monster, status, monster_vt, hurt, collision, credit = damage_target(g, vtable)
    dti = g.run(g.get(vtable + 20))
    g.put(0x1898674, g.get(dti + 4))
    g.stub(0x13EFDC4, lambda ptr, size, *_: g.cpu.mem_write(ptr, bytes(size)))
    for address in (0x2AD288, 0x27B918, g.get(vtable + 348), g.get(vtable + 508)):
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
    verify_shot(g, actor, monster, status, monster_vt, hurt, collision, credit, set_action, tick, meter)
    verify_magnetic_charge(g, actor, monster, hurt, collision, set_action, tick, meter)


def verify_shot(g, actor, monster, status, monster_vt, hurt, collision, credit, set_action, tick, meter):
    def put_vec(address, *vec):
        g.cpu.mem_write(address, struct.pack(f"<{len(vec)}f", *vec))

    def position(address):
        return struct.unpack("<3f", g.cpu.mem_read(address, 12))

    def matrix(address, x, y, z):
        put_vec(address, 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., x, y, z, 1.)

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
    roster = g.alloc(80)
    g.put(0x1851FE0, roster)
    g.put(roster + 28, monster)
    for address in (monster + 46585, monster + 30290, status + 416):
        g.cpu.mem_write(address, b"\1")
    put_vec(status + 428, 1., 1.)
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

    hp, credit_count = g.get(status + 880), len(credit)
    held = 0x800
    set_action(1, 0)
    tick(A, held=held, native_pad=True)
    assert g.half(actor + 9482) == 0x8000 + 84
    tick(frame=6.)  # Snapshots the camera direction even if it moves before release.
    put_vec(camera + 96, 1000., 0., 0.)
    tick(frame=11.)
    update()
    assert not rays and not joints
    assert meter()[1] == 0
    tick(frame=12.)
    update()
    assert rays == [((25., 100., 50.), (25., 100., 550.))], rays
    update()
    assert rays[-1] == ((25., 100., 550.), (25., 100., 1050.)), rays
    assert joints.count((monster, 5)) == 4, joints  # Sweep, attach and sample the mark.
    assert meter()[1] == 1
    rays.clear()
    joints.clear()
    matrix(pose, 100., 95., 2150.)
    update()
    assert not rays and joints == [(monster, 5)]
    assert meter()[1] == 2
    matrix(pose, 100., 95., 4150.)
    update()
    assert meter()[1] == 0
    g.put(status + 880, 0)
    joints.clear()
    update()
    g.put(status + 880, hp)
    update()
    assert not joints

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
    update()
    assert len(rays) == 1 and not joints
    assert g.get(status + 880) == hp and len(credit) == credit_count

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

    water = 110.
    tick(frame=60., ended=True)
    tick(A, held=held, native_pad=True)
    tick(frame=14.)
    update()
    assert len(rays) == 1 and not joints
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
    facing = g.half(actor + 4076)
    # The selected point is on the hurt sphere, so it can be off the centerline.
    assert facing == g.half(actor + 5280) and abs(facing - 0x4000) < 0x800, facing
    assert configure[-1] == 0
    assert g.get(actor + 11072) == struct.unpack("<I", struct.pack("<f", 28.))[0]

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
    mark_at(500., 12.)
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
    last = position(actor + 64)
    wall = True
    tick(frame=40.)
    assert position(actor + 64)[2] == last[2] and g.half(actor + 1204) == 1461
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
            g.put(bar + 160, 2)
            g.put(members + 4 * (i + 1), bar)
        index = len(groups)
        assert index < 6, "meter creates five bars and at most one indicator group"
        groups.append((node, bars))
        g.put(dynamic + 4 * index, node)
        g.put(layout + 648, len(groups))
        return 20 + index

    g.stub(0x54501C, clone)
    g.stub(0xA344F4, lambda *_: 0)
    # Run GU's position, visibility, size and color setters. The replay has
    # no renderer, so stop at the native geometry invalidation callback.
    g.stub(g.get(0x178FC48 + 80), lambda *_: 0)
    g.stub(0xA4DF54, lambda *_: 0)

    def sample():
        assert g.run(0x544CC8, layout) == 17
        assert g.get(root + 84) == 3
        fills = []
        signal = 0
        for i, (node, bars) in enumerate(groups):
            if g.get(node + 84) & 3 != 3:
                continue
            assert struct.unpack("<2f", g.cpu.mem_read(node + 144, 8)) == (136. + 19 * i, 49.)
            assert all(g.get(bar + 160) & 15 == 0 for bar in bars)
            sizes = [struct.unpack("<2f", g.cpu.mem_read(bar + 180, 8)) for bar in bars]
            if i == 5:
                assert sizes == [(4., 4.), (4., 4.)]
                signal = sum(g.get(bar + 164) != 0 for bar in bars)
            else:
                assert math.isclose(sizes[0][0] + sizes[1][0], 16., abs_tol=.0001)
                assert sizes[0][1] == sizes[1][1] == 8.
                assert struct.unpack("<2f", g.cpu.mem_read(bars[1] + 128, 8)) == (sizes[0][0], 0.)
                fills.append(sizes[0][0])
        return fills, signal

    return sample


def verify_magnetic_charge(g, actor, monster, hurt, collision, set_action, tick, meter):
    def charge_attack():
        set_action(1, 0)
        tick(X, held=0x800, native_pad=True)
        return g.half(actor + 9482)

    def number(address, value):
        g.cpu.mem_write(address, struct.pack("<f", value))

    # Switch shape in place. Entering the same action from neutral spends only
    # once, and having no native stamina rejects it before spending charge.
    number(actor + 5088, 0.)
    assert charge_attack() == 0
    number(actor + 5088, 1000.)
    # Four accepted attraction states consumed one segment each.
    assert charge_attack() == 0x8000 + 12
    assert meter()[0][0] < 16. and meter()[0][1:] == [0.] * 4
    tick(frame=20., ended=True)
    assert charge_attack() == 0

    # Nine FZ recharge intervals recover 1800 units, just below one segment.
    clock = g.get(0x2126764) + 104
    number(clock, 540.)
    g.run(g.hooks[0x16_ABAC], collision)
    number(clock, 2.)
    assert charge_attack() == 0
    assert 14.4 - 0.001 <= meter()[0][0] < 16.

    # Swinging does not give charge. Only the monster-damage hook for the
    # native serial submitted by the source event can credit these 43 x 6 units.
    g.cpu.mem_write(actor + 10057, b"\0")
    tick(X, native_pad=True)
    assert g.half(actor + 9482) == 0x8000 + 78
    tick(frame=60., ended=True)
    tick(frame=40.)
    hit = next(actor + at for at in (12320, 12504) if g.half(actor + at + 56) == 43)
    serial = g.half(hit + 52)
    args = (0, hit, monster, hurt, hurt, actor, g.alloc(4), g.alloc(4))
    g.cpu.mem_write(hit + 52, struct.pack("<H", serial ^ 1))
    g.run(g.hooks[0x178030], *args)
    assert charge_attack() == 0
    g.cpu.mem_write(hit + 52, struct.pack("<H", serial))
    g.run(g.hooks[0x178030], *args)
    assert charge_attack() == 0x8000 + 12
    assert charge_attack() == 0

    # The other form uses a distinct original action and spends the same amount.
    number(clock, 600.)
    g.run(g.hooks[0x16_ABAC], collision)
    number(clock, 2.)
    tick(SPECIAL, native_pad=True)
    tick(frame=99., ended=True)
    assert charge_attack() == 0x8000 + 38


if __name__ == "__main__":
    replay()
    print("Magnet Spike: dispatch, magnet HUD, melee, magnetic shot, charge, attraction and repulsion passed")
