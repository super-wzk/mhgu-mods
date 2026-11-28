"""Replay the packaged Magnet Spike through GU's normal hunter dispatcher."""

import itertools
import json
import math
import struct
import sys

from unicorn import UC_HOOK_CODE
from unicorn.arm_const import UC_ARM_REG_R1, UC_ARM_REG_S0, UC_ARM_REG_S1, UC_ARM_REG_SP

from verify_magnet_spike import PLUGIN, REPO, ROOT
from verify_media import verify_audio, verify_gauge, verify_effect_tracks
from verify_peer import capture_action, capture_position, shock_contacts, verify_peer
from damage_target import damage_target
from verify_swing_frames import loop_frames
from verify_ranges import vita_contact_clock, verify_contact_clock, verify_ranges, verify_marked_row
from verify_style import (
    BLOCK, CHANGE_ACTION, COLLISION_PASS, DECODE, DISPATCH, ENCODE, INIT_HUNTER, MANAGER,
    QUEST_CAMERA, QUEST_CAMERA_ORBIT, QUEST_CAMERA_TERRAIN, QUEST_CAMERA_UPDATE, SAVE, SETTLE_GUARD,
    WEAPON_DISPATCH, api_for, boot, character_save, quest_arc, select_character, setup_controller,
)

sys.path.insert(0, str(REPO / "tools/tests"))
from verify_packed_release import assets

PRIVATE = 0x10000
WEAPON = 16
Y, X, R, B, A, ZL, ZR = 0x10, 0x20, 0x40, 0x80, 0x100, 0x200, 0x800
SPECIAL = 0x1000


def particle_allocator(g):
    # GU allocates each submitted scene unit independently. The full weapon
    # replay keeps snapshots of them across simulated area changes.
    start, size = 0xB000000, 0x1000000
    g.cpu.mem_map(start, size)
    cursor = start
    def alloc(n, *_):
        nonlocal cursor
        address = cursor
        cursor += (n + 15) & ~15
        assert cursor <= start + size
        return address
    return alloc


def replay(*, inputs_only=False):
    saved = json.loads(character_save({}))
    saved["data"]["inventory"] = {
        "instances": {"1": {"equipment": 0x1001, "upgrade": 0}}, "equipped": 1, "sets": {},
    }
    g = boot((REPO / "mhgu/main.elf").read_bytes(), json.dumps(saved).encode(), ("registry",))
    bind_meter = g.get(api_for(g, b"mhgu::WeaponMeter", b"WMTR"))
    samplers = []
    g.cpu.hook_add(UC_HOOK_CODE, lambda cpu, *_: samplers.append(cpu.reg_read(UC_ARM_REG_R1)),
                   begin=bind_meter, end=bind_meter)
    bind_monster = g.get(api_for(g, b"mhgu::IndependentMelee", b"IMEL") + 12)
    def capture_observer(cpu, *_):
        g.restrained_query = g.get(cpu.reg_read(UC_ARM_REG_R1))
    observer_hook = g.cpu.hook_add(UC_HOOK_CODE, capture_observer, begin=bind_monster, end=bind_monster)
    exports = g.load(PLUGIN, 0x9200000)
    for callback in ("forge_onLoad", "forge_onInit"):
        g.run(exports[callback], MANAGER)
    g.cpu.hook_del(observer_hook)
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
    meter = setup_magnet_hud(g, samplers[0], actor)
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
    for address in (0x27D5C4, 0x3A5ED0, 0x5584A0, 0x27FE18, 0x27DB78):
        g.stub(address, lambda *_: 0)

    def tick(buttons=0, *, held=0, common=0, frame=0., next_frame=None, ended=False, fallback=0, native_pad=False):
        g.put(actor + 5112, buttons)
        g.put(actor + 5116, held | buttons)
        g.put(actor + 5124, buttons)
        g.put(actor + 5140, common)
        g.put(actor + 5156, fallback)
        g.cpu.mem_write(actor + 1280, struct.pack("<f", frame))
        saved_next = bytes(g.cpu.mem_read(actor + 1276, 4))
        if next_frame is not None:
            g.cpu.mem_write(actor + 1276, struct.pack("<f", next_frame))
        g.cpu.mem_write(actor + 1206, bytes([4 * ended]))
        if native_pad:
            g.cpu.mem_write(actor + 5108, struct.pack("<3H", held | buttons, 0, buttons))
            g.put(actor + 5188, 0)
            g.run(0x27CA28, actor)
        before = len(retail)
        g.run(DISPATCH, actor)
        if next_frame is not None:
            g.cpu.mem_write(actor + 1276, saved_next)
        return len(retail) == before

    if inputs_only:
        from verify_inputs import verify_inputs
        verify_inputs(g, actor, equipment, scratch, set_action, changes)
        return

    set_action(1, 0)
    g.run(CHANGE_ACTION, actor, WEAPON, 3, 0x80)
    assert not changes
    tick(fallback=1 << 14)
    assert retail == ["ground"] and not motions

    tick(X)
    assert changes[-1][:2] == (1, 1) and not motions
    set_action(2, 0)  # Moving draw keeps FZ state 78 and its slash follow-up.
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
    assert motions[-1] == (1049, 10., 6.)
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
    panel_name = r"GUI\frontier_magnet_spike\panel"
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
            assert flags == 2 and name in (flash_name, panel_name)
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
                     + [r"player\hit\frontier_ms\bind_finisher", r"player\hit\frontier_ms\shock"]
                     + [pel_name, flash_name, panel_name]
                     + [rf"sound\se\pl\frontier_ms\{name}" for name in ("blade", "common", "weapon", "voice_m", "voice_f")])
    contact = []
    def native_contact(_engine, _actor, _request, _joint):
        sp = g.cpu.reg_read(UC_ARM_REG_SP)
        contact.append((g.get(sp + 4), g.get(sp + 8)))
        return 0
    g.stub(0x4F0BE8, native_contact)
    verify_audio(g, actor, sounds, contact)
    verify_gauge(resources[panel_name, 0x241F5DEB])
    set_action(1, 0)
    g.put(actor + 10057, 1)
    tick(held=ZL, native_pad=True)
    assert any(frame == 507 for _, frame, _ in meter(artwork=True))
    tick(native_pad=True)
    assert all(frame != 507 for _, frame, _ in meter(artwork=True))
    g.cpu.mem_write(actor + 5088, struct.pack("<2f", 1000., 1000.))
    tick(R, native_pad=True)
    assert g.get(actor + 5140) & 0x80000001 == 0
    tick(X, held=R, native_pad=True)
    assert g.half(actor + 9482) in (0x8000 + 11, 0x8000 + 39)
    packed = g.run(ENCODE, actor, PRIVATE | 1440)
    assert packed & 255 == 40 and g.run(DECODE, actor, packed) == PRIVATE | 1440
    normal_draw = g.run(ENCODE, actor, 5002)
    assert normal_draw & 255 == 2 and g.run(DECODE, actor, normal_draw) == 5002
    for clip in (1014, 1015, 1016, 1017, 1022, 1407, 1408, 1429, 1444, 1451, 1452, 1460, 1461, 1471, 1472, 1473):
        encoded = g.run(ENCODE, actor, PRIVATE | clip)
        assert g.run(DECODE, actor, encoded) == PRIVATE | clip, clip

    part = g.alloc(5152)
    g.put(part + 48, actor)
    g.put(part + 240, g.alloc(128))
    g.put(actor + 9120, part)
    g.put(part + 1204, 0xffff)
    part_vtable, attach = g.alloc(80), g.alloc(4)
    g.put(part, part_vtable)
    g.put(part_vtable + 72, attach)
    parents = []
    g.stub(attach, lambda part, owner, joint, _: parents.append((owner, joint)))
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
    assert tracks == [(0, 40, 0., 12.5), (1, 255, 0., 0.)], tracks
    assert g.half(part + 1524) == 0xffff and g.get(part + 1212) & 16
    g.cpu.mem_write(actor + 1280, struct.pack("<f", 14.))
    g.run(update_part, part, actor, WEAPON)
    assert len(tracks) == 2  # Sampling the same clip does not restart it.
    assert struct.unpack("<f", g.cpu.mem_read(part + 1276, 4)) == (14.,)
    assert paths.count(part_bank) == 1

    packed = g.run(ENCODE, actor, PRIVATE | 1435)
    g.put(actor + 1204, packed)
    saved_loop = bytes(g.cpu.mem_read(actor + 1284, 12))
    # Last authored poses 88 and 90 are held for a full FZ tick.
    g.cpu.mem_write(actor + 1284, struct.pack("<3f", 90., 52., 1.))
    for body_frame, weapon_frame in ((86., 86.), (88., 88.), (52., 90.), (52., 90.),
                                     (54., 52.), (56., 54.), (56.5, 54.5)):
        g.cpu.mem_write(actor + 1280, struct.pack("<f", body_frame))
        g.run(update_part, part, actor, WEAPON)
        assert struct.unpack("<f", g.cpu.mem_read(part + 1276, 4)) == (weapon_frame,)
    assert tracks[-1] == (0, 35, 2., 86.), tracks
    g.cpu.mem_write(actor + 1284, saved_loop)
    for clip, frame, blend, joint in [(1440, 23, 0, 2), (1440, 24, 0, 8),
                                      (1043, 14, 0, 12), (1043, 14, 2, 8), (1043, 142, 0, 8)]:
        g.put(actor + 1204, g.run(ENCODE, actor, PRIVATE | clip))
        g.cpu.mem_write(actor + 1280, struct.pack("<f", frame))
        g.cpu.mem_write(actor + 1268, struct.pack("<f", blend))
        g.run(update_part, part, actor, WEAPON)
        assert g.get(part + 5068) == joint and parents[-1] == (actor, joint)
    g.cpu.mem_write(actor + 1268, struct.pack("<f", 0.))
    for public in (5001, 5003, 5004, 5009, 5012, 5013):
        assert g.run(DECODE, actor, g.run(ENCODE, actor, public)) == public
    # GU clears the drawn bit as it enters sheath even in impact form.
    saved_sex = bytes(g.cpu.mem_read(equipment + 1240, 1))
    for sex in (0, 1):
        g.cpu.mem_write(equipment + 1240, bytes([sex]))
        for mode_state, standing, walking, running, cuts in (
            (0, 1003, 1008, 1462, (74, 19, 30)), (32, 1005, 1009, 1463, (84, 40, 38))
        ):
            set_action(WEAPON, 0x8000 + mode_state)
            tick()
            for (slot, request, prev, clip), cutoff in zip(
                ((2, 5003, 0, standing), (3, 5009, 0, walking),
                 (2, 5003, 44 if mode_state else 40, running)), cuts
            ):
                set_action(1, slot)
                g.cpu.mem_write(actor + 9480, struct.pack("<H", WEAPON))
                g.cpu.mem_write(actor + 9484, struct.pack("<H", 0x8000 + prev))
                g.cpu.mem_write(actor + 10057, bytes(1))
                tick()
                packed = g.run(ENCODE, actor, request)
                assert packed == g.run(ENCODE, actor, PRIVATE | clip), (sex, mode_state, request, clip)
                g.put(actor + 1204, packed)
                for frame, expected in ((cutoff-1, 8), (cutoff, 2)):
                    g.cpu.mem_write(actor + 1280, struct.pack("<f", frame))
                    g.run(update_part, part, actor, WEAPON)
                    assert g.get(part + 5068) == expected, (sex, clip, frame)
    g.cpu.mem_write(equipment + 1240, saved_sex)
    verify_whetstone(g, actor, part, set_action, tick, update_part)
    loop_frames(g, actor, part, ENCODE, update_part)
    verify_melee(g, actor, vtable, part, shapes, set_action, tick, meter, motions, sounds)
    verify_effect_tracks(g, sounds)


def verify_whetstone(g, actor, part, set_action, tick, update_part):
    # GU 2D6634 requests 5255. Its retail LMT places equipment events
    # 11/12/30 between frame 20 and frame 324, including repeated strokes.
    set_action(1, 0)
    tick()
    g.cpu.hook_del(g.traps.pop(0x2A2444))  # Read the real sheathed flag.
    whetstone = g.run(ENCODE, actor, 5255)
    assert g.run(DECODE, actor, whetstone) == 5255
    g.put(actor + 1204, whetstone)
    # Read the socket event track from the retail weapon motion bank.
    lmt = assets(quest_arc("w07.arc"))[r"player\mot\w07\w07", 0x76820D81]
    clip, = struct.unpack_from("<I", lmt, 8 + 255 * 4)
    group, = struct.unpack_from("<I", lmt, clip + 52)
    kinds = struct.unpack_from("<32H", lmt, group)
    count, runs = struct.unpack_from("<2I", lmt, group + 64)
    timeline = []
    for mask, length in struct.iter_unpack("<2I", lmt[runs:runs + count * 8]):
        events = tuple(kind for i, kind in enumerate(kinds) if mask & (1 << i))
        timeline.extend([events] * length)
    for frame, joint, slot in ((19, 2, 0), (20, 8, 1), (220, 8, 1), (100, 8, 1),
                              (323.5, 8, 1), (324, 2, 0), (20, 8, 1)):
        events = timeline[int(frame)]
        g.put(actor + 5340, (1 << len(events)) - 1)
        for index, event in enumerate(events):
            g.put(actor + 5356 + 4 * index, event)
        g.cpu.mem_write(actor + 1280, struct.pack("<f", frame))
        g.run(update_part, part, actor, WEAPON)
        assert (g.get(part + 5068), g.half(part + 1204) & 255) == (joint, slot), frame
        assert g.cpu.mem_read(actor + 10057, 1) == b"\0"
    # An interrupt can change the hunter motion before the event list is cleared.
    g.put(actor + 1204, g.run(ENCODE, actor, 1))
    g.run(update_part, part, actor, WEAPON)
    actual = g.get(part + 5068), g.half(part + 1204) & 255
    assert actual == (2, 0), actual
    g.stub(0x2A2444, lambda *_: 1)
    g.put(actor + 5340, 0)


def verify_magnetic_steps(g, actor, set_action, tick, meter, sounds, confirm_world_refill):
    g.stub(0x65F93C, lambda *_: R)
    yaw = None
    for address, callback in (
        (0x27E608, lambda *_: int(yaw is not None)),
        (0x27F5D0, lambda *_: yaw & 0xffff),
    ):
        g.cpu.hook_del(g.traps.pop(address))
        g.stub(address, callback)

    distance_skill = 0
    g.cpu.hook_del(g.traps.pop(0x2864C4))
    g.stub(0x2864C4, lambda _, candidate, *__: int(candidate == distance_skill))

    def reset():
        g.run(g.hooks[INIT_HUNTER], actor)
        set_action(1, 0)
        g.put(actor + 10057, 0)
        tick()
        g.put(actor + 10057, 1)
        g.put(actor + 4076, 0)
        g.put(actor + 5280, 0)
        g.put(actor + 11072, 0)
        g.cpu.mem_write(actor + 5088, struct.pack("<2f", 1000., 1000.))

    def expect(state, clip):
        actual = g.half(actor + 9482), g.half(actor + 1204)
        assert actual == (0x8000 + state, clip), (actual, state, clip, at(5088))

    def press(button=Y, frame=0.):
        assert tick(button, held=R, frame=frame, fallback=1 << 14, native_pad=True)

    def at(offset):
        return struct.unpack("<f", g.cpu.mem_read(actor + offset, 4))[0]

    def put_float(offset, value):
        g.cpu.mem_write(actor + offset, struct.pack("<f", value))

    saved_position = bytes(g.cpu.mem_read(actor + 64, 12))
    saved_delta = at(28)
    put_float(28, 2.)
    # Enter through the GU controller, including the two stick pushes and the
    # actual impact A chain. Drive the FZ charge counter independently of LMT time.
    for impact in (False, True):
        reset()
        if impact:
            tick(SPECIAL, native_pad=True)
            tick(frame=99., ended=True)
        g.cpu.mem_write(actor + 5136, bytes(2))
        for _ in range(26):
            tick()
        for strength in (0, 100, 0, 100):
            g.cpu.mem_write(actor + 5136, struct.pack("<h", strength))
            tick()
        expect(44 if impact else 40, 1435 if impact else 1430)
        g.put(actor + 10288, 0)
        before = at(72)
        apply_root_motion(g, actor, 1435 if impact else 1430, 2, 18)
        assert at(72) > before
        g.cpu.mem_write(actor + 5136, bytes(2))
        for frame in range(2, 40, 2):
            tick(frame=frame)
        expect(44 if impact else 40, 1435 if impact else 1430)
        assert at(10288) == 8.
        tick(frame=40.)
        expect(46 if impact else 42, 1436 if impact else 1431)
        set_action(1, 0)
        g.cpu.mem_write(actor + 5136, struct.pack("<h", 100))
        tick(A)
        expect(23 if impact else 6, 1413 if impact else 1048)
        g.cpu.mem_write(actor + 5136, bytes(2))
    for charge_ticks, successor in ((0, 36), (19, 36), (20, 35), (35, 36)):
        reset()
        tick(SPECIAL, native_pad=True)
        tick(frame=99., ended=True)
        tick(A)
        tick(A, frame=52.)
        tick(A, frame=70.)
        expect(34, 1080)
        tick(held=A, frame=50.)
        for clock in range(charge_ticks):
            tick(held=A, frame=52. + clock)
        if charge_ticks == 35:
            expect(36, 1082)  # The overcharge must release even while held.
        else:
            # Releasing A is independent of a held aim or step modifier.
            tick(held={0: 0, 19: R, 20: ZL}[charge_ticks], frame=90.)
            expect(successor, 1082)
    reset()
    for impact, steps in (
        (False, ((107, 1493), (108, 1494), (106, 1464))),
        (True, ((116, 1480), (117, 1481), (158, 1447))),
    ):
        reset()
        if impact:
            tick(SPECIAL, native_pad=True)
            tick(frame=99., ended=True)
        press(X)
        expect(39 if impact else 11, 1060 if impact else 1045)
        assert meter()[0] == [16.] * 4 + [0.]
        if not impact:
            # State 11 adds signed displacement on top of the packaged root.
            g.put(actor + 4076, 0x4000)
            g.cpu.mem_write(actor + 64, struct.pack("<3f", 0., 3., 0.))
            velocity = struct.pack("<3f", 1., 2., 3.)
            g.cpu.mem_write(actor + 4112, velocity)
            tick(frame=17.)
            assert at(64) == 0.
            tick(frame=18.)
            assert at(64) == 20.
            apply_root_motion(g, actor, 1045, 18, 20)
            root_end = at(64)
            assert root_end != 20.
            tick(frame=20.)
            assert math.isclose(at(64) - root_end, 20. * .97, abs_tol=0.001)
            end = at(64)
            put_float(10156, 1.)
            tick(frame=20.)
            assert at(64) == end
            put_float(10156, 0.)
            put_float(28, 1.)
            tick(frame=21.)
            tick(frame=22.)
            assert math.isclose(at(64) - end, 20. * .97**2, abs_tol=0.001)
            put_float(28, 2.)
            end = at(64)
            tick(frame=58.)
            assert math.isclose(at(64) - end, -5., abs_tol=0.001)
            end = at(64)
            tick(frame=68.)
            assert at(64) == end and at(68) == 3.
            assert g.cpu.mem_read(actor + 4112, 12) == velocity
            g.cpu.mem_write(actor + 64, saved_position)
        reset()
        if impact:
            tick(SPECIAL, native_pad=True)
            tick(frame=99., ended=True)
        for button, (state, clip), segments in zip((Y, A, B), steps, (4, 3, 2)):
            set_action(1, 0)
            g.put(actor + 11072, 0)
            yaw = 0xc000  # Stick direction does not select the magnetic step.
            press(button)
            expect(state, clip)
            assert meter()[0] == [16.] * segments + [0.] * (5 - segments)
            assert at(11072) == 14.
            tick(frame=1.)
            assert meter()[0] == [16.] * segments + [0.] * (5 - segments)
            position = bytes(g.cpu.mem_read(actor + 64, 12))
            for distance_skill in (0, 166):
                tick(frame=2., next_frame=4.)
                delta = 240. / 34. if distance_skill else 0.
                expected = {Y: (delta, 0.), A: (-delta, 0.), B: (0., -delta)}[button]
                origin_x, _, origin_z = struct.unpack("<3f", position)
                assert math.isclose(at(64) - origin_x, expected[0], abs_tol=0.001)
                assert math.isclose(at(72) - origin_z, expected[1], abs_tol=0.001)
                g.cpu.mem_write(actor + 64, position)
            distance_skill = 0
            before = struct.unpack("<3f", position)
            apply_root_motion(g, actor, clip, 0, 10)
            dx, _, dz = (at(actor_offset) - value for actor_offset, value in zip((64, 68, 72), before))
            if button == B:
                assert dz < -abs(dx)
            else:
                # GU 27F5D0 maps stick-right to yaw -0x4000 at camera yaw 0.
                assert (-dx if button == A else dx) > abs(dz)
            g.cpu.mem_write(actor + 64, position)
            if impact and state == 158:
                tick(X, frame=10.)
                expect(158, 1447)
                tick(frame=44.)
                expect(161, 1449)
                assert math.isclose(at(5296), 1.1, abs_tol=1e-6)
                assert g.half(actor + 4076) == (-10920 & 0xffff)
                forward_z = 60. * math.cos(10920 * math.tau / 65536.)
                start = at(72)
                tick(frame=9.)
                assert at(72) == start
                tick(frame=10.)
                assert math.isclose(at(72) - start, forward_z, abs_tol=0.002)
                start = at(72)
                tick(frame=55., ended=True)
                expect(161, 1082)
                assert math.isclose(at(5296), 1.1, abs_tol=1e-6)
                assert math.isclose(at(72) - start, forward_z, abs_tol=0.002)
                start = at(72)
                tick(frame=68.)
                assert at(72) == start
                tick(frame=84.)
                assert at(5296) == 1.

    # Cancel a custom attack through native 1:6 and ask GU to encode the
    # public roll, then buffer a roll attack before frame 40.
    for impact in (False, True):
        for angle, clip in ((0x4000, 1007), (0xc000, 1006), (0x7000, 1006), (0x9000, 1007), (0, 28)):
            reset()
            if impact:
                tick(SPECIAL, native_pad=True)
                tick(frame=99., ended=True)
            yaw = -angle & 0xffff
            tick(X)
            if impact:
                tick(frame=100., ended=True)
            tick(B, frame=62., native_pad=True)
            assert (g.half(actor + 9478), g.half(actor + 9482)) == (1, 6), (impact, angle, g.half(actor + 9482))
            if clip != 28 or impact:
                wanted = (1021 if clip == 28 else clip + 4) if impact else clip
                assert g.run(ENCODE, actor, 28) == g.run(ENCODE, actor, PRIVATE | wanted)
            tick(X, frame=20.)
            assert g.half(actor + 9482) == 6
            tick(frame=40.)
            assert g.half(actor + 9482) == 0x8000 + (143 if impact else 140)
    yaw = None
    fallbacks = []
    g.stub(g.originals[DISPATCH], lambda *_: fallbacks.append(g.half(actor + 9482)))
    for impact in (False, True):
        for slot in (6, 7):
            reset()
            if impact:
                tick(SPECIAL, native_pad=True)
                tick(frame=99., ended=True)
            set_action(64, slot)
            g.cpu.mem_write(actor + 5, bytes([2]))
            g.cpu.mem_write(actor + 11988, bytes([1]))
            g.cpu.mem_write(actor + 1204, struct.pack("<H", 0x10f))
            put_float(5088, 149.)
            tick(common=1 << 3)
            assert g.half(actor + 9478) == 64 and fallbacks[-1] == slot
            put_float(5088, 1000.)
            tick(common=1 << 3)
            state = (104 if impact else 102) - (slot - 6)
            expect(state, 1495 + state - 101)
    g.cpu.hook_del(g.traps.pop(g.originals[DISPATCH]))
    g.cpu.mem_write(actor + 11988, bytes(1))

    # Idle uses the same FZ ordering as the action table. B wins over X,
    # and Y wins over A and the extra-shoulder boost command.
    for buttons, state, clip in ((B | X, 106, 1464), (Y | A | SPECIAL, 107, 1493)):
        reset()
        press(buttons)
        expect(state, clip)
        assert meter()[0] == [16.] * 4 + [0.]

    reset()
    yaw = 0x4000
    tick(X)
    expect(0, 1049)
    press(frame=51.)
    expect(0, 1049)
    press(Y | X, frame=52.)
    expect(107, 1493)
    tick(X, frame=20.)
    expect(107, 1493)
    tick(frame=32.)
    expect(110, 1036)
    start = at(72)
    tick(frame=60., ended=True)
    expect(110, 1040)
    assert g.half(actor + 4076) == 5460
    assert math.isclose(at(72) - start, 30. * math.cos(5460 * math.tau / 65536.), abs_tol=0.002)
    start = at(72)
    tick(frame=18.)
    assert at(72) == start

    yaw = 0
    set_action(1, 0)
    tick(X)
    tick(B, frame=62., native_pad=True)
    assert (g.half(actor + 9478), g.half(actor + 9482)) == (1, 6)
    yaw = 0x4000

    # The magnetic chord still owns B after the original weapon mapper
    # produces a command, even when its own resource check fails.
    set_action(1, 0)
    g.put(actor + 5088, 0)
    press(B)
    assert g.half(actor + 9482) == 0
    g.cpu.mem_write(actor + 5088, struct.pack("<f", 1000.))
    for _ in range(4):
        set_action(1, 0)
        press()
        expect(107, 1493)
    assert meter()[0] == [0.] * 5
    set_action(1, 0)
    press(B)
    assert g.half(actor + 9482) == 0
    yaw = None  # Isolate shock geometry from the preceding directional-cancel replay.
    evade_events = verify_evade_contact(
        g, actor, reset, press, expect, set_action, tick, sounds, confirm_world_refill,
    )
    reset()
    for address in (0x27E608, 0x27F5D0):
        g.cpu.hook_del(g.traps.pop(address))
        g.stub(address, lambda *_: 0)
    g.cpu.hook_del(g.traps.pop(0x65F93C))
    g.cpu.mem_write(actor + 64, saved_position)
    put_float(28, saved_delta)
    return evade_events


def verify_evade_contact(g, actor, reset, press, expect, set_action, tick, sounds, confirm_world_refill):
    manager, hit, resource, table, group, row, lookup, volume = (
        g.alloc(n) for n in (10316, 184, 4, 72, 12, 4, 4, 36)
    )
    g.put(manager + 8224, hit)
    g.put(hit + 132, resource)
    g.put(resource, table)
    g.put(table + 68, lookup)
    g.put(group + 4, 1)
    g.put(group + 8, row)
    g.put(row, row)
    g.stub(lookup, lambda *_: group)
    g.stub(g.originals[COLLISION_PASS], lambda *_: 3)
    clock = g.get(0x2126764) + 104
    saved_clock = g.get(clock)
    g.put(clock, 0x40000000)  # Two GU scene frames per pass.
    collision = g.get(0x1857830)

    # Capture the actual position packets sent by the confirmation callback.
    previous_network = g.get(0x18843F0)
    network, room, process = (g.alloc(n) for n in (1604, 240, 48))
    g.put(0x18843F0, network)
    g.put(network + 520, room)
    g.put(room + 232, 2)
    g.put(network + 992 + 4 * 6, process)
    g.put(process + 16, 1)
    g.put(process + 36, 1)
    transport = g.originals[0x9FC3D8]
    events = []

    def send(sender, target, packet, priority):
        assert (sender, target, priority) == (process, 1, 4)
        assert g.get(packet) == 0x17423E4 and g.get(packet + 44) >> 28 == 0xE
        events.append(bytes(g.cpu.mem_read(packet, 48)))

    g.stub(0x49694, lambda *_: 1)
    g.stub(transport, send)

    part = g.get(actor + 9120)
    mapping, matrix, handle, camera = (g.alloc(n) for n in (256, 160, 16, 2144))
    g.put(part + 1172, matrix)
    g.put(part + 1176, mapping)
    g.cpu.mem_write(mapping, bytes([255] * 4 + [0] + [255] * 251))
    matrix += 16
    rotation = (1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0.)
    g.cpu.mem_write(matrix, struct.pack("<16f", *rotation, 50., 10., 100., 1.))
    g.put(handle + 4, 1)
    g.put(0x188A0D4, g.alloc(4))
    g.put(0x2126F48, g.alloc(4))
    units = {}
    shock_fx = []

    def xyz(address):
        return struct.unpack("<3f", g.cpu.mem_read(address, 12))

    def emit(_, unit, handle_id, group_id):
        assert handle_id == group_id == 1 and g.get(unit + 320) == 0
        layer = g.get(g.cpu.reg_read(UC_ARM_REG_SP))
        if layer >= 205:
            shock_fx.append((layer, xyz(unit + 304), xyz(unit + 160)))
        else:
            units[unit] = layer, xyz(unit + 304)
        return 1

    def stop(unit, immediate, *_):
        assert immediate == 1
        del units[unit]

    def expect_flashes(groups, yaw=0.):
        expected = [(layer, point) for layers, point in groups for layer in layers]
        assert len(shock_fx) == len(expected), (shock_fx, expected)
        for (layer, point, angles), (expected_layer, expected_point) in zip(shock_fx, expected):
            assert layer == expected_layer and angles == (0., yaw, 0.)
            assert math.dist(point, expected_point) < 0.001
        shock_fx.clear()

    particle_stubs = (
        (0x3268D8, lambda *_: handle), (0x326C78, lambda *_: 0),
        (0x3288C4, lambda size, *_: g.alloc(size)), (0x328900, lambda *_: 0),
        (0x3267D0, emit), (0x32A400, stop),
    )
    for address, callback in particle_stubs:
        g.stub(address, callback)

    shake_samples = []
    g.put(camera, 0x1730D0C)
    g.put(camera + 2140, actor)
    camera_original = g.originals[QUEST_CAMERA]
    g.stub(camera_original, lambda *_: shake_samples.append(xyz(camera + 64)[1]))

    def step(button, state, clip, impact=True):
        reset()
        if impact:
            tick(SPECIAL, native_pad=True)
            tick(frame=99., ended=True)
        g.cpu.mem_write(actor + 64, bytes(12))
        press(button)
        expect(state, clip)

    def contact(kind, distance, *, owner=1, root_owner=0, area=0, delayed=False, target=0):
        g.cpu.mem_write(hit + 5, bytes([1, 0, area, 1]))
        g.cpu.mem_write(hit + 16, bytes([owner]))
        g.cpu.mem_write(hit + 110, bytes([root_owner]))
        g.cpu.mem_write(hit + 50, bytes([0x80 if target else 0]))
        g.put(hit + 140, target)
        g.cpu.mem_write(hit + 92, struct.pack("<f", float(delayed)))
        g.put(manager + 10288, 0)
        g.cpu.mem_write(volume, struct.pack("<9f", distance, -100., 0., 0.,
                                           distance, 100., 0., 0., 32.))
        g.run(0x1706A8, manager, kind, volume)
        record = bytes(g.cpu.mem_read(hit, 184))
        shapes = bytes(g.cpu.mem_read(manager + 8240, 32))
        assert g.run(g.hooks[COLLISION_PASS], manager) == 3
        assert g.cpu.mem_read(hit, 184) == record
        assert g.cpu.mem_read(manager + 8240, 32) == shapes

    step(A, 117, 1481)
    contact(1, 900.)
    contact(1, 200., owner=0)
    contact(1, 200., area=1)
    contact(1, 200., delayed=True)
    contact(1, 200., target=actor + 4)
    tick(X, frame=10.)
    tick(frame=32.)
    expect(95, 1032)
    assert not units and not events
    tick(frame=7.99)
    assert not shock_contacts(g, collision) and not shock_fx
    tick(frame=8.)
    tick(frame=8.)
    set_action(1, 0)
    first_sound = len(sounds)
    ordinary_flashes = {
        0: ((range(252, 254), (-150., 20., 0.)),),
        2: ((range(252, 254), (-178., 20., 70.)), (range(270, 284), (-28., 160., 250.))),
        4: ((range(252, 254), (-166., 20., 140.)),),
        6: ((range(254, 260), (-104., 20., 270.)),),
        10: ((range(260, 270), (-10., 20., 450.)),),
    }
    for age in range(0, 22, 2):
        g.cpu.mem_write(actor + 64, struct.pack("<3f", age, 20., 0.))
        assert not shock_contacts(g, collision)
        expect_flashes(ordinary_flashes.get(age, ()))
    assert sounds[first_sound:] == [(actor, "blade", cue, (-150., 20., 0.)) for cue in (23, 5)]
    # Expire the ordinary impact's shake before sampling the enhanced one.
    g.cpu.mem_write(camera + 28, struct.pack("<f", 120.))
    g.run(g.hooks[QUEST_CAMERA], camera)
    shake_samples.clear()
    g.cpu.mem_write(camera + 28, struct.pack("<f", 2.))

    step(Y, 116, 1480)
    for _ in range(5):
        contact(1, 900.)
    tick(A, frame=10.)
    # Confirm after A has already been buffered. GU's hurt immunity is still active.
    assert g.get(actor + 11072) == 0x41600000
    g.cpu.mem_write(actor + 64, struct.pack("<f", 500.))
    contact(1, 700.)
    assert list(units.values()) == [(layer, (50., 10., 155.)) for layer in range(180, 205)]
    assert len(events) == 1
    g.cpu.mem_write(matrix + 48, struct.pack("<3f", 75., 20., 150.))
    contact(1, 700.)
    assert len(units) == 25 and all(xyz(unit + 304) == (75., 20., 205.) for unit in units)
    assert len(events) == 1
    tick(frame=32.)
    expect(160, 1031)
    tick(frame=7.99)
    assert not shock_contacts(g, collision) and not shock_fx
    tick(frame=8.)
    tick(frame=12.)
    hitboxes = (actor + 12320, actor + 12504)
    assert any(g.half(box + 56) == 125 and g.cpu.mem_read(box + 86, 1) == b"\x3c"
               for box in hitboxes)
    tick(frame=15.)
    g.run(g.hooks[QUEST_CAMERA], camera)
    g.cpu.mem_write(camera + 64, bytes(g.cpu.mem_read(actor + 64, 12)))
    tick(frame=16.)
    g.run(g.hooks[QUEST_CAMERA], camera)
    assert shake_samples[0] == 0. and math.isclose(shake_samples[1], 11.98355, abs_tol=0.0001), shake_samples
    assert xyz(camera + 64)[1] == xyz(camera + 96)[1] == 0.
    # The request has its own scene clock and each later pulse samples the
    # current world position, even after the hunter has left its action.
    set_action(1, 0)
    spawned = {}
    positions = {6: (-114., 20., 170.), 10: (260., 20., 90.),
                 14: (-16., 20., 400.), 20: (60., 20., 550.)}
    flashes = {
        0: ((range(205, 207), (-90., 20., 0.)), (range(205, 207), (-120., 20., 80.))),
        2: ((range(205, 207), (-128., 20., 130.)), (range(234, 252), (-28., 160., 250.))),
        6: ((range(207, 212), positions[6]),),
        10: ((range(207, 212), positions[10]),),
        12: ((range(212, 221), (-18., 20., 400.)),),
        16: ((range(221, 234), (56., 20., 550.)),),
    }
    g.put(clock, 0)
    assert not shock_contacts(g, collision) and not shock_fx
    g.put(clock, 0x40000000)
    first_sound = len(sounds)
    for age in range(0, 52, 2):
        g.cpu.mem_write(actor + 64, struct.pack("<3f", age, 20., 0.))
        g.put(actor + 4076, 0x4000 if age == 10 else 0)
        active = shock_contacts(g, collision)
        expect_flashes(flashes.get(age, ()), yaw=90. if age == 10 else 0.)
        if age in positions:
            pulse, = active.keys() - spawned.keys()
            x, y, z = positions[age]
            expected = (2, 100., x, y + 400., z, x, y, z)
            assert all(math.isclose(a, b, abs_tol=0.001) for a, b in zip(active[pulse], expected))
            assert g.get(pulse + 12) == actor and (g.get(pulse + 48) & 0x300) == 0
            assert g.half(pulse + 56) == g.cpu.mem_read(pulse + 86, 1)[0] == 5
            assert g.cpu.mem_read(pulse + 108, 1) == b"\x01"
            spawned[pulse] = age, active[pulse], g.half(pulse + 52)
            confirm_world_refill(pulse, (x, y, z))
        assert active == {pulse: shape for pulse, (born, shape, _) in spawned.items() if age - born < 30}
    assert len(spawned) == len({serial for _, _, serial in spawned.values()}) == 4
    assert sounds[first_sound:] == [(actor, "blade", cue, (-90., 20., 0.)) for cue in (23, 5)]

    step(A, 117, 1481)
    assert not units
    contact(2, 260., owner=4, root_owner=1)
    tick(X, frame=10.)
    tick(frame=32.)
    expect(159, 1032)

    step(B, 158, 1447)
    contact(3, 260.)
    tick(A, frame=10.)
    tick(frame=44.)
    expect(162, 1449)
    tick(frame=14.)
    assert any(g.half(box + 56) == 125 for box in hitboxes)
    for _ in range(6):
        assert not shock_contacts(g, collision) and not shock_fx
    tick(frame=14., ended=True)
    expect(162, 1082)
    assert not shock_contacts(g, collision)
    assert [layer for layer, _, _ in shock_fx] == [205, 206, 205, 206]
    shock_fx.clear()

    step(A, 117, 1481)
    for _ in range(7):
        contact(1, 900.)
    # Extra GU Evasion frames must not extend the original contact window.
    g.put(actor + 11072, 0x42000000)
    contact(1, 200.)
    tick(X, frame=22.)
    tick(frame=32.)
    expect(95, 1032)

    step(Y, 107, 1493, impact=False)
    contact(1, 200.)
    tick(X, frame=20.)
    tick(frame=32.)
    expect(110, 1036)
    assert not units and len(events) == 3
    assert [packet[44] & 15 for packet in events] == [0, 1, 2]
    g.put(clock, saved_clock)
    g.put(0x18843F0, previous_network)
    for address, _ in particle_stubs:
        g.cpu.hook_del(g.traps.pop(address))
    g.cpu.hook_del(g.traps.pop(camera_original))
    g.cpu.hook_del(g.traps.pop(g.originals[COLLISION_PASS]))
    for address in (transport, 0x49694):
        g.cpu.hook_del(g.traps.pop(address))
    return events


def verify_melee(g, actor, vtable, part, shapes, set_action, tick, meter, motions, sounds):
    def byte(address):
        return g.cpu.mem_read(address, 1)[0]

    # Keep the native HDP constructor and collision expansion. Supply the
    # matrices GU would have evaluated from the two moving blade joints.
    monster, status, monster_vt, hurt, collision, credit = damage_target(g, vtable, native_affinity=True)
    for address in (monster + 46585, monster + 30290, status + 416):
        g.cpu.mem_write(address, b"\1")
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

    def confirm_world_refill(pulse, point):
        # The normal noncritical hit grants 5 × 600% = 30 units. The HUD
        # maps each 2000-unit segment to 16 pixels.
        before = sum(meter()[0])
        g.run(0x9D67C, monster)
        g.cpu.mem_write(collision + 28, struct.pack("<B3x4f", 1, 2., *point))
        g.run(0x16C0C0, collision, pulse, monster)
        after = sum(meter()[0])
        assert math.isclose(after - before, .24, abs_tol=.001), (before, after)
        g.run(0x9D67C, monster)

    evade_events = verify_magnetic_steps(g, actor, set_action, tick, meter, sounds, confirm_world_refill)
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
    # Walk-sheathe with the stick held, then tap X before GU returns to walking.
    # Exercise the real pad, including a late stock plain-draw request.
    from verify_inputs import NativePad
    pad = NativePad(g, actor)
    set_action(1, 3)
    g.cpu.mem_write(actor + 10057, bytes([1]))  # Before the native sheath clears drawn.
    pad.poll(X, strength=100)
    g.run(CHANGE_ACTION, actor, 1, 1, 0)
    assert (g.half(actor + 9478), g.half(actor + 9482)) == (1, 3)
    g.cpu.mem_write(actor + 10057, bytes(1))
    g.cpu.mem_write(actor + 5, bytes([1]))
    pad.dispatch()
    for _ in range(3):
        pad.poll(0, strength=100)
        pad.dispatch()
        assert (g.half(actor + 9478), g.half(actor + 9482)) == (1, 3)
    set_action(2, 0)
    pad.poll(0, strength=100)
    pad.dispatch()
    assert g.half(actor + 9482) == 0x8000 + 78
    assert motions[-1][0] == 1440
    pad.poll(0, strength=100)
    tick(frame=60., ended=True)
    assert g.half(actor + 9482) == 0x8000 + 78 and motions[-1][0] == 1049
    tick(frame=39.)
    assert byte(hit + 5) == 0
    tick(frame=42.)
    assert byte(hit + 5) == 1 and g.get(hit + 12) == g.get(hit + 112) == actor
    actual = g.half(hit + 56), byte(hit + 86), byte(hit + 108)
    assert actual == (43, 0, 1), actual
    assert struct.unpack("<2f", g.cpu.mem_read(hit + 92, 8)) == (0., 14.)
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

    verify_contact_clock(g, hit, REPO / "mhfz/eboot.bin.elf", 0, 14)
    verify_ranges(g, actor, part, hit, collision, shapes,
                  (ROOT / "target/assets/magnet_spike.fmsa").read_bytes(), REPO / "mhfz/eboot.bin.elf")
    g.put(collision + 10288, 0)
    g.run(0x1696FC, hit)

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

    # A move may change speed after contact birth. The SDT lifetime stays on scene time.
    g.cpu.mem_write(actor + 1292, struct.pack("<f", 1.75))
    for _ in range(7):
        g.run(0x16919C, hit)
        assert byte(hit + 5) == 1
    g.run(0x16919C, hit)
    assert byte(hit + 5) == 0
    g.cpu.mem_write(actor + 1292, struct.pack("<f", 1.))
    set_action(4, 14)  # End the physical pad fixture before injecting other commands.
    pad.poll(0)
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
    guard_packets, effects = verify_guard(g, actor, set_action, tick, meter, motions, sounds, collision)
    verify_peer(g, actor, vtable, part, monster, status, collision, credit, set_action, tick, meter,
                flight_packets, boost_packets, guard_packets, shot_data, sounds, effects, evade_events)
    verify_bind_readiness(g, actor, monster, status, hurt, collision, set_action, tick, shapes, meter, sounds)
    verify_strike_spawn(g, actor, monster, status, hurt, set_action, tick)
    verify_bind_drop(g, actor, set_action, tick, meter)


def verify_bind_drop(g, actor, set_action, tick, meter):
    configurations, landing_probes = [], []
    floor = 100.

    def put_float(address, value):
        g.cpu.mem_write(address, struct.pack("<f", value))

    def get_float(address):
        return struct.unpack("<f", g.cpu.mem_read(address, 4))[0]

    def landing_floor(owner, out, *_):
        assert owner == actor
        landing_probes.append(get_float(actor + 68))
        put_float(out, floor)
        return 1

    for address, callback in (
        (0x2E8B04, landing_floor),
        (0x282FD8, lambda owner, mode, *_: configurations.append((owner, mode))),
    ):
        g.cpu.hook_del(g.traps.pop(address))
        g.stub(address, callback)

    def expect(state, clip):
        actual = (g.half(actor + 9482), g.half(actor + 1204))
        assert actual == (0x8000 + state, clip), actual

    for impact, exit_path in itertools.product((False, True), ("recover", "roll", "end", "interrupt")):
        g.run(g.hooks[INIT_HUNTER], actor)
        set_action(1, 0)
        g.put(actor + 10057, 0)
        tick()
        g.put(actor + 10057, 1)
        put_float(actor + 5088, 1000.)
        put_float(actor + 28, 2.)
        if impact:
            tick(SPECIAL, native_pad=True)
            tick(frame=99., ended=True)
        tick(X, held=R, native_pad=True)
        if impact:
            tick(frame=28.)
        twin = (39, 1424) if impact else (11, 1045)
        start = 66. if impact else 72.
        expect(*twin)
        put_float(actor + 68, floor)
        put_float(actor + 5088, 0.)
        tick(X | A, held=R, frame=start - 4., fallback=1 << 14, native_pad=True)
        expect(*twin)
        put_float(actor + 5088, 1000.)
        assert meter()[0] == [16.] * 4 + [0.]
        count = len(configurations)
        if impact:
            assert tick(X | A, held=R, frame=start - 2., fallback=1 << 14, native_pad=True)
            expect(*twin)  # Buffer the chord, then release before the execution frame.
            assert tick(frame=start, fallback=1 << 14, native_pad=True)
        else:
            tick(frame=start, fallback=1 << 14, native_pad=True)
            expect(*twin)  # No automatic transition at the execution frame.
            assert tick(X | A, held=R, frame=start + 2., fallback=1 << 14, native_pad=True)
        bind = 94 if impact else 97
        expect(bind, 1052 if impact else 1421)
        assert meter()[0] == [16.] * 3 + [0., 0.]
        assert configurations[count:] == [(actor, 0)]

        launch = 20 if impact else 4
        tick(frame=2.)
        if impact:
            tick(frame=4.)
            assert get_float(actor + 68) == floor - 2.  # Event 25 starts before event 1.
            for frame in range(6, launch, 2):
                tick(frame=frame)
        assert configurations[count:] == [(actor, 0)]
        probes = len(landing_probes)
        tick(frame=launch)
        assert configurations[count:] == [(actor, 0), (actor, 2)]
        expect(bind, 1052 if impact else 1421)
        assert get_float(actor + 68) < floor and len(landing_probes) == probes
        assert get_float(actor + 11072) >= 4.
        if impact:
            # Move into the authored second clip while the floor is still locked out.
            tick(frame=22.)
            tick(frame=23., ended=True)
            expect(bind, 1421)
        else:
            tick(frame=6.)
        for frame in range(8, 44, 2):
            tick(frame=frame, fallback=1 << 14)
            expect(bind, 1421)
        for hit in (actor + 12320, actor + 12504):
            g.run(0x1687A8, hit)
        tick(frame=44.)
        assert g.half(actor + 12320 + 56) == 1
        assert g.get(actor + 12320 + 12) == actor

        # Use GU's floor sweep and landing callback. B during the drop must
        # not become an attack or roll of the equipped GU fallback weapon.
        tick(B, frame=48., native_pad=True, fallback=1 << 14)
        for frame in (46., 48.) * 16:
            if g.half(actor + 9482) == 0x8000 + 98:
                break
            expect(bind, 1421)
            tick(frame=frame, fallback=1 << 14, native_pad=True)
        expect(98, 1422)
        assert landing_probes[-1] < floor and get_float(actor + 68) == floor
        assert not (g.get(actor + 10057) & 0x40)
        tick(frame=15., ended=True)
        expect(99, 1442)
        put_float(actor + 1268, 0.)
        mode_bits = struct.unpack_from("<I", capture_action(g, actor), 16)[0] >> 22 & 15
        assert mode_bits == (8 | int(impact)), (impact, mode_bits)
        if exit_path == "interrupt":
            set_action(64, 6)  # GU interrupts the shared landing pose while drawn.
            original = g.originals[DISPATCH]
            forwarded = []
            g.stub(original, lambda *_: forwarded.append(g.half(actor + 9478)))
            tick()
            g.cpu.hook_del(g.traps.pop(original))
            assert forwarded == [64]
            set_action(1, 0)
            tick(X, native_pad=True)
            for _ in range(8):
                if g.half(actor + 9478) == WEAPON:
                    break
                tick(native_pad=True)
            assert g.half(actor + 9482) == 0x8000 + (18 if impact else 0), (
                impact, g.half(actor + 9478), g.half(actor + 9482))
            continue
        if exit_path == "roll":
            tick(B, frame=123., native_pad=True)
            expect(99, 1442)
            tick(B, frame=124., native_pad=True)
            assert (g.half(actor + 9478), g.half(actor + 9482)) == (1, 6)
            tick(X, frame=20.)
            tick(frame=40.)
            assert g.half(actor + 9482) == 0x8000 + (143 if impact else 140)
            continue
        if exit_path == "end":
            tick(frame=156., ended=True)
            expect(156 if impact else 100, 1443)
            continue
        tick(frame=153.)
        expect(99, 1442)
        put_float(actor + 1268, 1.)
        tick(frame=154.)
        expect(99, 1442)
        put_float(actor + 1268, 0.)
        tick(frame=154.)
        expect(156 if impact else 100, 1443)
        if impact:
            tick(frame=68.)
            expect(156, 1427)
        tick(frame=149., ended=True)
        assert (g.half(actor + 9478), g.half(actor + 9482)) == (1, 0)

    # The original recovery pose opens a normal GU roll at frame 124.
    set_action(WEAPON, 0x8000 + 99)
    tick()
    tick(B, frame=123., native_pad=True)
    expect(99, 1442)
    tick(B, frame=124., native_pad=True)
    assert (g.half(actor + 9478), g.half(actor + 9482)) == (1, 6)


def verify_bind_readiness(g, actor, monster, status, hurt, collision, set_action, tick, shapes, meter, sounds):
    quests, quest, factor, pose, roster = (g.alloc(n) for n in (168, 64, 4, 64, 9530))
    g.put(0x188AD90, quests)
    g.put(quests + 164, quest)
    g.put(0x1851FE0, roster)
    g.put(roster + 28, monster)
    g.put(status + 880, 1000)
    for address in (monster + 46585, monster + 30290, status + 416):
        g.cpu.mem_write(address, b"\1")
    g.cpu.mem_write(hurt, struct.pack("<7H", 5, 0, 0, 0, 0, 0, 3))
    g.cpu.mem_write(hurt + 44, b"\xff\xff")
    lookup = g.get(g.get(monster) + 208)
    clock = g.get(0x2126764) + 104
    pulses = []
    motion_effects = []
    ordinary_layers = []
    bound_effects = []
    caster_effects = []
    stopped = []
    stopped_caster = []
    monster_parent, get_parent = g.alloc(16), g.alloc(4)
    g.put(g.get(monster) + 304, get_parent)
    g.put(monster + 1172, g.alloc(160))
    g.put(monster + 1176, g.alloc(256))
    g.stub(get_parent, lambda owner, *_: monster_parent if owner == monster else 0)

    def submit(_, unit, handle, group):
        layer = g.get(g.cpu.reg_read(UC_ARM_REG_SP))
        assert handle == group == 1
        parent = g.get(unit + 320)
        point = struct.unpack("<3f", g.cpu.mem_read(unit + (144 if parent else 304), 12))
        if layer >= 573:
            if layer in range(1030, 1036):
                ordinary_layers.append((layer, parent, g.half(unit + 194), point))
            return 1
        if 284 <= layer < 290:
            pulses.append((layer, point))
        elif 499 <= layer < 545:
            joint = g.half(unit + 194)
            bound_effects.append((layer, parent, joint, point, unit))
            if parent:
                assert g.get(unit + 132) & 0x82 == 0x82
                assert g.cpu.mem_read(unit + 176, 12) == bytes(12)
        elif 545 <= layer < 573:
            assert not parent
            caster_effects.append((layer, point, unit))
        else:
            assert 13 <= layer < 37 or 290 <= layer < 499, layer
            joint = g.half(unit + 194) if parent else None
            motion_effects.append((layer, parent, joint, point))
        return 1

    def stop(unit, *_):
        units = {unit for _, _, unit in caster_effects}
        (stopped_caster if unit in units else stopped).append(unit)

    for address, callback in (
        (lookup, lambda owner, joint, *_: pose if (owner, joint) == (monster, 5) else 0),
        (0x3267D0, submit),
        (0x32A400, stop),
        (g.originals[COLLISION_PASS], lambda *_: 3),
    ):
        if address in g.traps:
            g.cpu.hook_del(g.traps.pop(address))
        g.stub(address, callback)

    def point(x):
        g.cpu.mem_write(pose, struct.pack("<16f", 1., 0., 0., 0., 0., 1., 0., 0.,
                                         0., 0., 1., 0., x, 30., 40., 1.))

    def spawn(kind, set_id, revision, rank):
        g.cpu.mem_write(monster + 46576, struct.pack("<H", kind))
        g.cpu.mem_write(status + 438, struct.pack("<H", set_id))
        g.cpu.mem_write(status + 23744, bytes([revision]))
        g.cpu.mem_write(quest + 44, bytes([rank]))

    def advance(frames, count, x=None):
        g.cpu.mem_write(clock, struct.pack("<f", frames))
        assert g.run(g.hooks[COLLISION_PASS], collision) == 3
        assert len(pulses) == count * 6, pulses
        if x is not None:
            assert pulses[-6:] == [(layer, (x, 30., 40.)) for layer in range(284, 290)]

    def fresh_strike():
        g.run(g.hooks[INIT_HUNTER], actor)
        blades = [actor + offset for offset in (12320, 12504)]
        for blade in blades:
            g.run(0x1687A8, blade)
        set_action(2, 0)
        g.cpu.mem_write(actor + 10057, b"\0")
        tick(X, native_pad=True)
        tick(frame=60., ended=True)
        tick(frame=40.)
        blade = next(blade for blade in blades if g.half(blade + 56) == 43)
        g.put(blade + 144, 0)
        return blade

    paralysis = g.alloc(16)
    g.put(g.get(monster + 30188) + 160, paralysis)
    g.cpu.mem_write(paralysis, struct.pack("<H", 400))
    g.cpu.mem_write(paralysis + 6, struct.pack("<H", 200))
    g.run(0xA2AA8, monster)  # Initialize native paralysis buildup and threshold.
    hit = fresh_strike()

    def contacts(count):
        for _ in range(count):
            g.put(factor, 0x3f800000)
            g.run(g.hooks[0x17885C], 0, hit, monster, hurt, actor, factor)
            assert g.get(factor) == 0x3f800000

    g.cpu.mem_write(actor + 4180, b"\0")
    g.cpu.mem_write(monster + 4180, b"\0")
    point(80.)
    # Drive actual GU hit callbacks. Switch the paralysis row independently
    # of species to prove that neither a known ID nor a high rank enables it.
    tune_slot = g.get(monster + 30188) + 160
    spawn(1, 125, 10, 1)
    for ptr, threshold, duration in ((0, 400, 200), (paralysis, 0xffff, 200),
                                     (paralysis, 0, 200), (paralysis, 400, 0)):
        g.put(tune_slot, ptr)
        g.cpu.mem_write(paralysis, struct.pack("<H", threshold))
        g.cpu.mem_write(paralysis + 6, struct.pack("<h", duration))
        contacts(50)
        advance(2., 0)
    g.cpu.mem_write(paralysis + 6, struct.pack("<H", 200))
    contacts(1)
    advance(2., 0)  # The ineligible hits did not build a hidden meter.
    # The same first bind threshold applies across quest ranks and species.
    for i, (kind, rank) in enumerate(((0x50, 1), (0x50, 2), (0x50, 4),
                                      (0x450, 4), (0x0e, 1), (0x7ffe, 3))):
        spawn(kind, 130 + i, 1, rank)
        contacts(41)
        advance(2., i)
        contacts(1)
        advance(2., 1 + i, 80.)

    hit = fresh_strike()
    pulses.clear()
    point(12.)
    spawn(1, 123, 9, 1)
    contacts(1)
    advance(1., 0)  # No projectile or mark; a first ordinary hit has no ready flash.
    contacts(40)  # 1763 units, just below the shared threshold of 1800.
    advance(1., 0)
    g.cpu.mem_write(monster + 4180, b"\1")
    contacts(1)
    g.cpu.mem_write(monster + 4180, b"\0")
    advance(2., 0)
    contacts(1)
    advance(0., 0)
    g.put(quests + 164, 0)
    advance(2., 0)
    g.put(quests + 164, quest)
    advance(2., 1, 12.)
    point(80.)
    advance(21., 1)
    advance(1., 2, 80.)
    advance(22., 3, 80.)
    advance(29., 3)
    advance(1., 4, 80.)

    spawn(1, 123, 10, 4)
    advance(40., 4)  # A new spawn cannot use the old one's charge or queued pulse.
    contacts(41)
    advance(2., 4)  # 1763 is below the shared threshold in a G quest too.
    contacts(1)
    advance(2., 5, 80.)
    spawn(1, 124, 10, 4)
    advance(100., 5)
    spawn(12, 125, 10, 1)
    g.put(tune_slot, 0)  # A spawn without a paralysis row.
    contacts(42)
    advance(2., 5)
    g.put(tune_slot, paralysis)

    # A bind opens at state 98's landing event. Use GU's actual action request
    # and shared entry; model/AI fields outside the fixture are stubbed.
    networked = [False]
    for address, callback in (
        (0x5BB10, lambda *_: networked[0]),
        (0x496F0, lambda *_: 0),
        (0x49580, lambda *_: 0),
        (0x49D5C, lambda *_: 0),
        (0x49D94, lambda *_: 0),
        (0xA7E84, lambda *_: 0),
        (0xA1DC4, lambda *_: 0),
    ):
        if address in g.traps:
            g.cpu.hook_del(g.traps.pop(address))
        g.stub(address, callback)

    def controls(group=0, slot=0, ailments=0, pending=0):
        g.cpu.mem_write(monster + 29664, bytes([group, slot]))
        g.cpu.mem_write(status + 24072, bytes([ailments, pending]))

    def timer():
        return struct.unpack("<f", g.cpu.mem_read(status + 24076, 4))[0]

    def pose_state(state, frame=0., buttons=0, ended=False, next_frame=None):
        g.cpu.mem_write(actor + 1276, struct.pack("<f", frame if next_frame is None else next_frame))
        tick(buttons, frame=frame, ended=ended, native_pad=True, fallback=1 << 14)
        assert g.half(actor + 9482) == 0x8000 + state, g.half(actor + 9482)

    def landing():
        g.put(actor + 1268, 0)
        g.cpu.mem_write(actor + 64, struct.pack("<3f", 0., 30., -40.))
        g.cpu.mem_write(actor + 4076, b"\0\0")
        set_action(WEAPON, 0x8000 + 98)
        pose_state(98)

    def collide(frames):
        g.cpu.mem_write(clock, struct.pack("<f", frames))
        assert g.run(g.hooks[COLLISION_PASS], collision) == 3

    def released_since(before):
        assert [(layer, parent, joint) for layer, parent, joint, _, _ in bound_effects[before:]] == [
            (layer, monster_parent, 5) for layer in range(519, 545)
        ]

    def native_tick():
        g.cpu.reg_write(UC_ARM_REG_S0, 0x3F800000)
        g.run(0x72128, monster, status + 24076)
        collide(2.)

    def panel(label, travel=0, *, ready=False):
        caption, tracks = meter(panel=True)
        assert caption == label and tracks == [], (caption, tracks, label)
        art = [(frame, pos) for _, frame, pos in meter(artwork=True) if 508 <= frame <= 530]
        if label is None:
            assert art == []
        else:
            plate, cursor = art
            assert plate == (508, (553., 353.5))
            assert (520 if ready else 509) <= cursor[0] <= (530 if ready else 519)
            assert cursor[1] == (543. + travel, 342.), cursor

    camera, command_query, stick_strength = g.alloc(2800), g.alloc(4), g.alloc(4)
    g.put(camera, 0x1730D0C)
    g.cpu.mem_write(camera + 28, struct.pack("<f", 2.))
    camera_commands = set()
    g.stub(command_query, lambda _hunter, command, *_: int(command in camera_commands))
    g.stub(stick_strength, lambda *_: 31)

    def orbit(expected, *, toward, owner=actor, scripted=False, manual=False):
        vtable, camera_hunter_vtable = g.get(owner), g.alloc(2224)
        g.cpu.mem_write(camera_hunter_vtable, bytes(g.cpu.mem_read(vtable, 2224)))
        g.put(camera_hunter_vtable + 744, command_query)
        g.put(camera_hunter_vtable + 568, stick_strength)
        g.put(owner, camera_hunter_vtable)
        g.put(camera + 2140, owner)
        g.put(camera + 2540, 0x1357)
        g.put(camera + 2608, 0)
        g.put(camera + 2712, int(scripted))
        position = bytes(g.cpu.mem_read(monster + 64, 12))
        g.cpu.mem_write(monster + 64, struct.pack("<3f", *toward))
        options = g.get(0x188CA48)
        if manual:
            camera_commands.add(3)
            mode = g.alloc(128)
            g.cpu.mem_write(mode + 101, b"\x03")
            g.put(0x188CA48, mode)
        try:
            if manual:
                assert g.run(g.originals[QUEST_CAMERA_ORBIT], camera, 0) == 1
                adjustment = g.get(camera + 2540) - 0x1357
                assert adjustment > 0
                expected += adjustment
                g.put(camera + 2540, 0x1357)
                g.put(camera + 2608, 0)
            assert g.run(QUEST_CAMERA_ORBIT, camera, 0) == (1 if manual else 255)
            assert g.get(camera + 2608) == (2 if manual else 0)
            assert g.get(camera + 2540) == expected, hex(g.get(camera + 2540))
        finally:
            camera_commands.clear()
            g.put(0x188CA48, options)
            g.put(owner, vtable)
            g.cpu.mem_write(monster + 64, position)

    def lens(radius, degrees, *, elapsed=2., owner=actor, locked=False, finisher=False, wall=False):
        # Enter the installed camera hook, then let GU's 26934 run up to the
        # actual terrain-ray boundary with the adjusted positions.
        g.run(QUEST_CAMERA_UPDATE, camera)
        assert struct.unpack("<f", g.cpu.mem_read(camera + 60, 4))[0] == 50.
        g.put(camera + 152, 1)
        g.put(camera + 2140, owner)
        g.put(camera + 2712, int(locked))
        g.cpu.mem_write(camera + 28, struct.pack("<f", elapsed))
        height, = struct.unpack("<f", g.cpu.mem_read(actor + 68, 4))
        g.cpu.mem_write(camera + 64, struct.pack("<3f", 10., height + 120., 380.))
        g.cpu.mem_write(camera + 96, struct.pack("<3f", 10., height + 100., -20.))
        eye_y = height + (140. if finisher else 120.)
        target_y = height + (170. if finisher else 100.)
        sampled = []

        def ray(_, eye, target, *_args):
            sampled.append((struct.unpack("<3f", g.cpu.mem_read(eye, 12)),
                            struct.unpack("<3f", g.cpu.mem_read(target, 12))))
            if wall:
                hit = g.get(g.cpu.reg_read(UC_ARM_REG_SP))
                g.cpu.mem_write(hit, struct.pack("<3f", 10., (eye_y + target_y) / 2., 180.))
            return int(wall)

        options = g.get(0x188CA48)
        g.put(0x188CA48, lens_options)
        g.stub(0x183724, ray)
        try:
            g.run(QUEST_CAMERA_TERRAIN, camera, 3)
        finally:
            g.cpu.hook_del(g.traps.pop(0x183724))
            g.put(0x188CA48, options)
            g.put(camera + 2712, 0)
        [(eye, target)] = sampled
        assert eye[:2] == (10., eye_y) and math.isclose(eye[2], radius - 20., abs_tol=.0001), eye
        assert target == (10., target_y, -20.), target
        if wall:
            assert struct.unpack("<3f", g.cpu.mem_read(camera + 64, 12)) == (10., (eye_y + target_y) / 2., 180.)
        fov, = struct.unpack("<f", g.cpu.mem_read(camera + 60, 4))
        assert math.isclose(fov, degrees, abs_tol=.0001), (fov, degrees)
        return fov

    lens_options = g.alloc(128)
    lens_update = g.originals[QUEST_CAMERA_UPDATE]
    g.stub(lens_update, lambda *_: 0)
    g.stub(0x17FA4C, lambda *_: 0)
    g.cpu.mem_write(camera + 60, struct.pack("<f", 50.))

    def approach(degrees, target, ticks=1):
        return target + (degrees - target) * 0.95 ** ticks

    struggle = "磁缚　转动左摇杆，连按 X 或 A"
    qualified = "磁缚达标　等待终结窗口"
    finish = "按 ZR 使出磁缚终结"
    panel(None)
    controls()
    spawn(0x50, 126, 1, 1)
    g.cpu.mem_write(hurt + 16, struct.pack("<f", 65.))
    g.cpu.mem_write(status + 23726, struct.pack("<H", 133))
    contacts(41)
    point(100.)
    landing()
    collide(2.)
    assert timer() == 0.
    contacts(1)
    point(870.)  # Root is outside; the native hurt sphere also misses by 5.
    collide(2.)
    assert timer() == 0.
    point(864.)
    # The sphere is anchored 80 units forward at casting, independent of
    # the hunter's root once the landing action has spawned the contact.
    g.cpu.mem_write(actor + 64, struct.pack("<3f", -400., 30., -40.))
    g.cpu.mem_write(hurt + 10, struct.pack("<H", 2))
    collide(2.)
    assert timer() == 0.
    g.cpu.mem_write(hurt + 10, b"\0\0")
    g.cpu.mem_write(status + 442, b"\1")
    collide(2.)
    assert timer() == 0.
    g.cpu.mem_write(status + 442, b"\0")
    networked[0] = True
    peers = g.get(0x1885B58) + 32
    previous_peers = bytes(g.cpu.mem_read(peers, 12))
    g.put(peers, g.get(peers) or g.alloc(14000))
    collide(2.)
    assert timer() == 0.
    g.cpu.mem_write(peers, bytes(12))  # Solo hub, same quest-context flag and no remote hunter.
    before_sound = len(sounds)
    collide(0.)
    assert timer() == 0.
    orbit(0x1357, toward=(200., 30., -40.))
    lens(400., 50.)
    collide(2.)
    reaction = (timer(), g.half(monster + 29664), g.half(status + 24072))
    assert reaction == (960., 0x1F0A, 0), reaction
    networked[0] = False
    g.cpu.mem_write(peers, previous_peers)
    assert g.get(status + 880) == 1000 and g.half(status + 23726) == 133
    assert g.half(status + 24072) == 0
    assert [(layer, point) for layer, point, _ in caster_effects] == [
        (layer, (-400., 30., 60.)) for layer in range(545, 555)
    ]
    assert sounds[before_sound:] == [(actor, "blade", 18, (-400., 30., -40.))]
    orbit(0x1357, toward=(200., 30., -40.))  # Confirmation still occurs in landing state 98.
    lens(400., 50.)
    # This landing holds at hunt frame 240, a world aura boundary. The
    # attached pulse first appears at frame 256 and repeats at 288.
    assert [(layer, parent, point) for layer, parent, _, point, _ in bound_effects] == [
        (layer, 0, (864., 230., 40.)) for layer in range(499, 506)
    ]
    panel(struggle)
    collide(0.)
    assert len(bound_effects) == 7
    panel(struggle)
    collide(15.)
    assert len(bound_effects) == 7
    collide(1.)
    assert len(caster_effects) == 10
    collide(8.)
    pulse = caster_effects[10:]
    assert [(layer, point) for layer, point, _ in pulse] == [
        (layer, (-400., 80., 60.)) for layer in range(570, 573)
    ]
    unit = pulse[0][2]
    pitch, yaw, roll = struct.unpack("<3f", g.cpu.mem_read(unit + 160, 12))
    assert 2. < pitch < 3. and 90. < yaw < 92. and roll == 0.
    g.cpu.mem_write(actor + 64, struct.pack("<3f", -320., 30., -40.))
    point(-320.)
    collide(0.)
    assert g.cpu.mem_read(unit + 304, 12) == struct.pack("<3f", -400., 80., 60.)
    collide(2.)
    assert g.cpu.mem_read(unit + 304, 12) == struct.pack("<3f", -320., 80., 60.)
    assert struct.unpack_from("<f", g.cpu.mem_read(unit + 160, 12), 4)[0] == 180.
    point(864.)
    collide(22.)
    assert [layer for layer, _, _ in caster_effects[13:]] == list(range(570, 573))
    assert [(layer, parent, joint) for layer, parent, joint, _, _ in bound_effects[7:]] == [
        (layer, monster_parent, 5) for layer in range(506, 519)
    ] * 2
    assert not stopped  # Prior pulses can overlap in the source bank.

    pose_state(99, frame=15., ended=True)
    pose_state(99, frame=124., buttons=B)
    pose_state(99, frame=154.)
    orbit(0x4000, toward=(-600., 30., -40.))
    orbit(0xC000, toward=(200., 30., -40.), manual=True)
    orbit(0x8000, toward=(-320., 60., 60.))
    orbit(0x1357, toward=(200., 30., -40.), scripted=True)
    orbit(0x1357, toward=(200., 30., -40.), owner=g.get(g.get(0x1885B58) + 32))
    degrees = lens(400., 51.)
    lens(410., degrees, elapsed=0.)
    lens(400., 50., locked=True)
    lens(400., 50., owner=g.get(g.get(0x1885B58) + 32))
    degrees = lens(410., approach(degrees, 70.), wall=True)
    g.cpu.mem_write(status + 23744, b"\2")
    orbit(0x1357, toward=(200., 30., -40.))
    g.cpu.mem_write(status + 23744, b"\1")
    assert not g.run(0xA0D10, monster, status, 0, 0x100)
    collide(2.)
    set_action(1, 0)
    before_release = len(bound_effects)
    collide(2.)
    released_since(before_release)
    orbit(0x1357, toward=(200., 30., -40.))
    lens(420., approach(degrees, 50.))
    g.cpu.mem_write(actor + 4180, b"\1")
    lens(400., 50.)
    g.cpu.mem_write(actor + 4180, b"\0")
    assert sorted(stopped) == sorted(unit for _, _, _, _, unit in bound_effects[:before_release])
    assert set(stopped_caster) == {unit for layer, _, unit in caster_effects if layer >= 570}
    assert timer() == 0.
    panel(None)
    assert g.run(0xA0D10, monster, status, 0, 0x100) == 1

    first_unit = len(bound_effects)
    controls()
    landing()
    collide(2.)
    assert timer() == 960.
    collide(28.)  # Cross the next aura and pulse before leaving the area.
    assert len(bound_effects) > first_unit
    g.cpu.mem_write(actor + 4180, b"\1")
    before_release = len(bound_effects)
    before_stop = len(stopped)
    collide(2.)
    assert timer() == 0. and len(bound_effects) == before_release
    assert set(stopped[before_stop:]) == {unit for _, _, _, _, unit in bound_effects[first_unit:]}
    g.cpu.mem_write(actor + 4180, b"\0")
    set_action(1, 0)
    tick(native_pad=True)

    first_unit = len(bound_effects)
    controls()
    landing()
    collide(2.)
    assert timer() == 960.
    collide(26.)
    assert len(bound_effects) > first_unit
    spawn(0x50, 126, 2, 1)  # The same GU roster slot now holds a new spawn.
    controls(10, 31)
    g.put(status + 24076, 0x42C80000)
    before_release = len(bound_effects)
    before_stop = len(stopped)
    collide(2.)
    assert timer() == 100. and len(bound_effects) == before_release
    assert set(stopped[before_stop:]) == {unit for _, _, _, _, unit in bound_effects[first_unit:]}
    g.put(status + 24076, 0)
    controls()
    set_action(1, 0)
    tick(native_pad=True)
    contacts(42)  # The replacement starts with its own accumulation.
    point(870.)
    landing()
    collide(14.)
    point(864.)
    collide(2.)
    assert timer() == 0.
    pose_state(99, frame=15., ended=True)
    pose_state(100, frame=154.)
    before_sound = len(sounds)
    before_caster = len(caster_effects)
    landing()
    collide(15.)  # One collision update spans the whole 14-frame landing contact.
    assert timer() == 960.
    pose_state(99, frame=15., ended=True)
    pose_state(99, frame=154.)
    g.put(monster + 28, 0x40000000)
    for _ in range(71):
        native_tick()
    assert timer() == 818.  # Monster-side timer still runs beneath the input clock.
    panel(struggle, 120)
    before_release = len(bound_effects)
    native_tick()
    released_since(before_release)
    assert timer() == 0.
    panel(None)
    pose_state(100, frame=150.)
    assert [cue for _, name, cue, _ in sounds[before_sound:] if name == "blade" and cue in (18, 19)] == [18, 19]
    assert [layer for layer, _, _ in caster_effects[before_caster:] if 555 <= layer < 570] == list(range(555, 570))
    controls()

    landing()
    contacts(79)  # 3397 is below the second threshold of 3420.
    collide(2.)
    assert timer() == 0.
    contacts(1)
    collide(2.)
    assert timer() == 960.
    pose_state(99, frame=15., ended=True)
    pose_state(99, frame=154., buttons=ZR)  # An early finish press cannot open the window.
    # Rathian: initial 72 ticks, 30 accepted inputs add 120 ticks to the
    # 70% threshold. Alternate buttons first, then exercise stick-only input.
    for age in range(192):
        buttons = 0
        if age < 90 and age % 6 == 0:
            buttons = X if age % 12 == 0 else A
        if 90 <= age < 180 and age % 6 == 0:
            g.cpu.mem_write(actor + 5132, struct.pack("<H", (age // 6 % 4) * 0x4000))
            g.cpu.mem_write(actor + 5136, struct.pack("<h", 100))
        pose_state(99, frame=154., buttons=buttons)
        native_tick()
        if age == 6:
            panel(struggle, -1)
        if age == 174:
            panel(qualified, 93, ready=True)
    panel(finish, 122, ready=True)
    pose_state(99, frame=154., buttons=A)  # Only ZR accepts the finish window.
    native_tick()
    panel(finish, 122, ready=True)
    degrees = lens(400., approach(50., 70., 10), elapsed=20.)
    pose_state(105, frame=154., buttons=ZR)
    panel(None)
    orbit(0xC000, toward=(200., 30., -40.))
    assert g.half(actor + 1204) == 1488
    g.cpu.mem_write(actor + 1280, struct.pack("<f", 175.))
    degrees = lens(500., approach(degrees, 70.))
    g.cpu.mem_write(actor + 1280, struct.pack("<f", 176.))
    g.cpu.mem_write(actor + 1268, struct.pack("<f", 1.))
    degrees = lens(500., approach(degrees, 70.))
    g.put(actor + 1268, 0)
    # GU stores a loaded bank and slot here. 1488 is the source clip ID.
    g.cpu.mem_write(actor + 1204, struct.pack("<H", 0x858))
    degrees = lens(505., approach(degrees, 55.), finisher=True, wall=True)
    g.cpu.mem_write(actor + 9482, struct.pack("<H", 0x8000 + 157))
    lens(505., degrees, elapsed=0., finisher=True)
    g.cpu.mem_write(actor + 9482, struct.pack("<H", 0x8000 + 105))
    g.cpu.mem_write(actor + 1204, struct.pack("<H", 1488))
    g.cpu.mem_write(actor + 1280, struct.pack("<f", 559.))
    degrees = lens(505., approach(degrees, 55.), finisher=True)
    g.cpu.mem_write(actor + 1280, struct.pack("<f", 560.))
    lens(415., 50., elapsed=300.)
    lens(400., 50.)
    g.cpu.hook_del(g.traps.pop(lens_update))
    g.cpu.hook_del(g.traps.pop(0x17FA4C))
    parent = g.alloc(16)
    getter = g.alloc(4)
    g.put(g.get(actor) + 304, getter)
    g.stub(getter, lambda *_: parent)
    before_fx = len(motion_effects)
    pose_state(105, frame=53., next_frame=54.)
    g.cpu.mem_write(actor + 1268, struct.pack("<f", 1.))
    pose_state(105, frame=54., next_frame=56.)
    assert len(motion_effects) == before_fx
    g.put(actor + 1268, 0)
    pose_state(105, frame=54., next_frame=56.)
    assert motion_effects[before_fx:] == [
        (layer, parent, 12, (-90., 0., 0.)) for layer in range(314, 316)
    ]
    pose_state(105, frame=54., next_frame=56.)
    assert len(motion_effects) == before_fx + 2
    g.cpu.mem_write(actor + 5136, b"\0\0")
    g.cpu.mem_write(monster + 64, struct.pack("<3f", 400., 30., -40.))
    pose_state(105, frame=80.)
    assert g.half(actor + 4076) == 0x4000
    for contact in (actor + 12320, actor + 12504):
        g.run(0x1687A8, contact)
    cue_start = len(sounds)
    pose_state(105, frame=113., next_frame=114.)
    pose_state(105, frame=114., next_frame=115.)
    assert [cue for _, bank, cue, _ in sounds[cue_start:] if bank == "blade" and cue == 26] == [26]
    before_fx = len(motion_effects)
    pose_state(105, frame=183., next_frame=184.)
    assert len(motion_effects) == before_fx
    assert not any(g.half(actor + at + 56) == 600 for at in (12320, 12504))
    pose_state(105, frame=184., next_frame=186.)
    x, y, z = struct.unpack("<3f", g.cpu.mem_read(actor + 64, 12))
    impact = motion_effects[before_fx:]
    assert [layer for layer, _, _, _ in impact] == list(range(381, 436)) * 2, len(impact)
    assert all(parent == 0 and joint is None and math.dist(point, (x + 170., y, z)) < .001
               for _, parent, joint, point in impact[:55]), impact[:3]
    assert all(owner == parent and joint == 0xffff and point == (0.,0.,170.)
               for _, owner, joint, point in impact[55:])
    assert [cue for _, bank, cue, _ in sounds[cue_start:] if bank == "blade" and cue in (26, 27)] == [26,27]
    strike = next(actor + at for at in (12320, 12504) if g.half(actor + at + 56) == 600)
    assert g.get(strike + 132) == shapes[r"player\hit\frontier_ms\bind_finisher"]
    verify_contact_clock(g, strike, REPO / "mhfz/eboot.bin.elf", 4, 30)
    g.put(collision + 10288, 0)
    assert g.run(0x1696FC, strike) == 1
    kind, radius = struct.unpack("<B3xf", g.cpu.mem_read(collision + 8240, 8))
    assert (kind, radius) == (1, 700.)
    serial = g.half(strike + 52)
    pose_state(105, frame=184., next_frame=186.)
    assert g.half(strike + 52) == serial
    assert len(motion_effects) == before_fx + 110
    for _ in range(3):
        g.run(0x16919C, strike)
    xyz = g.cpu.mem_read(collision + 8248, 12)
    g.put(status + 880, 2000)
    g.put(status + 884, 2000)
    direction_table, enemy_data = g.alloc(32), g.alloc(120)
    g.put(monster + 30184, enemy_data)
    g.put(enemy_data + 112, direction_table)
    g.cpu.hook_del(g.traps.pop(g.originals[0x9A828]))

    def strike_at(position):
        g.cpu.mem_write(collision + 28, struct.pack("<B3xf", 1, 2.) + position)
        g.run(0x16C0C0, collision, strike, monster)

    g.run(0x9D67C, monster)
    # An adjacent spawn inside the weapon sphere is ineligible while the real captive is active.
    revision = g.cpu.mem_read(status + 23744, 1)[0]
    g.cpu.mem_write(status + 23744, bytes([revision ^ 1]))
    strike_at(bytes(xyz))
    assert g.half(status + 1160) == 0
    g.cpu.mem_write(status + 23744, bytes([revision]))
    verify_marked_row(g, strike, monster, hurt, collision)
    strike_at(struct.pack("<3f", 5000., 30., 0.))
    assert bytes(g.cpu.mem_read(collision + 8248, 12)) == bytes(xyz)
    damage = g.half(status + 1160)
    assert 0 < damage < 2000 and g.half(monster + 29664) == 0x1F0A
    g.run(0x97DDC, monster)
    assert g.get(status + 880) == 2000 - damage
    reaction = (g.half(status + 942), g.half(monster + 29664), timer())
    assert reaction == (33, 0xA70A, 0.), reaction
    assert g.half(status + 24072) == 0 and g.half(status + 23726) == 133
    # The GU rider that wins the retail gauge selects this same reaction.
    set_action(4, 13)
    assert g.run(0x2F7B6C, actor) == 1
    set_action(WEAPON, 0x8000 + 105)
    before_release = len(bound_effects)
    collide(2.)
    released_since(before_release)
    collide(2.)
    assert len(bound_effects) == before_release + 26

    def restore_ground_target():
        controls()
        g.put(status + 880, 2000)
        g.cpu.mem_write(status + 958, struct.pack("<H", 1000))
        g.run(0x9D67C, monster)

    # FZ 816FE82A skips unrestrained monsters even inside the finisher sphere.
    restore_ground_target()
    spawn(1, 127, 1, 1)
    strike_at(struct.pack("<3f", 5000., 30., 0.))
    g.run(0x97DDC, monster)
    assert g.get(status + 880) == 2000 and g.half(monster + 29664) == 0
    strike_at(bytes(xyz))
    g.run(0x97DDC, monster)
    assert g.get(status + 880) == 2000 and g.half(monster + 29664) == 0

    # Roster reuse between contact and reaction keeps the new spawn safe.
    restore_ground_target()
    strike_at(bytes(xyz))
    spawn(1, 127, 2, 1)
    g.run(0x97DDC, monster)
    assert g.half(monster + 29664) == 0

    # A later unrelated damage batch must not inherit an undelivered topple.
    restore_ground_target()
    strike_at(bytes(xyz))
    g.run(0x9D67C, monster)
    g.cpu.mem_write(status + 1092, b"\1")
    g.cpu.mem_write(status + 1160, struct.pack("<H", 1))
    g.run(0x97DDC, monster)
    assert g.get(status + 880) == 1999 and g.half(monster + 29664) == 0

    g.run(g.hooks[INIT_HUNTER], actor)
    assert timer() == 0.

    controls(10, 31)
    g.run(0xA2C04, monster)  # A real GU paralysis entry still uses native tuning.
    assert timer() == 200. and g.half(status + 24072) == 4
    assert g.half(status + 23726) == 0
    g.run(0x9D67C, monster)
    strike_at(bytes(xyz))
    g.run(0x97DDC, monster)
    assert timer() == 200. and g.half(status + 24072) == 4
    assert g.half(monster + 29664) == 0x1F0A
    g.put(status + 24076, 0)
    controls()

    def network_landing():
        nonlocal hit
        hit = fresh_strike()
        spawn(1, 128, 3, 1)
        contacts(42)
        point(100.)
        landing()
        collide(2.)
        panel(struggle)

    def replica_effects(receive, start, stop):
        equipment = g.get(actor + 5060)
        g.put(actor + 5060, g.alloc(1376))  # The observer uses a GU weapon.
        before = len(bound_effects)
        before_caster = len(caster_effects)
        receive(start)
        collide(60.)
        assert {layer for layer, _, _, _, _ in bound_effects[before:]} == set(range(499, 519))
        active_units = {unit for _, _, _, _, unit in bound_effects[before:]}
        before_release = len(bound_effects)
        before_stop = len(stopped)
        receive(stop)
        collide(2.)
        released_since(before_release)
        assert set(stopped[before_stop:]) == active_units
        collide(60.)
        assert len(bound_effects) == before_release + 26
        assert len(caster_effects) == before_caster
        g.put(actor + 5060, equipment)

    networked[0] = True
    verify_ground_hold_replication(g, monster, status, network_landing, replica_effects)
    assert [(layer, joint, pos) for layer, parent, joint, pos in ordinary_layers[-6:] if parent] == [
        (layer, 0xffff, (20., 90., 30.)) for layer in range(1030, 1036)
    ], ordinary_layers[-6:]
    networked[0] = False

    def arm_snapshot():
        nonlocal hit
        hit = fresh_strike()
        spawn(1, 129, 4, 1)
        contacts(41)
        point(100.)

    snapshot = verify_bind_snapshot(g, actor, monster, status, arm_snapshot, contacts, collide, pulses)
    verify_bind_exchange(g, actor, monster, status, collision, snapshot, landing, collide, panel,
                         set_action, tick, caster_effects, orbit)
    g.run(g.hooks[INIT_HUNTER], actor)
    g.put(quests + 164, 0)
    g.stub(g.originals[0x9A828], lambda *_: 0)


def verify_ground_hold_replication(g, monster, status, landing, effects):
    saved_network = g.get(0x18843F0)
    network, room, process = (g.alloc(size) for size in (1604, 240, 48))
    g.put(0x18843F0, network)
    g.put(network + 520, room)
    g.put(room + 232, 2)
    g.put(network + 992 + 4 * 60, process)
    g.cpu.mem_write(network + 1598, b"\1")
    g.cpu.mem_write(process + 16, b"\1")
    g.put(process + 36, 1)
    member = [0]
    g.cpu.hook_del(g.traps.pop(0x49694))
    g.stub(0x49694, lambda *_: member[0])
    g.cpu.mem_write(monster + 29732, b"\0")
    g.cpu.mem_write(status + 424, b"\x08")
    vtable = g.get(monster)
    g.put(vtable + 1060, 0xCB1F8)
    g.put(vtable + 1068, 0xCB330)
    g.put(vtable + 1072, 0xCC154)
    g.put(0x185A794, 0x9877)
    g.cpu.mem_write(g.get(0x1851FE0) + 9492, b"\1")

    transport = g.originals[0x9FC3D8]
    sent = []

    def send(sender, target, packet, priority):
        assert (sender, target, priority, g.get(packet)) == (process, 1, 16, 0x1741484)
        sent.append(bytes(g.cpu.mem_read(packet, 204)))
        return 1

    g.stub(transport, send)
    duration = struct.unpack("<I", struct.pack("<f", 960.))[0]
    g.cpu.mem_write(status + 23726, struct.pack("<H", 133))
    landing()
    assert len(sent) == 1 and struct.unpack_from("<I", sent[0], 64)[0] >> 20 == 960
    assert g.get(status + 24076) == duration and g.half(status + 23726) == 133
    g.run(0x76AFC, monster, 0, 0)  # The controller sends its next ordinary action.
    assert len(sent) == 2 and struct.unpack_from("<I", sent[1], 64)[0] >> 20 == 0
    g.cpu.hook_del(g.traps.pop(transport))

    packet = g.alloc(204)
    member[0] = 1

    def receive(sample):
        g.cpu.mem_write(packet, sample)
        assert g.run(0x1F4554, process, packet, 1, sample[9]) == 1

    g.cpu.mem_write(monster + 29728, b"\0")
    g.put(status + 24076, 0)
    g.cpu.mem_write(status + 23913, b"\x02")
    receive(sent[0])
    assert g.half(monster + 29664) == 0x1F0A and g.get(status + 24076) == duration
    assert (g.half(status + 24072), g.half(status + 23726), g.cpu.mem_read(status + 23913, 1)) == (
        0, 133, b"\x02",
    )
    receive(sent[1])
    assert g.half(monster + 29664) == 0 and g.get(status + 24076) == 0
    assert g.half(status + 23726) == 133 and g.cpu.mem_read(status + 23913, 1) == b"\x02"
    effects(receive, sent[0], sent[1])

    # A normal action packet still enters paralysis. An old spawn, a stale
    # controller, or a locally controlled target cannot use the SDK timer.
    current = bytearray(sent[0])
    previous_owner, previous_spawn, vanilla = (current.copy() for _ in range(3))
    previous_owner[9] = 2
    previous_spawn[14] ^= 1
    flags = struct.unpack_from("<I", current, 64)[0]
    struct.pack_into("<I", vanilla, 64, flags & 0xFFFFF)
    for sample, locally_owned in ((current, 1), (previous_owner, 0), (previous_spawn, 0), (vanilla, 0)):
        g.cpu.mem_write(monster + 29728, bytes([locally_owned]))
        receive(bytes(sample))
        assert g.get(status + 24076) == struct.unpack("<I", struct.pack("<f", 200.))[0]
        assert (g.half(status + 24072), g.half(status + 23726)) == (4, 0)
        g.cpu.mem_write(monster + 29728, b"\0")
        g.cpu.mem_write(monster + 29664, b"\0\0")
        g.cpu.mem_write(status + 24072, b"\0\0")
        g.put(status + 24076, 0)

    g.cpu.mem_write(monster + 29728, b"\1")
    g.cpu.mem_write(status + 424, b"\0")
    member[0] = 0
    g.put(0x18843F0, saved_network)


def verify_bind_snapshot(g, actor, monster, status, arm, contacts, collide, pulses):
    saved_network = g.get(0x18843F0)
    network, room, process = (g.alloc(size) for size in (1604, 240, 48))
    g.put(0x18843F0, network)
    g.put(network + 520, room)
    g.put(room + 232, 2)
    g.put(network + 992 + 4 * 68, process)
    g.cpu.mem_write(process + 16, b"\1")
    g.put(process + 36, 1)
    g.put(0x185A994, 0x9876)
    member = [1]
    g.cpu.hook_del(g.traps.pop(0x49694))
    g.stub(0x49694, lambda *_: member[0])

    arm()
    g.cpu.mem_write(status + 424, b"\x08")
    g.cpu.mem_write(monster + 29732, b"\1")
    stock = g.alloc(1376)
    equipment = g.get(actor + 5060)
    g.put(actor + 5060, stock)
    transport = g.originals[0x9FC3D8]
    snapshots = []

    def send(sender, recipient, packet, priority):
        assert (sender, recipient, priority, g.get(packet)) == (process, 1, 16, 0x1741604)
        snapshots.append(bytes(g.cpu.mem_read(packet, 48)))
        return 1

    g.stub(transport, send)
    collide(2.)  # Controller sends even when the local hunter uses a GU weapon.
    low, = snapshots
    assert low[9:12] == b"\1\0\x09" and low[16:20] == b"MHS1"
    assert struct.unpack_from("<IHH", low, 24) == (1763, 1, 1)
    g.put(actor + 5060, equipment)
    contacts(1)
    g.put(actor + 5060, stock)
    collide(2.)
    high = snapshots[-1]
    assert len(snapshots) == 2 and struct.unpack_from("<IHH", high, 24) == (1806, 2, 1)

    g.run(g.hooks[INIT_HUNTER], actor)  # Start with a clean observer on the second console.
    member[0] = 0
    g.cpu.mem_write(monster + 29728, b"\0")
    g.run(0xB161C, monster, 1)
    peer = g.get(g.get(0x1885B58) + 32)
    peer_equipment = g.get(peer + 5060)
    g.put(peer + 5060, stock)
    packet = g.alloc(48)

    def receive(sample, source=1):
        g.cpu.mem_write(packet, sample)
        assert g.run(0x1F51F4, process, packet, 1, source) == 1

    # Replay exact boundary values via the authoritative GU monster packet.
    # Even a higher resistance level received from an older peer uses the cap.
    quest = g.get(g.get(0x188AD90) + 164)
    old_rank = bytes(g.cpu.mem_read(quest + 44, 1))
    probe = bytearray(high)
    sequence = 3
    for rank in (1, 2, 3, 4):
        g.cpu.mem_write(quest + 44, bytes([rank]))
        for tier, threshold in enumerate((1800, 3420, 6498, 12346, 23457, 23457)):
            before = len(pulses)
            for value, count in ((threshold - 1, before), (threshold, before + 6)):
                struct.pack_into("<IH", probe, 24, (tier << 16) | value, sequence)
                sequence += 1
                receive(bytes(probe))
                collide(60.)
                assert len(pulses) == count, (rank, tier, value)
    g.cpu.mem_write(quest + 44, old_rank)
    g.run(g.hooks[INIT_HUNTER], actor)

    baseline = len(pulses)
    wrong_spawn = bytearray(high)
    wrong_spawn[14] ^= 1
    wrong_owner = bytearray(high)
    wrong_owner[9] = 2
    receive(bytes(wrong_spawn))
    receive(bytes(wrong_owner), 2)
    receive(high, 2)
    receive(low)
    collide(2.)
    assert len(pulses) == baseline
    receive(high)
    collide(2.)
    assert len(pulses) == baseline + 6
    receive(low)  # Delayed packet must not undo readiness.
    collide(22.)
    assert len(pulses) == baseline + 12

    spent = bytearray(high)
    struct.pack_into("<IH", spent, 24, 0 | (1 << 16), 3)
    receive(bytes(spent))
    receive(high)
    collide(22.)
    assert len(pulses) == baseline + 12

    g.run(0xB161C, monster, 0)
    collide(2.)  # The new controller republishes the inherited meter and resistance.
    assert len(snapshots) == 3 and snapshots[-1][9] == 0
    assert struct.unpack_from("<I", snapshots[-1], 24)[0] == 1 << 16
    assert len(pulses) == baseline + 12
    g.cpu.hook_del(g.traps.pop(transport))
    g.cpu.mem_write(status + 424, b"\0")
    g.put(0x18843F0, saved_network)
    g.put(actor + 5060, equipment)
    g.put(peer + 5060, peer_equipment)
    return high


def verify_bind_exchange(g, actor, monster, status, collision, snapshot, landing, collide, panel,
                         set_action, tick, caster_effects, orbit):
    network, room, actions, unique = (g.alloc(size) for size in (1604, 240, 48, 48))
    saved_network = g.get(0x18843F0)
    g.put(0x18843F0, network)
    g.put(network + 520, room)
    g.put(room + 232, 2)
    g.cpu.mem_write(network + 1598, b"\1")
    for process, procedure in ((actions, 60), (unique, 68)):
        g.put(network + 992 + 4 * procedure, process)
        g.cpu.mem_write(process + 16, b"\1")
        g.put(process + 36, 1)

    member = [0]
    for address in (0x49694, 0x3F8398):
        g.cpu.hook_del(g.traps.pop(address))
        g.stub(address, lambda *_: member[0])
    g.cpu.mem_write(status + 424, b"\x08")
    g.run(0xB161C, monster, 1)
    g.put(0x185A794, 0x9877)
    g.put(0x185A994, 0x9876)
    manager = g.get(0x1885B58)
    controller = g.get(manager + 32)
    successor = g.get(manager + 36)
    equipment, controller_equipment = g.get(actor + 5060), g.get(controller + 5060)
    successor_equipment = g.get(successor + 5060)
    g.put(controller + 5060, g.alloc(1376))
    g.put(successor + 5060, g.alloc(1376))
    transport = g.originals[0x9FC3D8]
    commands, monster_actions, snapshots = [], [], []

    def send(process, dst, packet, priority):
        assert (dst, priority) == (1, 16)
        if process == actions:
            assert g.get(packet) == 0x1741484
            monster_actions.append(bytes(g.cpu.mem_read(packet, 204)))
        else:
            assert process == unique and g.get(packet) == 0x1741604
            data = bytes(g.cpu.mem_read(packet, 48))
            assert data[16:20] == (b"MHE1" if data[33] else b"MHS1")
            (commands if data[33] else snapshots).append(data)
        return 1

    g.stub(transport, send)
    wire = g.alloc(204)

    def receive(data, process=unique):
        g.cpu.mem_write(wire, data)
        proc = 0x1F51F4 if process == unique else 0x1F4554
        assert g.run(proc, process, wire, 1, data[9]) == 1

    def stage_caster():
        g.run(g.hooks[INIT_HUNTER], actor)
        set_action(1, 0)
        tick()
        receive(snapshot)
        landing()
        collide(2.)
        panel(None)

    stage_caster()
    request, = commands
    assert request[9] == 0 and request[33:35] == b"\1\1"
    assert struct.unpack_from("<3f", request, 36) == (0., 30., 40.)
    assert not monster_actions
    before_caster = len(caster_effects)

    # Replay the same request on the controller, whose local hunter has a GU
    # weapon. The request can arrive before the caster's native landing action.
    member[0] = 1
    g.run(0xB161C, monster, 1)
    profile = g.alloc(1472)
    g.cpu.mem_write(profile + 8, bytes(g.cpu.mem_read(equipment, 1376)))
    g.put(profile + 1400, 0x98000000 | 0x1001 << 1)
    g.put(actor + 5060, manager + 7004)
    g.run(0x202894, g.alloc(64), profile, 0)
    set_action(WEAPON, 0x8000 + 99)
    far = bytearray(request)
    struct.pack_into("<H", far, 28, 0)
    struct.pack_into("<3f", far, 36, 0., 30., 2000.)
    receive(bytes(far))
    collide(2.)
    assert not monster_actions
    set_action(WEAPON, 0x8000 + 97)
    wrong_recipient = bytearray(request)
    wrong_recipient[34] = 0
    receive(bytes(wrong_recipient))
    collide(2.)
    receive(request)
    collide(2.)
    assert not monster_actions
    set_action(WEAPON, 0x8000 + 99)
    collide(2.)
    start, = monster_actions
    assert struct.unpack_from("<I", start, 64)[0] >> 20 == 960
    confirm = commands[-1]
    assert confirm[9] == 1 and confirm[33:35] == b"\2\0"
    assert confirm[28:30] == request[28:30]
    hold_started = snapshots[-1]
    hold_id = (1 << 18) | struct.unpack_from("<H", request, 28)[0]
    assert struct.unpack_from("<If", hold_started, 36) == (hold_id, 0.)
    remaining = struct.pack("<f", 900.)
    g.cpu.mem_write(status + 24076, remaining)
    receive(request)
    collide(64.)
    assert len(monster_actions) == 1 and g.cpu.mem_read(status + 24076, 4) == remaining
    hold_aged = snapshots[-1]
    assert struct.unpack_from("<If", hold_aged, 36) == (hold_id, 64.)

    g.run(0xB161C, monster, 2)
    collide(2.)
    assert snapshots[-1] == hold_aged
    assert g.cpu.mem_read(status + 24076, 4) == remaining
    g.run(0xB161C, monster, 1)

    end = bytearray(request)
    end[33] = 3
    end[36:] = bytes(12)
    wrong_serial = bytearray(end)
    wrong_serial[28] ^= 128
    receive(bytes(wrong_serial))
    collide(2.)
    assert g.cpu.mem_read(status + 24076, 4) == remaining
    receive(bytes(end))
    collide(2.)
    assert g.get(status + 24076) == 0
    assert struct.unpack_from("<I", snapshots[-1], 24)[0] == 1 << 16
    assert len(caster_effects) == before_caster
    g.run(0x76AFC, monster, 0, 0)
    _, stop = monster_actions
    receive(request)
    collide(2.)
    assert len(monster_actions) == 2  # Retried requests cannot start another hold.

    # Deliver the controller's confirmation before its native action to a
    # clean caster. The contest opens only after both packets arrive.
    member[0] = 0
    g.run(0xB161C, monster, 1)
    g.put(actor + 5060, equipment)
    stage_caster()
    assert commands[-1] == request, (commands[-1].hex(), request.hex())
    receive(confirm)
    collide(2.)
    panel(None)
    orbit(0x1357, toward=(200., 30., -40.))
    assert len(caster_effects) == before_caster
    receive(start, actions)
    receive(hold_started)
    collide(2.)
    panel("磁缚　转动左摇杆，连按 X 或 A")
    orbit(0x1357, toward=(200., 30., -40.))
    assert [layer for layer, _, _ in caster_effects[before_caster:]] == list(range(545, 555))
    set_action(WEAPON, 0x8000 + 99)
    orbit(0xC000, toward=(200., 30., -40.))
    g.run(0xB161C, monster, 2)
    collide(12.)
    panel("磁缚　转动左摇杆，连按 X 或 A", 10)
    orbit(0xC000, toward=(200., 30., -40.))
    assert len(caster_effects) == before_caster + 10
    set_action(1, 0)
    tick()
    collide(2.)
    panel(None)
    end[34] = 2
    redirected_end = bytes(end)
    assert commands[-1] == redirected_end
    assert g.get(status + 24076) != 0  # The caster's handle cannot release the replica.
    g.run(0xB161C, monster, 1)
    receive(stop, actions)
    collide(2.)
    assert g.get(status + 24076) == 0

    landing()
    collide(2.)
    late_request = commands[-1]
    assert late_request[33] == 1 and late_request[28:30] != request[28:30]
    receive(start, actions)
    receive(confirm)
    collide(2.)
    panel(None)
    collide(120.)
    panel(None)
    assert commands[-1][33] == 3 and commands[-1][28:30] == late_request[28:30]
    receive(stop, actions)

    # Replay on the third console. A recent controller snapshot associates
    # the already-running native action with its original caster.
    member[0] = 2
    g.put(actor + 5060, manager + 7004)
    set_action(WEAPON, 0x8000 + 99)
    for cancel_before_update in (False, True):
        g.run(g.hooks[INIT_HUNTER], successor)
        g.run(0xB161C, monster, 1)
        receive(start, actions)
        receive(hold_aged)
        g.cpu.mem_write(status + 24076, remaining)
        g.run(0xB161C, monster, 2)
        actions_before = len(monster_actions)
        if not cancel_before_update:
            collide(2.)
            assert len(monster_actions) == actions_before
            assert g.cpu.mem_read(status + 24076, 4) == remaining
            assert snapshots[-1][9] == 2
            assert struct.unpack_from("<If", snapshots[-1], 36) == (hold_id, 66.)
        receive(redirected_end)
        collide(2.)
        assert len(monster_actions) == actions_before
        assert g.get(status + 24076) == 0
        assert snapshots[-1][9] == 2
        assert struct.unpack_from("<I", snapshots[-1], 24)[0] == 1 << 16
        assert snapshots[-1][36:] == bytes(12)
        receive(redirected_end)
        collide(2.)
        assert struct.unpack_from("<I", snapshots[-1], 24)[0] == 1 << 16
        g.run(0x76AFC, monster, 0, 0)

    def replay_finisher(state, contact_frame):
        set_action(WEAPON, 0x8000 + state)
        packet = capture_action(g, actor)
        g.put(actor + 5060, manager + 7004)
        g.cpu.mem_write(wire, packet)
        g.run(0x1FF308, wire, actor)
        blades = [actor + offset for offset in (12320, 12504)]
        for blade in blades:
            g.run(0x1687A8, blade)
        serial = None
        for frame in (0., 163., 164., 183., 184., 184.):
            g.cpu.mem_write(actor + 1280, struct.pack("<f", frame))
            g.cpu.mem_write(actor + 1276, struct.pack("<f", frame + 2.))
            g.cpu.mem_write(actor + 1206, b"\0")
            g.run(WEAPON_DISPATCH, actor)
            assert g.half(actor + 1204) == 1488
            hits = [blade for blade in blades if g.half(blade + 56) == 600]
            assert len(hits) == int(frame >= contact_frame), (frame, hits)
            if hits:
                strike, = hits
                if serial is None:
                    serial = g.half(strike + 52)
                assert g.half(strike + 52) == serial
        return strike

    # A replica retains the ordinary frame. The same monster's new controller
    # uses event 44, then skips the duplicate event 43 on this console.
    g.run(g.hooks[INIT_HUNTER], successor)
    g.run(0xB161C, monster, 1)
    receive(start, actions)
    receive(hold_aged)
    g.cpu.mem_write(status + 24076, remaining)
    replay_finisher(105, 184.)
    g.run(0xB161C, monster, 2)
    collide(2.)
    assert struct.unpack_from("<If", snapshots[-1], 36) == (hold_id, 66.), (
        snapshots[-1], g.get(status + 24076), g.half(monster + 29664),
    )
    strike = replay_finisher(157, 164.)
    for _ in range(3):
        g.run(0x16919C, strike)
    g.put(collision + 10288, 0)
    assert g.run(0x1696FC, strike) == 1
    xyz = g.cpu.mem_read(collision + 8248, 12)
    g.run(0x9D67C, monster)
    g.cpu.mem_write(collision + 28, struct.pack("<B3xf", 1, 2.) + bytes(xyz))
    g.run(0x16C0C0, collision, strike, monster)
    assert g.half(status + 1160) > 0
    g.run(0x97DDC, monster)
    assert g.half(monster + 29664) == 0xA70A and g.half(status + 942) == 33

    g.cpu.hook_del(g.traps.pop(transport))
    g.cpu.mem_write(status + 424, b"\0")
    g.cpu.mem_write(monster + 29664, b"\0\0")
    member[0] = 0
    g.run(0xB161C, monster, 0)
    g.put(0x18843F0, saved_network)
    g.put(actor + 5060, equipment)
    g.put(controller + 5060, controller_equipment)
    g.put(successor + 5060, successor_equipment)


def verify_strike_spawn(g, actor, monster, status, hurt, set_action, tick):
    events = []
    callback = g.alloc(4)

    def contact(owner, power, affinity, target):
        assert owner == actor and power & 0xffff == 43 and affinity == 0x3f800000
        spawn = struct.unpack("<HHB", g.cpu.mem_read(target, 5)) if target else None
        events.append(spawn)

    g.stub(callback, contact)
    api = api_for(g, b"mhgu::IndependentMelee", b"IMEL")
    assert g.run(g.get(api + 4), WEAPON, callback) == 1
    for hit in (actor + 12320, actor + 12504):
        g.run(0x1687A8, hit)
    set_action(2, 0)
    g.cpu.mem_write(actor + 10057, b"\0")
    tick(X, native_pad=True)
    tick(frame=60., ended=True)
    tick(frame=40.)
    hit = next(actor + at for at in (12320, 12504) if g.half(actor + at + 56) == 43)

    g.cpu.mem_write(monster + 46576, struct.pack("<H", 30))
    for address in (monster + 46585, monster + 30290, status + 416):
        g.cpu.mem_write(address, b"\1")
    g.cpu.mem_write(status + 438, struct.pack("<H", 123))
    g.put(status + 880, 1000)
    factor = g.alloc(4)
    g.put(hit + 144, 0)
    for revision, area in ((9, 0), (10, 0), (10, 1)):
        g.cpu.mem_write(status + 23744, bytes([revision]))
        g.cpu.mem_write(monster + 4180, bytes([area]))
        g.put(factor, 0x3f800000)
        g.run(g.hooks[0x17885C], 0, hit, monster, hurt, actor, factor)
    assert events == [(30, 123, 9), (30, 123, 10), None], events


def verify_guard(g, actor, set_action, tick, meter, motions, sounds, collision):
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
        if parent and (selector < 105 or 1208 <= selector < 1230 or 304 <= selector < 314):
            joint = 0xffff if 1208 <= selector < 1230 or 304 <= selector < 314 else 8
            assert g.half(unit + 194) == joint and g.get(unit + 132) & 0x80, selector
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
        assert tick(X | A if sheathed else ZR, held=R if sheathed else 0,
                    native_pad=True, fallback=1 << 14)
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
    assert state() == 0x8000 + 14 and motions[-1] == (1034, 0., 14.), (state(), motions[-1])
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
    assert any(g.half(actor + slot + 56) == 30 for slot in (12320, 12504))

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
        counter_fx, counter_audio = len(effects), len(sounds)
        at(32.)
        assert [entry[0] for entry in effects[counter_fx:]] == [*range(1208, 1230), *range(304, 314)]
        assert all(entry[1:] == (parent, (30., 140., 250.)) for entry in effects[counter_fx:])
        assert [(b, cue) for _, b, cue, _ in sounds[counter_audio:]] == [("blade", 14), ("blade", 5)]
        at(32.)
        assert len(effects) == counter_fx + 32 and len(sounds) == counter_audio + 2
        # Independent family-121 counter, through GU's linked list, clock and HDP expansion.
        pass_entry = g.originals[COLLISION_PASS]
        if pass_entry not in g.traps:
            g.stub(pass_entry, lambda *_: 3)
            restore = True
        else:
            restore = False
        clock = g.get(0x2126764) + 104
        saved_clock = bytes(g.cpu.mem_read(clock, 4))
        g.cpu.mem_write(clock, struct.pack("<f", 2.))
        origin = struct.unpack("<3f", g.cpu.mem_read(actor + 64, 12))
        yaw = g.half(actor + 4076) * math.tau / 65536
        power = {57:80, 76:95, 77:120}[follow_up]
        active = []
        for cycle in range(19):
            hits = shock_contacts(g, collision)
            for hit, (kind, radius, x, y, z, x2, y2, z2) in hits.items():
                if g.half(hit + 56) != power:
                    continue
                reach = 100 + 40 * cycle
                assert kind == 2 and radius == 100 and abs(y - y2) == 400
                assert math.dist((x,z),(origin[0]+math.sin(yaw)*reach,origin[2]+math.cos(yaw)*reach)) < .02
                assert (x,z) == (x2,z2)
                active.append(cycle)
        expected = [t for t, live in enumerate(vita_contact_clock(REPO / "mhfz/eboot.bin.elf", 4, 30)) if live]
        assert active == expected, (follow_up, active, expected)
        g.cpu.mem_write(clock, saved_clock)
        if restore:
            g.cpu.hook_del(g.traps.pop(pass_entry))
    slash_counter = capture_action(g, actor)

    for skill, closes in ((169, 28.), (168, 30.)):
        start()
        skills.add(skill)
        probe(closes - 1.)
        probe(closes, accepted=0)
        skills.clear()

    start(sheathed=True)
    assert state() == 0x8000 + 112 and motions[-1] == (1068, 0., 4.)
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
    tick(frame=170., ended=True, held=ZR)
    assert state() == 0
    tick(held=ZR)
    assert state() == 0  # Holding ZR across recovery does not start another guard.
    tick()
    tick(ZR)
    assert state() == 0x8000 + 112
    return (slash_counter, impact_assault, impact_guard), effects


def verify_shot(g, actor, monster, status, monster_vt, hurt, collision, credit, set_action, tick, meter, sounds):
    def put_vec(address, *vec):
        g.cpu.mem_write(address, struct.pack(f"<{len(vec)}f", *vec))

    def stamina_state():
        return tuple(struct.unpack("<f", g.cpu.mem_read(actor + offset, 4))[0]
                     for offset in (5088, 10500, 10768))

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
    g.stub(0x3288C4, particle_allocator(g))
    g.stub(0x328900, lambda *_: 0)
    layers = {}
    aimed_units = set()
    repel_effects = []
    get_parent = g.alloc(4)
    g.put(g.get(actor) + 304, get_parent)
    g.stub(get_parent, lambda owner, *_: owner + 600)

    def spawn(_, unit, handle_id, group):
        assert handle_id == group == 1
        selector = g.get(g.cpu.reg_read(UC_ARM_REG_SP))
        if selector >= 146:
            if 1104 <= selector < 1112:
                aimed_units.add(unit)
            return 1
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
        if unit in aimed_units:
            aimed_units.remove(unit)
        else:
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
    held = ZL
    set_action(1, 0)
    first_sound = len(sounds)
    tick(A, held=held, native_pad=True)
    assert g.half(actor + 9482) == 0x8000 + 84
    put_vec(camera + 60, 90.)
    sight, = [pos for _, frame, pos in meter(artwork=True) if frame == 507]
    assert sight == (616., 336.), sight
    shot_packet = capture_action(g, actor)
    put_vec(camera + 96, 0., 0., -1000.)
    assert any(frame == 507 for _, frame, _ in meter(artwork=True))
    put_vec(camera + 96, 1000., 0., 0.)
    tick(frame=2., next_frame=4.)
    tick(frame=6., next_frame=8.)  # Rotating before the cue cannot redirect the shot saved on entry.
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
    def aimed_frames():
        original = g.get(actor + 5116)
        g.put(actor + 5116, original | ZL)
        frames = [frame for _, frame, _ in meter(artwork=True)]
        g.put(actor + 5116, original)
        return frames
    assert meter()[1] == 1 and 547 in aimed_frames()
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
    assert meter()[1] == 2 and 548 in aimed_frames()
    follow = (100., 95., hit_point[2] + 1050.)
    assert visuals() == [(i, follow) for i in range(8, 13)]
    # The player leaves first, then the same monster joins the new area.
    g.cpu.mem_write(actor + 4180, bytes([1]))
    update()
    assert meter()[1] == 0 and 507 in aimed_frames() and not visuals()
    g.cpu.mem_write(monster + 4180, bytes([1]))
    update()
    assert meter()[1] == 2 and visuals() == [(i, follow) for i in range(8, 13)]
    g.cpu.mem_write(actor + 4180, bytes([0]))
    g.cpu.mem_write(monster + 4180, bytes([0]))
    update()
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

    def aerial_x(frame):
        put_vec(actor + 1280, frame)
        pad.poll(X)
        pad.dispatch()
        result = state()  # Must enter the attack on this poll, before release.
        pad.poll(0)
        return result

    def airborne_contact(contact, power):
        assert g.half(contact + 56) == power
        assert g.cpu.mem_read(actor + 10057, 1)[0] & 0x40
        saved = bytes(g.cpu.mem_read(collision + 28, 64))
        hp = g.get(status + 880)
        before = len(credit)
        g.cpu.mem_write(status + 1160, bytes(2))  # Start a fresh monster damage tick.
        g.put(collision + 10288, 0)
        assert g.run(0x1696FC, contact) >= 1
        # Put a hurt sphere on the first live weapon volume. Use GU's
        # normal intersection, damage application and player attribution.
        point = bytes(g.cpu.mem_read(collision + 8248, 12))
        g.cpu.mem_write(collision + 28, struct.pack("<B3xf", 1, 2.) + point)
        g.run(0x16C0C0, collision, contact, monster)
        damage = g.half(status + 1160)
        assert damage > 0 and credit[before:] == [(0, 0, damage)], (power, damage, credit[before:])
        g.run(0x97DDC, monster)
        assert g.get(status + 880) == hp - damage
        g.cpu.mem_write(collision + 28, saved)

    def attract():
        set_action(1, 0)
        assert tick(X, held=held, native_pad=True, fallback=1 << 14)
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

    def repel(extra_buttons=0):
        set_action(1, 0)
        assert tick(B | extra_buttons, held=held, native_pad=True, fallback=1 << 14)
        return state()

    put_vec(actor + 11072, 0.)
    assert repel(A) == 0  # A failed neutral repel still owns the sample over shot.
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
    from verify_inputs import NativePad, native_ground_table
    pad = NativePad(g, actor)
    set_action(1, 0)
    ground_rows = native_ground_table(g)
    g.put(actor + 9384, ground_rows)
    for keys, expected in ((B, 0), (ZL | B | A | X, 0x8000 + 66)):
        assert pad.poll(keys) & (1 << 3) == 0
        g.run(0x2E9D54, actor)
        pad.dispatch()
        assert state() == expected and g.half(actor + 9478) == (16 if expected else 1)
    assert g.half(actor + 1204) == 1429
    # The rest of this replay injects resolved controller samples directly.
    set_action(4, 14)
    pad.poll(0)
    set_action(16, 0x8000 + 66)
    if g.get(actor + 9384) == ground_rows:
        g.put(actor + 9384, 0)
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
    apply_root_motion(g, actor, 1429, 0, 45)
    tick(X, held=held, frame=45., native_pad=True)
    assert state() == 0x8000 + 66
    tick(frame=59., native_pad=True)
    assert state() == 0x8000 + 66
    apply_root_motion(g, actor, 1429, 45, 60)
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
    skills = {163}
    g.cpu.hook_del(g.traps.pop(0x2864C4))
    g.stub(0x2864C4, lambda _, skill, *__: int(skill in skills))
    g.cpu.hook_del(g.traps.pop(0x65FA04))
    g.stub(0x65FA04, lambda *_: 0x400)  # GU's exchanged shoulder layout.
    put_vec(actor + 11072, 0.)
    tick(B, held=ZL, frame=61., native_pad=True)
    assert state() == 0x8000 + 84
    tick(B, held=ZL, frame=62., native_pad=True)
    assert state() == 0x8000 + 67 and g.half(actor + 1204) == 1444
    g.cpu.hook_del(g.traps.pop(0x65FA04))
    g.stub(0x65FA04, lambda *_: ZR)
    assert g.get(actor + 11072) == struct.unpack("<I", struct.pack("<f", 48.))[0]
    check_repel_effect((0., 0., 170.), 100., 0.8)
    apply_root_motion(g, actor, 1444, 0, 40)
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
        tick(X | A, held=R, native_pad=True)
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
    put_vec(actor + 10500, 100.)
    put_vec(actor + 10768, 140.)
    assert attract() == 0 and stamina_state() == (0., 100., 140.)
    put_vec(actor + 10500, 0.)
    put_vec(actor + 10768, 0.)

    def takeoff_accent():
        pass_entry = g.originals[COLLISION_PASS]
        restore = pass_entry not in g.traps
        if restore:
            g.stub(pass_entry, lambda *_: 3)
        put_vec(clock, 2.)
        # Request 18/19 is already running while the initial pose is blending.
        start = len(sounds)
        shock_contacts(g, collision)
        assert not any(bank == "blade" and cue == 13 for _, bank, cue, _ in sounds[start:])
        shock_contacts(g, collision)
        shock_contacts(g, collision)
        assert [cue for owner, bank, cue, _ in sounds[start:]
                if owner == actor and bank == "blade" and cue == 13] == [13]
        if restore:
            g.cpu.hook_del(g.traps.pop(pass_entry))

    put_vec(actor + 5088, 1.)
    assert attract() == 0x8000 + 58 and g.half(actor + 1204) == 1472
    takeoff_accent()
    tick(frame=33., native_pad=True)
    assert position(actor + 64) == (0., 0., 0.) and configure[-1] == 0
    first_launch = len(sounds)
    tick(frame=34., next_frame=36.)
    assert position(actor + 64) == (0., 0., 0.) and configure[-1] == 2
    tick(frame=36., next_frame=38.)
    assert position(actor + 64) == (0., 0., 0.)
    tick(frame=38., next_frame=40.)
    assert [cue for _, bank, cue, _ in sounds[first_launch:] if bank == "blade"] == [15, 11, 21]
    first = position(actor + 64)
    assert first[1] > 0. and first[2] > 0. and configure[-1] == 2, first
    assert stamina_state() == (0., 0., 0.)
    put_vec(actor + 5088, 1000.)
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
    assert aerial_x(50.) == 0x8000 + 59  # First eligible physical poll.
    assert (state(), g.half(actor + 1204)) == (0x8000 + 59, 1016), (state(), g.half(actor + 1204))
    put_vec(actor + 64, steered[0], 1000., steered[2])
    rate = 0.5
    tick(frame=1.)
    assert position(actor + 64) == (steered[0], 1007.5, steered[2])
    rate = 0.
    tick(frame=1.)
    assert position(actor + 64)[1] == 1007.5
    rate = 0.5
    tick(frame=2.)
    rate = 1.
    tick(frame=4.)
    y = position(actor + 64)[1]
    assert math.isclose(y, 1015. + 15. * 0.97 - 0.4, abs_tol=0.001)
    tick(frame=54.)
    power = [g.half(contact + 56) for contact in contacts]
    assert 200 in power, power  # FZ melee record 16.
    airborne_contact(contacts[power.index(200)], 200)
    assert math.isclose(position(actor + 64)[1], y - 80., abs_tol=0.001)
    tick(frame=56.)
    assert math.isclose(position(actor + 64)[1], y - 80. - 80. * 0.97 - 6., abs_tol=0.001)
    put_vec(actor + 64, steered[0], 0.25, steered[2])
    tick(frame=58.)
    assert state() == 0x8000 + 60 and g.half(actor + 1204) == 1015
    assert position(actor + 64)[1] == 0. and configure[-1] == 0
    set_action(4, 14)
    pad.poll(0)

    put_vec(actor + 64, 0., 0., 0.)
    set_action(1, 0)
    tick(SPECIAL, native_pad=True)
    tick(frame=99., ended=True)
    mark_at(2250., 12.)
    assert attract() == 0x8000 + 68 and g.half(actor + 1204) == 1460
    takeoff_accent()
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
    tick(A | X, held=held, frame=26., native_pad=True)  # Shot is closed, attraction can buffer.
    assert state() == 0x8000 + 84
    tick(frame=62., native_pad=True)
    assert state() == 0x8000 + 68
    tick(frame=34., native_pad=True)
    tick(frame=36.)
    tick(frame=38.)
    for frame in range(40, 150, 2):
        tick(frame=min(frame, 85.))
        if g.half(actor + 1204) == 1461:
            break
    assert g.half(actor + 1204) == 1461
    for frame in (2., 4., 6.):
        tick(frame=frame)
    assert math.dist(position(actor + 64), position(pose + 48)) < 300.
    assert aerial_x(8.) == 0x8000 + 73
    assert state() == 0x8000 + 73 and g.half(actor + 1204) == 1017
    airborne_contact(contacts[0], 25)
    x, _, z = position(actor + 64)
    put_vec(actor + 64, x, 1000., z)
    y = 1000.
    for frame in range(2, 50, 2):
        if 4 <= frame < 40:
            y += 5. * 0.97 ** ((frame - 4) // 2)
        elif frame >= 40:
            y += 5. if frame < 44 else -30.
        if 10 <= frame < 42:
            y -= 0.4 * ((frame - 10) // 2)
        elif frame >= 42:
            y -= 5. * ((frame - 42) // 2)
        tick(frame=frame)
        assert state() == 0x8000 + 73 and math.isclose(position(actor + 64)[1], y, abs_tol=0.001), frame
    assert g.half(contacts[1] + 56) == 80
    # FZ waits for tick 26, then checks contact 164 units below the root.
    put_vec(actor + 64, x, 160., z)
    tick(frame=50.)
    assert state() == 0x8000 + 73 and position(actor + 64)[1] == 110.
    tick(frame=52.)
    assert state() == 0x8000 + 74 and g.half(actor + 1204) == 1014
    assert position(actor + 64)[1] == 0.
    g.run(0x1687A8, contacts[0])
    tick(frame=8.)
    assert g.half(contacts[0] + 56) == 120
    set_action(4, 14)
    pad.poll(0)

    # Plain B can branch during attraction without entering GU's ground roll.
    # The attraction starts with event 45. The emergency landing's event 11
    # charges the normal evade amount once. Run both through the actual GU
    # cost code, with the item expiring first and the melody still active.
    spent, expiry = [], []
    stock_cue = g.get(vtable + 940)
    cue = g.alloc(4)
    g.put(vtable + 940, cue)
    g.stub(cue, lambda owner, category, index, _: expiry.append((owner, category, index)))
    stock_quest = g.get(0x188AD90)
    g.put(0x188AD90, g.alloc(164))
    put_vec(actor + 5088, 1000., 1000.)
    put_vec(actor + 10500, 100.)
    put_vec(actor + 10768, 140.)
    put_vec(actor + 10884, 700.)
    g.cpu.mem_write(actor + 10721, b"\x03")
    g.cpu.mem_write(actor + 10985, b"\x02")

    def stamina(cpu, *_):
        spent.append(struct.unpack("<f", struct.pack("<I", cpu.reg_read(UC_ARM_REG_S0)))[0])

    watch_cost = g.cpu.hook_add(UC_HOOK_CODE, stamina, begin=0x2A5EBC, end=0x2A5EBC)
    put_vec(actor + 64, 0., 0., 0.)
    set_action(1, 0)
    tick(SPECIAL, native_pad=True)
    tick(frame=99., ended=True)
    mark_at(2250., 14.)
    assert attract() == 0x8000 + 58
    tick(frame=34., native_pad=True)
    tick(frame=36.)
    tick(frame=38.)
    assert spent == [-60.] and stamina_state() == (1000., 0., 20.) and not expiry
    assert g.cpu.mem_read(actor + 10721, 1) == b"\x03"
    tick(common=1 << 3, frame=50., native_pad=True)
    assert state() == 0x8000 + 63 and g.half(actor + 1204) == 1407

    def emergency_movement(first_step, second_step):
        put_vec(actor + 64, 0., 300., 0.)
        g.cpu.mem_write(actor + 4076, b"\x00\x00")
        tick(frame=2.)
        assert position(actor + 64) == (0., 268.5, first_step)
        tick(frame=4.)
        x, y, z = position(actor + 64)
        assert (x, y) == (0., 234.) and math.isclose(z, second_step, abs_tol=0.001)

    emergency_movement(10., 19.7)
    for frame in range(6, 70, 2):
        tick(frame=frame)
        if state() == 0x8000 + 64:
            break
    assert state() == 0x8000 + 64 and g.half(actor + 1204) == 1408
    assert spent == [-60., -150.] and landings == [(actor, 0)] * 4, (spent, landings)
    assert stamina_state() == (1000., 0., 0.) and expiry == [(actor, 1, 36)]
    assert g.get(actor + 10884) == 0
    assert g.cpu.mem_read(actor + 10721, 1) == g.cpu.mem_read(actor + 10985, 1) == b"\0"
    # Give the additional emergency-landing case one segment without changing
    # the refill phase that the charge test below expects.
    put_vec(clock, 600.)
    update()
    put_vec(clock, 2.)
    put_vec(actor + 64, 0., 0., 0.)
    set_action(1, 0)
    tick(SPECIAL, native_pad=True)
    tick(frame=99., ended=True)
    mark_at(2250., 12.)
    skills.add(156)  # GU Constitution +2 halves the unprotected discrete costs.
    assert attract() == 0x8000 + 68
    for frame in (34., 36., 38.):
        tick(frame=frame)
    assert stamina_state() == (970., 0., 0.)
    tick(A | B, held=ZL, frame=50., native_pad=True, fallback=1 << 14)
    assert state() == 0x8000 + 69 and g.half(actor + 1204) == 1019
    emergency_movement(5., 10.)
    wall = True
    tick(frame=6.)
    assert position(actor + 64) == (0., 196.5, 10.)
    wall = False
    put_vec(actor + 64, 0., 0.25, 0.)
    tick(frame=8.)
    assert state() == 0x8000 + 70 and position(actor + 64)[1] == 0.
    assert spent == [-60., -150.] * 2 and landings == [(actor, 0)] * 5, (spent, landings)
    assert stamina_state() == (895., 0., 0.) and expiry == [(actor, 1, 36)]
    skills.remove(156)
    g.cpu.hook_del(watch_cost)
    g.put(vtable + 940, stock_cue)
    g.put(0x188AD90, stock_quest)
    set_action(1, 0)
    tick(SPECIAL, native_pad=True)
    tick(frame=99., ended=True)
    for contact in contacts:
        g.run(0x1687A8, contact)
    mark_data = mark_events, repel_packet, pose, repel_effects
    return (shot_packet, events, (spawn, stop, visuals), mark_data), (flight_packet, guided_position, coast_position)


def apply_root_motion(g, actor, clip, start, end):
    """Run GU's root-motion transform using samples from the packaged LMT."""
    assert g.half(actor + 1204) == clip
    bank = assets(PLUGIN.parents[1] / "arc/frontier/magnet_spike.arc")
    lmt = bank[rf"player\mot\frontier_ms\{(clip - 1000) // 100}", 0x76820D81]
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


def setup_magnet_hud(g, sampler, actor):
    snapshot = g.alloc(512)
    layout, widget_list, vital = (g.alloc(n) for n in (688, 4, 76))
    stock, dynamic, sharpness, root = (g.alloc(n) for n in (80, 64, 228, 228))
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
    messages = {}
    message = g.alloc(352)
    g.run(0xA520A4, message)

    def clone(owner, resource, template, parent):
        assert (owner, resource, parent) == (layout, 0xffffffff, 10)
        assert template in (5, 16)
        assert g.get(g.cpu.reg_read(UC_ARM_REG_SP)) == 0x2123290
        node, members, animation = (g.alloc(n) for n in (228, 12, 176))
        g.put(node + 212, template)
        g.put(node + 216, members)
        g.put(node + 220, animation)
        if template == 16:
            g.put(animation, 0x178F9BC)
            g.put(message + 112, animation)
            g.put(animation + 104, message)
            g.put(members + 4, message)
            bars = []
        else:
            bars = [g.alloc(256), g.alloc(256)]
            g.put(animation + 104, bars[0])
            g.put(bars[0] + 108, bars[1])
            for i, bar in enumerate(bars):
                g.put(bar, 0x178FC48)
                g.put(bar + 112, animation)
                g.put(bar + 160, 2)
                g.put(members + 4 * (i + 1), bar)
        index = len(groups)
        assert index < 14, "status groups are reused across repeated binds"
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

    def prepare_text(parser, string, length, *_):
        # The native setter reaches the font parser with the cloned GU message.
        messages[g.get(parser + 16)] = bytes(g.cpu.mem_read(string, length)).decode()
        return 0

    g.stub(0x13EFD7C, lambda address, size, *_: g.cpu.mem_write(address, bytes(size)))
    g.stub(0xA4146C, lambda *_: 0)  # No native font geometry pool in the fixture.
    g.stub(0xA418C4, prepare_text)

    def sample(*, boost_tracks=False, flashes=False, panel=False, artwork=False):
        assert g.run(0x544CC8, layout) == 17
        assert g.get(root + 84) == 3
        images = []
        caption, panel_tracks = None, []
        for node, bars in groups:
            if g.get(node + 84) & 3 != 3:
                continue
            position = struct.unpack("<2f", g.cpu.mem_read(node + 144, 8))
            if not bars:
                assert position == (640., 308.)
                caption = messages[g.get(g.get(node + 216) + 4)]
                continue
            sprite = g.get(bars[1] + 108)
            if sprite:
                assert all(not g.get(bar + 84) & 1 for bar in bars)
                assert g.get(sprite + 84) & 1 and g.get(sprite) == 0x178FCC8
                atlas = g.get(g.get(sprite + 260) + 12) & 0xffff
                x, y, right, bottom = struct.unpack("<4f", g.cpu.mem_read(sprite + 268, 16))
                w = int(right - x)
                frame = int(y / 32) * (atlas // w) + int(x / w)
                if atlas == 72 or frame <= 506 or frame >= 531:
                    # All gauge artwork including the left edge of the spend flash.
                    assert position[0] >= 80. + 96., (frame, position)
                images.append((atlas, frame, position))
            else:
                assert position == (528., 337. + 12. * len(panel_tracks))
                width, height = struct.unpack("<2f", g.cpu.mem_read(bars[0] + 180, 8))
                assert height == 8.
                panel_tracks.append((width, g.get(bars[0] + 164)))
        if artwork:
            return images
        if panel:
            return caption, panel_tracks
        if flashes:
            return [(0 if frame <= 31 else round((pos[0] - 212.4)/19.2), frame)
                    for atlas, frame, pos in images if atlas == 72]
        if g.get(actor + 5060) != g.get(0x1885B58) + 124:
            return ([], 0)
        g.run(sampler, snapshot, actor)
        units, per, segments, signal = struct.unpack("<HHBB", g.cpu.mem_read(snapshot, 6))
        if images:
            assert (3456, units // 100 + 101 * signal, (200., 52.)) in images
        if boost_tracks:
            return [(16. * g.cpu.mem_read(snapshot+8+8*i, 1)[0]/100.,
                     g.get(snapshot+12+8*i)) for i in range(2)]
        return [16.*max(0,min(per, units-i*per))/per for i in range(segments)], signal

    return sample


def frontier_impact_charge():
    from unicorn import arm_const as arm
    from verify_assets import Machine, MEM, STOP

    m = Machine(REPO / "mhfz/eboot.bin.elf", True)
    actor = MEM + 0x4000
    m.put(0x82BB48C0, MEM + 0x1000)
    m.put(MEM + 0x1000 + 2956, MEM + 0x2400)
    m.put(MEM + 0x2400 + 20, MEM + 0x3000)
    blob = (ROOT / "target/assets/magnet_spike.fmsa").read_bytes()
    at = blob.rindex(b"MSPR") + 4
    m.c.mem_write(MEM + 0x3000, blob[at:at + 166])
    transitions = []
    m.trap(0x8121D284, lambda owner, flag, *_: int(flag == 32))
    m.trap(0x812256E4, lambda *_: transitions.append(m.c.mem_read(actor + 2856, 1)[0]))
    for address in (0x8102CD38, 0x818DD2DC, 0x8182B03C, 0x8182B4FE):
        m.trap(address, lambda *_: 0)
    m.c.hook_add(UC_HOOK_CODE, lambda cpu, *_: cpu.reg_write(arm.UC_ARM_REG_PC, STOP | 1),
                 begin=0x81221F62, end=0x81221F62)
    for skill, threshold in ((0, 20), (0x12e, 16), (0x12f, 18), (0x130, 24)):
        m.trap(0x8114690C, lambda owner, test, *_: int(skill == test))
        m.c.mem_write(actor + 5, bytes(1))
        m.c.reg_write(arm.UC_ARM_REG_R6, actor)
        charged = []
        transitions.clear()
        for tick in range(1, threshold + 16):
            m.call(0x81221FC0)
            if m.c.mem_read(actor + 2856, 1)[0]:
                charged.append(tick)
        assert charged == list(range(threshold + 1, threshold + 15)), (skill, charged)
        assert transitions == [0]


def frontier_refill_curve():
    from verify_assets import Machine, MEM

    machine = Machine(REPO / "mhfz/eboot.bin.elf", True)
    hunter, ext = 0x83A28E70, MEM + 0x4000
    machine.c.mem_write(hunter, bytes([1, 0, 0, 13]))
    machine.put(0x82BB48C0, MEM + 0x1000)
    machine.put(MEM + 0x1000 + 2956, MEM + 0x2400)
    machine.put(MEM + 0x2400 + 20, MEM + 0x3000)
    blob = (ROOT / "target/assets/magnet_spike.fmsa").read_bytes()
    offset = blob.rindex(b"MSPR") + 4
    machine.c.mem_write(MEM + 0x3000, blob[offset:offset + 164])
    machine.trap(0x8121D226, lambda actor, *_: ext if actor == hunter else 0)
    machine.trap(0x8182B54E, lambda actor, *_: int(actor == hunter))
    for routine in (0x8182C996, 0x812276D0, 0x8121D3EA):
        machine.trap(routine, lambda *_: 0)
    machine.c.mem_write(ext + 122, struct.pack("<hh", 8000, 0) + bytes([255]) * 6)
    curve = []
    for _ in range(301):
        machine.call(0x8122880E)
        curve.append(struct.unpack("<h", machine.c.mem_read(ext + 122, 2))[0])
    assert curve == [min(10000, 8000 + 200 * (i // 30)) for i in range(1, 302)]
    return curve


def verify_magnetic_charge(g, actor, monster, hurt, collision, set_action, tick, meter):
    source_curve = frontier_refill_curve()
    def charge_attack():
        set_action(1, 0)
        tick(X | A, held=R, native_pad=True)
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
        set_action(2, 0)
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
    # Start the independent regen test at exactly one segment. The airborne
    # damage cases above now legitimately recharge the gauge on contact.
    elapsed(3200.)
    for _ in range(4):
        assert charge_attack() == 0x8000 + 12
        tick(frame=99., ended=True)
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
    g.cpu.mem_write(monster + 4180, bytes([1]))
    g.run(0x17885C, 0, hit, monster, hurt, actor, factor)
    assert meter() == before
    g.cpu.mem_write(monster + 4180, bytes([0]))
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

    # Refill to maximum, then compare the opposite form against the Vita clock.
    elapsed(3000.)
    assert meter()[0] == [16.] * 5
    set_action(1, 0)
    tick(SPECIAL, native_pad=True)
    tick(frame=99., ended=True)
    assert charge_attack() == 0x8000 + 38
    prior = 0
    for source_tick in (1, 29, 30, 59, 60, 299, 300, 301):
        elapsed(2. * (source_tick - prior))
        expected = source_curve[source_tick - 1] * 16. / 2000.
        assert math.isclose(sum(meter()[0]), expected, abs_tol=.001), (source_tick, meter(), expected)
        prior = source_tick


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
        set_action(2, 0)
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
        set_action(2, 0)
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
            tick(R, native_pad=True)
            assert g.half(actor + 9482) == 0
        assert tick(SPECIAL, held=R, native_pad=True,
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
    tick(X | A, held=R, native_pad=True)
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
    tick(X | A, held=R, native_pad=True)
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
    frontier_impact_charge()
    replay()
    print("Magnet Spike: dispatch, HUD, melee, deflection, guard, magnetic shot, steps, charge, boost, flight, bind and peer replay passed")
