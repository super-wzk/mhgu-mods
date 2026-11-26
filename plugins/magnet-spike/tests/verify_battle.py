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
        g.run(DISPATCH, actor)

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

    tick(0x1000, native_pad=True)  # GU special attack maps to the FZ mode command.
    assert motions[-1] == (1456, 0., 0.)
    tick(frame=99., ended=True)
    tick(A)
    assert g.half(actor + 9482) == 0x8000 + 32 and motions[-1][0] == 1072
    tick(frame=100., ended=True)
    tick(common=1 << 8)
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
    verify_melee(g, actor, vtable, part, shapes, set_action, tick)


def verify_melee(g, actor, vtable, part, shapes, set_action, tick):
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
    tick(0x1000, native_pad=True)
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
    verify_shot(g, actor, monster, status, monster_vt, hurt, collision, credit, set_action, tick)
    verify_magnetic_charge(g, actor, monster, hurt, collision, set_action, tick)


def verify_shot(g, actor, monster, status, monster_vt, hurt, collision, credit, set_action, tick):
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
    tick(frame=12.)
    update()
    assert rays == [((25., 100., 50.), (25., 100., 550.))], rays
    update()
    assert rays[-1] == ((25., 100., 550.), (25., 100., 1050.)), rays
    assert joints.count((monster, 5)) == 3, joints  # Test per sweep, then attach.
    rays.clear()
    joints.clear()
    matrix(pose, 100., 95., 1150.)
    update()
    assert not rays and joints == [(monster, 5)]
    g.put(status + 880, 0)
    joints.clear()
    update()
    g.put(status + 880, hp)
    update()
    assert not joints

    set_action(1, 0)
    tick(common=1 << 8)
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
    assert len(rays) == 1 and joints == [(monster, 5), (monster, 5)]
    rays.clear()
    joints.clear()
    put_vec(clock, 17998.)
    update()
    assert not rays and joints == [(monster, 5)]
    joints.clear()
    put_vec(clock, 2.)
    update()
    assert not rays and not joints

    water = 110.
    tick(frame=60., ended=True)
    tick(A, held=held, native_pad=True)
    tick(frame=14.)
    update()
    assert len(rays) == 1 and not joints
    update()
    assert len(rays) == 1 and not joints


def verify_magnetic_charge(g, actor, monster, hurt, collision, set_action, tick):
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
    for _ in range(5):
        assert charge_attack() == 0x8000 + 12
        tick(frame=20., ended=True)
    assert charge_attack() == 0

    # Nine FZ recharge intervals recover 1800 units, just below one segment.
    clock = g.get(0x2126764) + 104
    number(clock, 540.)
    g.run(g.hooks[0x16_ABAC], collision)
    number(clock, 2.)
    assert charge_attack() == 0

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
    tick(common=1 << 8)
    tick(frame=99., ended=True)
    assert charge_attack() == 0x8000 + 38


if __name__ == "__main__":
    replay()
    print("Magnet Spike: dispatch, native melee, magnetic shot and charge passed")
