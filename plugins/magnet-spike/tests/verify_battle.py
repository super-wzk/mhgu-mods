"""Replay the packaged Magnet Spike through GU's normal hunter dispatcher."""

import json
import struct
import sys

from unicorn.arm_const import UC_ARM_REG_S0, UC_ARM_REG_S1

from verify_magnet_spike import PLUGIN, REPO
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

    def tick(buttons=0, *, common=0, frame=0., ended=False, fallback=0, native_pad=False):
        g.put(actor + 5112, buttons)
        g.put(actor + 5140, common)
        g.put(actor + 5156, fallback)
        g.cpu.mem_write(actor + 1280, struct.pack("<f", frame))
        g.cpu.mem_write(actor + 1206, bytes([4 * ended]))
        if native_pad:
            g.cpu.mem_write(actor + 5108, struct.pack("<3H", buttons, 0, buttons))
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
    res_manager, res_vtable, load = (g.alloc(n) for n in (128, 52, 4))
    g.put(res_manager, res_vtable)
    g.put(res_vtable + 48, load)
    g.put(0x2126A64, res_manager)
    paths = []

    def resource(_, kind, path, flags):
        assert flags == 1
        name = g.string(path)
        paths.append(name)
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
    assert paths == [r"arc\frontier\magnet_spike"] + [rf"player\mot\frontier_ms\{g}" for g in (0, 2, 4)]
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


if __name__ == "__main__":
    replay()
    print("Magnet Spike native dispatch: draw, buffered slash, mode, fallback isolation and part playback passed")
