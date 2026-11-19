"""Exercise both packaged NROs, Forge INI selection and the original GU motion path."""

import math
import os
import struct
import sys
import zlib
from pathlib import Path

from unicorn import UC_HOOK_CODE

ROOT = Path(__file__).resolve().parents[1]
REPO = ROOT.parents[1]
sys.path.insert(0, str(REPO / "plugins/monster-registry/tests"))
from verify_espinas_quest import Game, MANAGER

NX = ROOT / "dist/great-sword-extreme/romfs/nativeNX"
PROVIDER = (
    REPO
    / "plugins/style-registry/dist/style-registry/romfs/nativeNX/plugins/style_registry.nro"
)
CONTENT = NX / "plugins/great_sword_extreme.nro"
CONFIG = "savedata:/style_registry.ini"
ENCODE, DECODE = 0x27C6A0, 0x27C6D4
MENU_OPEN, MENU_UPDATE = 0x548B7C, 0x5490FC
STATUS_STYLE, STATUS_STYLE_SMALL = 0x62FB78, 0x633450
HIT_ROW = 0x2A8A88
DISPATCH, BLOCK = 0x282DA8, 0x2F2BA0
WEAPON_DISPATCH = 0x2B8178
GAME_STYLE = 0x268818
ART_READY = 0x2A26C8
ART_CAPACITY = 0x2778AC
VALOR_HUD = 0x639BD0
FX_RELEASE = 0x32A104
DRAW_ROUTE = 0x115FCF4
DB_DRAW_ROUTE = 0x11BA2A0
EQUIPMENT_GRIP = 0x308834
CHANGE_ACTION, GS_CHANGE_ACTION = 0x2AB670, 0x1160650
COMMAND_BIND, COMMAND_CONDITION = 0x2E9B7C, 0x2ECEFC
COMMAND_UPDATE = 0x2E9D54
SHARPNESS = 0x2AD288
HIT_EXPAND, MODEL_DEPTH = 0x1696FC, 0xA957CC
GUARD_REACTION, DRAW_MODEL = 0x2F4B20, 0xA956D8


def effect_routes():
    data = (ROOT / "target/assets/effects/routes.bin").read_bytes()
    return [range(first, first + count) for first, count in struct.iter_unpack("<HH", data)]


def api_for(g, identifier, magic):
    name = g.alloc(32)
    g.cpu.mem_write(name, identifier + b"\0")
    dt = g.run(0x7AEF68, g.run(0x7CBDB4, name, 0xFFFFFFFF) & 0x7FFFFFFF)
    assert dt and g.get(dt + 32) == int.from_bytes(magic, "little")
    return g.get(dt + 36)


def service(g):
    api = api_for(g, b"mhgu::StyleRegistryV1", b"SRG1")
    return g.get(api), g.get(api + 4)

def style_descriptor(g, ident, weapon, encode=0, art_slots=0):
    descriptor = g.alloc(68)
    g.cpu.mem_write(descriptor, struct.pack("<4IBB50x", ident, weapon, encode, 0, 0, art_slots))
    return descriptor


def boot(elf, config, order=("registry", "sword"), mount_ok=True):
    g = Game(elf)
    g.stub(0x13EFDAC, lambda p, size, *_: g.cpu.mem_write(p, bytes(size)))
    g.inline_hooks = g.inline_hooks | {
        ENCODE,
        DECODE,
        GAME_STYLE,
        ART_READY,
        ART_CAPACITY,
        VALOR_HUD,
        CHANGE_ACTION,
        COMMAND_BIND,
        COMMAND_CONDITION,
        GS_CHANGE_ACTION,
        DRAW_ROUTE,
        DB_DRAW_ROUTE,
        FX_RELEASE,
        SHARPNESS,
        HIT_EXPAND,
        MODEL_DEPTH,
        GUARD_REACTION,
        DRAW_MODEL,
        ENCODE,
        DISPATCH,
        WEAPON_DISPATCH,
        HIT_ROW,
    }
    g.originals = {}
    install = g.hook

    def hook(ret, addr, detour, saved):
        result = install(ret, addr, detour, saved)
        g.originals[addr] = g.get(saved)
        return result

    g.hook = hook
    if config is not None:
        g.files[CONFIG] = config

    # Model sequential MtFile writes: Forge's stock INI writer emits a line in parts.
    def append(obj, p, n, _):
        assert g.io_mount and g.opened[obj] == CONFIG
        g.files[CONFIG] += bytes(g.cpu.mem_read(p, n))
        return n

    slot = g.alloc(4)
    g.stub(slot, append)
    g.put(g.files_vt + 12 * 4, slot)
    for name in order:
        path, base = (
            (PROVIDER, 0x9000000) if name == "registry" else (CONTENT, 0x9200000)
        )
        ex = g.load(
            path, base, {} if mount_ok else {"forge_fs_mountSaveData": lambda *_: 0}
        )
        if name == "sword":
            assert "forge_onUpdate" not in ex
        for cb in ["forge_onLoad", "forge_onInit"]:
            g.run(ex[cb], MANAGER)
    assert not g.io_mount and not g.opened
    expected = (
        {
            ENCODE,
            DECODE,
            MENU_OPEN,
            MENU_UPDATE,
            STATUS_STYLE,
            STATUS_STYLE_SMALL,
            GAME_STYLE,
            ART_READY,
            ART_CAPACITY,
            VALOR_HUD,
            DISPATCH,
            WEAPON_DISPATCH,
            HIT_ROW,
            CHANGE_ACTION,
            COMMAND_BIND,
            COMMAND_CONDITION,
            GS_CHANGE_ACTION,
            DRAW_ROUTE,
            DB_DRAW_ROUTE,
            BLOCK,
            SHARPNESS,
            GUARD_REACTION,
            DRAW_MODEL,
        }
        if "registry" in order
        else set()
    )
    if order == ("registry", "sword"):
        expected.update((FX_RELEASE, EQUIPMENT_GRIP, HIT_EXPAND, MODEL_DEPTH))
    assert set(g.hooks) == expected
    if config is not None:
        assert g.files[CONFIG] == config
    return g


def hunters(g):
    actor = g.alloc(14000)
    eq = g.alloc(1500)
    remote = g.alloc(14000)
    mgr = g.alloc(128)
    for a in actor, remote:
        g.put(a, 0x1749A88)
        g.put(a + 5060, eq)
        g.cpu.mem_write(a + 10068, b"\x03")  # no queued Hunter Art
    g.put(0x1885B58, mgr)
    g.put(mgr + 28, actor)
    g.stub(0x3F8398, lambda *_: 0)
    g.cpu.mem_write(eq + 1241, b"\x05")  # native Valor selection is preserved
    return actor, eq, remote


def verify_valor(elf, config):
    from unicorn import arm_const as arm

    g = boot(elf, config)
    hunter, eq, other = hunters(g)
    ui, gauge, layout, nodes, node = [g.alloc(x) for x in (264, 196, 700, 128, 240)]
    for fn in (0x56E3B0, 0x54E320, 0x634030, 0x634124, 0x54533C, 0x56E538, 0xA344F4):
        g.stub(fn, lambda *_: 0)

    def create(_, kind, *__):
        assert kind == 0x18A0268  # cUIOStyleTatsujinGauge
        return gauge

    g.stub(0x56DD44, create)
    g.put(gauge + 36, layout)
    g.put(gauge + 188, 16)
    g.put(gauge + 100, 0x447A0000)  # full scale 1000
    for off in (80, 128):
        g.put(gauge + off, 0xFFFFFFFF)
    g.put(layout + 664, 32)
    g.put(layout + 248, nodes)
    g.put(nodes + 16 * 4, node)
    for off in (432, 1356):
        g.stub(g.get(0x1749A88 + off), lambda *_: 0)
    g.stub(g.get(0x1749A88 + 2184), lambda *_: g.cpu.reg_write(arm.UC_ARM_REG_S0, 0))
    g.stub(0x2864C4, lambda *_: 0)
    hit, target = g.alloc(200), g.alloc(5700)
    g.cpu.mem_write(hit + 168, struct.pack("<h", 30))
    g.put(g.get(0x1885B58) + 32, other)
    for actor, allowed in ((hunter, False), (other, True)):
        g.put(ui + 76, int(actor == other))
        g.run(0x633D1C, ui)
        assert g.get(ui + 220) == (gauge if allowed else 0)
        g.cpu.mem_write(actor + 9478, struct.pack("<3H", 1, 0, 276))
        assert g.run(0x2A24E8, actor) == int(allowed)
        g.put(actor + 10128, 10)
        g.run(0x172474, 0, hit, target, actor)
        assert g.get(actor + 10128) == (40 if allowed else 10)
    # A previously created, even full, gauge must disappear; respect the
    # original HUD's requested visibility when the independent style ends.
    g.put(hunter + 10128, 1000)
    for requested in (0, 2):
        g.cpu.mem_write(eq + 1236, b"\0")
        g.put(node + 84, 3)
        g.run(VALOR_HUD, gauge)
        assert g.run(0x545600, layout, 16) == 0
        g.put(node + 84, requested)
        g.cpu.mem_write(eq + 1236, b"\x01")
        g.run(VALOR_HUD, gauge)
        assert g.run(0x545600, layout, 16) == int(bool(requested))
    assert g.cpu.mem_read(eq + 1241, 1) == b"\x05"
    print(
        "GU Valor: stance eligibility, on-hit gauge gain, HUD creation and existing gauge visibility passed"
    )


def verify_great_sword(g):
    nx = NX
    actor, eq, remote = hunters(g)
    # A retail great sword predicate, including its virtual style lookup.
    for native_style in range(6):
        g.cpu.mem_write(eq + 1241, bytes([native_style]))
        assert (
            g.run(GAME_STYLE, actor) == 0 and g.run(GAME_STYLE, remote) == native_style
        )
        for a in (actor, remote):
            g.cpu.mem_write(a + 13096, b"\x01")
            assert g.run(0x1176610, a) == int(a == remote and native_style == 3)
    g.cpu.mem_write(eq + 1241, b"\x05")
    g.stub(0x2862F4, lambda _, art_id, *__: int(art_id in (4, 5, 6)))
    g.cpu.mem_write(eq + 4 + 572, struct.pack("<3H", 4, 5, 6))
    for hunter in actor, remote:
        g.cpu.mem_write(hunter + 10078, struct.pack("<3H", 100, 100, 100))
        g.cpu.mem_write(hunter + 10072, struct.pack("<3H", 99, 100, 100))
        for slot in range(3):
            assert g.run(ART_READY, hunter, slot) == int(slot != 0)
    assert bytes(g.cpu.mem_read(eq + 576, 6)) == struct.pack("<3H", 4, 5, 6)

    def byte(p, v):
        g.cpu.mem_write(p, bytes([v]))

    # Manager lookup uses the genuine loose filename and verifies a native LMT slot.
    resource_mgr = g.alloc(16)
    vt = g.alloc(64)
    g.put(resource_mgr, vt)
    g.put(0x2126A64, resource_mgr)
    load_fn = g.alloc(4)
    g.put(vt + 48, load_fn)
    resources = {}
    loads = []

    def load(_, kind, name, flags):
        rel = g.string(name)
        loads.append(rel)
        if kind == 0x1889FDC:
            assert flags == 1 and rel == r"effect\frontier_gs\particles"
            b = (nx / "effect/frontier_gs/particles.pel").read_bytes()
            assert b[:12] == b"PEL\0\x0f\0\0\0\0\0\0\0"
            (count,) = struct.unpack_from("<I", b, 12)
            obj, rows = g.alloc(176), g.alloc(4 * count)
            g.put(obj + 80, 1)
            g.put(obj + 124, count)
            g.put(obj + 136, rows)
            at = 16
            for i in range(count):
                n = b[at]
                path = b[at + 1 : at + 1 + n].decode().replace("\\", "/")
                at += n + 1
                assert (
                    b[at] == 0
                    and (nx / (path + ".efl")).is_file()
                )
                body = b[at + 1 : at + 161]
                assert body[61] == 0
                at += 161
                row = g.alloc(192)
                g.cpu.mem_write(row + 32, body)
                g.put(rows + 4 * i, row)
            assert b[at:] == bytes(4)
            return obj
        if kind == 0x2125BEC:
            assert flags == 1
            sr = (nx / (rel.replace("\\", "/") + ".srqr")).read_bytes()
            obj = g.alloc(176)
            rows = g.alloc(len(sr))
            g.cpu.mem_write(rows, sr)
            g.put(obj + 80, 1)
            g.put(obj + 104, g.get(rows + 8))
            g.put(obj + 108, rows + 52)
            resources[rel] = obj
            return obj
        if kind == 0x1857810:
            assert rel in (r"player\hit\frontier_gs\radiance", r"player\hit\frontier_gs\guard_counter") and flags == 1
            counter = rel.endswith("guard_counter")
            data = (nx / (rel.replace("\\", "/") + ".hts")).read_bytes()
            assert struct.unpack_from("<4s5I", data) == (
                b"HTS\0",
                0x20131015,
                1,
                0,
                1,
                24,
            )
            assert struct.unpack_from("<4H10f", data, 24) == (
                255,
                0,
                1,
                0,
                0.0,
                200.0,
                0.0,
                250.0 if counter else 150.0,
                200.0 if counter else 0.0,
                0.0,
                0.0,
                50.0,
                200.0 if counter else 0.0,
                0.0,
            )
            if rel not in resources:
                resources[rel] = g.alloc(112)
                # The real native expander reads this rHitSize group and the
                # packed capsule. Only the resource-manager lookup is mocked.
                shape = g.alloc(len(data))
                g.cpu.mem_write(shape, data)
                group, entries = g.alloc(12), g.alloc(4)
                g.put(group + 4, 1)
                g.put(group + 8, entries)
                g.put(entries, shape + 24)
                vt, get_group = g.alloc(72), g.alloc(4)
                g.put(resources[rel], vt)
                g.put(vt + 68, get_group)
                g.stub(get_group, lambda _, index, *args: group if index == 0 else 0)
            g.put(resources[rel] + 80, 1)
            return resources[rel]
        assert kind == 0x2125210 and flags == 1
        data = (nx / (rel.replace("\\", "/") + ".lmt")).read_bytes()
        slots = struct.unpack_from("<H", data, 6)[0]
        assert slots >= 7
        obj = g.alloc(104)
        hdr = g.alloc(8 + 4 * slots)
        g.cpu.mem_write(hdr, data[: 8 + 4 * slots])
        g.put(obj + 80, 1)
        g.put(obj + 100, hdr)
        g.put(obj + 84, 1)
        for i in range(slots):
            if off := struct.unpack_from("<I", data, 8 + 4 * i)[0]:
                clip = g.alloc(64)
                # The motion setter reads lengths, looping and initial transform.
                # Timeline pointers are exercised separately by verify_assets.
                g.cpu.mem_write(clip + 8, data[off + 8 : off + 48])
                g.put(hdr + 8 + 4 * i, clip)
        resources[rel] = obj
        return obj

    g.stub(load_fn, load)
    # Native setter retains references. Refcount functions depend on the OS.
    g.stub(0x887648, lambda *_: 0)
    g.stub(0x887660, lambda *_: 0)
    encode = lambda a, m: g.run(g.hooks[ENCODE], a, m)
    decode = lambda a, m: g.run(g.hooks[DECODE], a, m)
    assert encode(remote, 5001) == 0x501 and encode(actor, 5101) == 0x565 and not loads
    for i in range(8, 16):
        g.put(actor + 3844 + i * 4, 0x1234)
    assert encode(actor, 5001) == 0x501
    g.put(actor + 3844 + 13 * 4, 0)
    assert encode(actor, 5001) == 0xD01 and decode(actor, 0xD01) == 5001
    assert g.get(actor + 3844 + 13 * 4) == resources[r"player\mot\frontier_gs\m_0"]
    assert g.get(actor + 3844 + 15 * 4) == 0x1234 and decode(remote, 0xD01) == 13001
    assert encode(actor, 5001) == 0xD01 and len(loads) == 1
    byte(eq + 1240, 1)
    g.put(actor + 3844 + 12 * 4, 0)
    assert encode(actor, 5001) == 0xC01 and loads[-1].endswith("f_0")
    g.put(actor + 3844 + 11 * 4, 0)
    load_hit_data(g, actor, remote, encode)
    verify_battle(g, actor, remote, resources)
    byte(eq + 1236, 1)
    assert encode(actor, 5001) == 0x501 and decode(actor, 0xC01) == 12001
    assert bytes(g.cpu.mem_read(eq + 1241, 1)) == b"\x05"


def quest_arc(name):
    romfs = os.environ.get("MHGU_ROMFS")
    if not romfs:
        raise SystemExit("Set MHGU_ROMFS to the GU RomFS root containing nativeNX.")
    return Path(romfs) / "nativeNX/arc/player/quest" / name


def load_hit_data(g, actor, remote, encode):
    arc = quest_arc("w00.arc")
    b = arc.read_bytes()
    for i in range(struct.unpack_from("<H", b, 6)[0]):
        name, kind, cs, _, off = struct.unpack_from("<64s4I", b, 12 + 80 * i)
        if kind == 0x4E2EF008 and name.rstrip(b"\0").endswith(b"pl_we00_hitdata"):
            hdp = zlib.decompress(b[off : off + cs])
            break
    else:
        raise AssertionError("missing retail GS hit data")
    assert hdp[:4] == b"HDP\0"
    data = g.alloc(len(hdp))
    g.cpu.mem_write(data, hdp)
    obj, vt, shapes = g.alloc(112), g.alloc(72), g.alloc(112)
    g.put(obj, vt)
    g.put(vt + 64, 0x270D8C)
    g.put(vt + 68, 0x270D98)
    g.put(obj + 100, data)
    g.put(obj + 104, data + 16)
    g.stub(0x13EFDC4, lambda dest, n, *_: g.cpu.mem_write(dest, bytes(n)))
    for a in (actor, remote):
        g.run(0x1687A8, a + 12320)  # Includes GU's unit collision scale.
        g.put(a + 12696, obj)
        g.put(a + 12700, shapes)
    g.stub(0x16B920, lambda *_: 1)  # engine-wide collision serial
    # Outside a style strike the native HDP table remains intact.
    for public, index in [(5106, 6), (5107, 5)]:
        assert encode(actor, public) == 0x500 + public % 1000
        stock = data + 16 + 56 * index
        assert g.run(HIT_ROW, remote, index) == stock
        assert g.run(HIT_ROW, actor, index) == stock


def verify_action_lifecycle(elf, config):
    g = boot(elf, config)
    p, eq, remote = hunters(g)
    # Scene notifications and unrelated actor predicates are outside this
    # check. Execute the real common transition/reset and both NRO detours.
    for fn in (0x2AC690, 0x27C8F8, 0xE6890, 0x2827B4,
               0x227B3C, 0x2F722C, 0x2C7E28):
        g.stub(fn, lambda *_: 0)
    for off in (432, 628, 1012, 1356, 1780, 1792, 1796, 1732, 1744,
                1748, 1736, 1740, 1724, 1120, 1812):
        g.stub(g.get(0x1749A88 + off), lambda *_: 0)
    g.cpu.mem_write(p + 9478, struct.pack("<3H", 1, 0, 0))
    g.cpu.mem_write(p + 5, b"\x02")
    g.cpu.mem_write(p + 10057, b"\x03")
    g.cpu.mem_write(p + 12325, b"\x01")
    g.put(p + 9488, 0x12345678)
    g.put(p + 13100, 0x12345678)
    for stock in (0, 1, 19, 800):
        g.run(GS_CHANGE_ACTION, p, 16, stock, 0)
        assert (g.half(p + 9478), g.half(p + 9482)) == (1, 0)
        assert g.cpu.mem_read(p + 5, 1) == b"\x02"
        assert g.get(p + 13100) == 0x12345678
    for move in (0, 19, 47, 59, 200):
        previous = (g.half(p + 9478), g.half(p + 9482))
        g.run(CHANGE_ACTION, p, 16, 0x8000 + move, 0)
        assert (g.half(p + 9478), g.half(p + 9482)) == (16, 0x8000 + move)
        assert (g.half(p + 9480), g.half(p + 9484)) == previous
        assert g.cpu.mem_read(p + 5, 1) == b"\0"
        assert g.cpu.mem_read(p + 12325, 1) == b"\0"
        assert g.cpu.mem_read(p + 10057, 1)[0] & 6 == 0
        assert g.get(p + 9488) == 0 and g.get(p + 13100) == 0x12345678
    g.run(GS_CHANGE_ACTION, p, 64, 1, 0)
    assert (g.half(p + 9478), g.half(p + 9482)) == (64, 1)
    verify_weapon_arts(g, p)
    g.cpu.mem_write(eq + 1241, b"\0")
    g.run(CHANGE_ACTION, remote, 16, 19, 0)
    assert g.half(remote + 9482) == 19
    print("GU common lifecycle: independent slots, Hunter Art dispatch, stock rejection, history, cleanup and damage passed")


def quest_entries(arc):
    b = quest_arc(arc).read_bytes()
    for i in range(struct.unpack_from("<H", b, 6)[0]):
        name, kind, size, _, off = struct.unpack_from("<64s4I", b, 12 + 80 * i)
        yield name.rstrip(b"\0"), kind, b[off : off + size]


def verify_weapon_arts(g, p):
    arts = [
        struct.unpack_from("<H4xH", zlib.decompress(raw), 0x1F8)
        for name, kind, raw in quest_entries("w00_m.arc")
        if kind == 0x5139901D and b"\\sa\\" in name
    ]
    lmt = next(zlib.decompress(raw) for name, kind, raw in quest_entries("w00.arc")
               if kind == 0x76820D81 and name.endswith(b"\\w00_sa"))
    count = struct.unpack_from("<H", lmt, 6)[0]
    bank, header, events = g.alloc(104), g.alloc(8 + 4 * count), g.alloc(288)
    g.put(p + 3844 + 7 * 4, bank)
    g.put(bank + 100, header)
    g.cpu.mem_write(header, lmt[:8])
    # Keep the retail clip lengths and playback metadata. The offline hunter
    # has neither a skeleton nor effect tracks to render.
    for slot in (1, 51, 101, 151, 155, 156):
        off = struct.unpack_from("<I", lmt, 8 + 4 * slot)[0]
        assert off != 0
        clip = g.alloc(64)
        g.cpu.mem_write(clip + 8, lmt[off + 8 : off + 48])
        g.put(clip + 52, events)
        g.put(header + 8 + 4 * slot, clip)
    g.put(p + 240, g.alloc(32))
    g.put(p + 1188, 1)
    g.cpu.mem_write(p + 1206, b"\x02")
    for fn in (0x2AC4BC, 0x2B7240, 0x2864C4, 0x1160294, 0x282624, 0x11688D0):
        g.stub(fn, lambda *_: 0)
    g.stub(0x27C664, lambda *_: 1)  # hunter model is loaded
    starters = [1, 1, 1, 51, 51, 51, 101, 101, 101, 155, 156, 151]
    for i, ((group, slot), motion) in enumerate(zip(arts, starters, strict=True)):
        g.put(p + 10084, i % 3)
        g.run(GS_CHANGE_ACTION, p, group, slot, 0x40)
        g.run(g.hooks[DISPATCH], p)
        assert (g.half(p + 9478), g.half(p + 9482)) == (group, slot)
        assert g.cpu.mem_read(p + 5, 1)[0] in (1, 2)
        assert g.half(p + 1204) == 0x700 + motion

    for start, follow_up in ((54, 57), (62, 63), (63, 64), (64, 65), (90, 91)):
        g.run(GS_CHANGE_ACTION, p, 16, start, 0x40)
        g.run(GS_CHANGE_ACTION, p, 16, follow_up, 0)
        assert g.half(p + 9482) == follow_up
        g.run(GS_CHANGE_ACTION, p, 16, 19, 0)
        assert g.half(p + 9482) == follow_up


def verify_battle(g, p, remote, resources):
    routes = effect_routes()

    def h(off, n):
        g.cpu.mem_write(p + off, struct.pack("<H", n))

    def f(off, n):
        g.cpu.mem_write(p + off, struct.pack("<f", n))

    def phase(n):
        g.cpu.mem_write(p + 5, bytes([n]))

    def action(group, act):
        h(9480, g.half(p + 9478))
        h(9484, g.half(p + 9482))
        h(9478, group)
        h(9482, act)
        phase(0)
        g.cpu.mem_write(p + 9384, bytes(64))
        g.cpu.mem_write(p + 10057, bytes([g.cpu.mem_read(p + 10057, 1)[0] & ~6]))

    def draw_weapon(kind=0):
        route = g.run(DRAW_ROUTE, p, kind)
        g.run(GS_CHANGE_ACTION, p, *struct.unpack("<3H", g.cpu.mem_read(route, 6)))

    def tick(frame=None, held=0, pressed=0, draw=False):
        if frame is not None:
            f(1280, frame)
            f(5332, frame)
        h(5108, held)
        h(5112, pressed)
        # Resolved native commands, not the first physical edge of a combo.
        combo = {0x20: 2, 0x100: 4, 0x120: 0x40}.get(pressed & 0x120, 0)
        if pressed & 0x40:
            combo |= 0x20
        g.put(p + 5188, combo)
        g.cpu.mem_write(p + 5140, bytes(32))
        for active, cmd in ((pressed & 0x1000, 8), (pressed & 0x80, 3),
                            (pressed & 0x10, 13), (pressed & 0x40, 70), (held & 0x40, 0)):
            if active:
                g.run(0x285DE4, p, cmd)
        # Execute GU's weapon command mapper rather than recreating its bits.
        g.run(0x1160058, p)
        if draw:
            draw_weapon()
        if g.get(p + 9384):
            g.run(COMMAND_UPDATE, p)
        g.run(g.hooks[DISPATCH], p)
        drain()

    def clip():
        return g.half(p + 1204) & 255

    def value():
        return g.half(p + 12320 + 56)

    def hitstop_rates():
        # Run GU's contact feedback and read the actual hunter motion rate
        # for both slowdown and full stop. Preserve the surrounding scene.
        result = p + 12320 + 6
        timers = p + 10156
        old_result = bytes(g.cpu.mem_read(result, 1))
        old_timers = bytes(g.cpu.mem_read(timers, 8))
        rates = []
        for kind in (3, 4):
            g.cpu.mem_write(result, bytes([kind]))
            g.run(0x16DB68, 0, p + 12320)
            g.run(0x2A80F8, p)
            rates.append(g.cpu.reg_read(arm.UC_ARM_REG_S0))
        g.cpu.mem_write(result, old_result)
        g.cpu.mem_write(timers, old_timers)
        return tuple(rates)

    def whiff():
        return bytes(g.cpu.mem_read(p + 12325, 1)) == b"\0"

    # Exercise the real packaged dispatch, GU motion setter, input state and HDP
    # construction. Substitute the outer scene transitions and audio services.
    g.put(p + 240, g.alloc(32))
    g.put(p + 1188, 1)
    g.put(p + 5068, 1)
    h(5078, 100)
    f(28, 2)
    g.stub(0x65F93C, lambda *_: 64)
    g.stub(0x65FA04, lambda *_: 0x800)
    for off in (752, 756, 792, 796, 808, 812):
        g.stub(g.get(0x1749A88 + off), lambda *_: 0)
    # Keep the two real detours. Replace only the common scene lifecycle,
    # independently exercised by verify_action_lifecycle.
    g.stub(g.originals[CHANGE_ACTION], lambda actor, group, act, _: action(group, act))
    g.stub(0x2B6E5C, lambda *_: action(1, 0))
    g.stub(0x2C4EEC, lambda *_: phase(1))
    f(5088, 900)
    # Camera and option lookups are outside this scene. Preserve native guard
    # directions and the continuous/discrete distinction in stamina requests.
    g.stub(g.get(0x1749A88 + 1412), lambda *_: 0)
    g.stub(g.get(0x1749A88 + 1428), lambda *_: 0)
    g.stub(0x27F678, lambda *_: 0x1234)
    # Run GU's actual angle comparisons, using a camera facing +Z.
    g.cpu.mem_write(p + 11990, b"\x01")
    g.put(p + 12016, 0)
    skills = set()
    audio = g.alloc(2800)
    g.put(0x1896F5C, audio)
    g.cpu.mem_write(p + 4180, b"\xff")  # GU's always-audible area in the offline scene.
    queued = set()
    charge_rises = []

    def drain():
        for i in range(64):
            slot = audio + 236 + 36 * i
            if g.cpu.mem_read(slot, 1)[0]:
                cue, res = g.get(slot + 16), g.get(slot + 32)
                assert g.cpu.mem_read(slot + 2, 2) == b"\x07\x01"
                assert g.get(slot + 20) == g.get(slot + 28) == p
                assert g.cpu.mem_read(slot + 4, 12) == g.cpu.mem_read(p + 64, 12)
                assert g.run(0xB68280, res, cue)
                request = cue - 4096
                queued.add((res, request))
                if 128 <= request <= 130:
                    charge_rises.append(request)
                g.cpu.mem_write(slot, b"\0")

    g.stub(0x2864C4, lambda _, k, *__: int(k in skills))
    from unicorn import arm_const as arm

    def stamina(actor, continuous):
        cost = struct.unpack(
            "<f", struct.pack("<I", g.cpu.reg_read(arm.UC_ARM_REG_S0))
        )[0]
        if continuous:
            cost *= struct.unpack("<f", g.cpu.mem_read(actor + 28, 4))[0]
        current = struct.unpack("<f", g.cpu.mem_read(actor + 5088, 4))[0]
        f(5088, max(0, current + cost))

    g.stub(0x2A5CE4, lambda actor, *_: stamina(actor, True))
    g.stub(0x2A5EBC, lambda actor, *_: stamina(actor, False))

    def neutral():
        action(1, 0)
        g.cpu.mem_write(p + 10057, b"\x01")
        h(1204, g.run(g.hooks[ENCODE], p, 5001))
        phase(1)

    for drawn in (5001, 5004):
        neutral()
        h(1204, g.run(g.hooks[ENCODE], p, drawn))
        tick(0, held=0x10, pressed=0x10)
        assert (g.half(p + 9478), g.half(p + 9482)) == (1, 2)
        for _ in range(30):
            tick(held=0x10)
        assert g.half(p + 9482) == 2
    for button, expected in [(0x100, 6), (32, 1), (0x120, 37)]:
        neutral()
        tick(0, held=button, pressed=button)
        assert clip() == expected
        assert g.half(p + 9482) == 0x8000 + {0x100: 0, 32: 19, 0x120: 48}[button]
    # Replay the real combination resolver: X then A and A then X must both
    # wait on the first edge and enter draw uppercut on the next frame.
    g.stub(0x27D5C4, lambda *_: 0)
    g.stub(0x3A5ED0, lambda *_: 0)
    g.stub(0x5584A0, lambda *_: 0)
    for first, second in ((32, 0x100), (0x100, 32)):
        neutral()
        g.cpu.mem_write(p + 10057, b"\0")
        g.put(p + 5188, 0)
        for held, edge, expected in ((first, first, 1), (0x120, second, 48)):
            h(5108, held)
            h(5112, edge)
            g.put(p + 5140, 0)
            g.cpu.mem_write(p + 5156, bytes(16))
            g.run(0x27CA28, p)
            g.run(0x1160058, p)
            if g.get(p + 5156) & 2:
                draw_weapon()
            g.run(g.hooks[DISPATCH], p)
            assert clip() == expected
        assert g.half(p + 9482) == 0x8000 + 47
    for kind in (2, 4):
        action(2, 3)
        g.cpu.mem_write(p + 10057, b"\0")
        g.put(p + 5156, 1 << 1)
        draw_weapon(kind)
        assert (g.half(p + 9478), g.half(p + 9482)) == (32, 0)
    # A monster hit is not necessary to route a directional input within
    # the ordinary sweep window. Holding right + A must select state 45.
    neutral()
    tick(0, held=0x100, pressed=0x100)
    # Leave the PEL handle resolver, proof binder and scene registration native.
    manager = g.alloc(128)
    g.put(0x188A0D4, manager)
    g.put(0x2126F48, g.alloc(16))
    g.put(manager + 92, 16)
    for off, size in [(96, 64), (100, 128), (104, 64), (108, 8)]:
        g.put(manager + off, g.alloc(size))
    g.put(g.get(manager + 108) + 4, 0xFFFFFFFF)
    units = []
    failed_allocations = []

    def allocate_proof(size, *_):
        if failed_allocations and failed_allocations.pop(0):
            return 0
        return g.alloc(size)

    g.stub(0x3288C4, allocate_proof)
    g.stub(0x328900, lambda unit, *_: g.put(unit, 0x17529CC))
    g.stub(0xC067EC, lambda scene, layer, unit, _: units.append(unit))
    tick(10, pressed=0x1000)
    assert not units and struct.unpack("<f", g.cpu.mem_read(p+5088,4))[0] == 900
    tick(12, pressed=0x800)
    assert units and all(g.cpu.mem_read(u + 344, 1)[0] == 0 for u in units)
    assert 0x32B210 not in g.hooks
    g.put(0x188A0D4, 0)
    assert struct.unpack("<f", g.cpu.mem_read(p + 5088, 4))[0] == 750
    assert struct.unpack("<f", g.cpu.mem_read(p + 10296, 4))[0] > 0
    tick(14,held=0x800)
    assert struct.unpack("<f",g.cpu.mem_read(p+5088,4))[0] == 750
    h(5136, 120)
    h(5132, 0x4000)
    tick(80, held=0x100, pressed=0x100)
    assert clip() == 56, clip()
    h(5136, 0)
    stock, entries, resource, command, table_list, tables = (
        g.alloc(n) for n in (24, 4, 124, 24, 120, 48)
    )
    g.cpu.mem_write(stock, struct.pack("<6I", 0, 0, 1, 1, 0, entries))
    g.put(entries, command)  # An inactive stock row stands in for the retail table.
    g.put(command + 8, resource)
    g.put(table_list + 104, 12)
    g.put(table_list + 116, tables)
    g.put(tables + 10 * 4, stock)
    g.put(tables + 11 * 4, stock)
    g.stub(0x285F68, lambda *_: table_list)

    def roll(side):
        action(1, 6)
        g.cpu.mem_write(p + 10057, b"\x01")
        g.put(p + 4076, 0)
        g.put(p + 5280, 0)
        h(5132, {0: 0x4000, 9102: 0x8000, 32769: 0}[side])
        h(5136, 120 if side else 0)
        packed = g.run(g.hooks[ENCODE], p, 28)
        assert packed & 255 == {0: 28, 9102: 12, 32769: 13}[side], (hex(packed), side)
        h(1204, packed)
        phase(2)
        f(1268, 0)
        f(5328, 0)
        f(1284, 80)
        g.run(0x115FDF8, p)  # GU's drawn GS roll binds weapon table 2011.
        bound = g.get(p + 9384)
        assert bound != stock and g.get(bound + 8) == 3
        assert g.get(g.get(bound + 20)) == command
        return packed

    for side, key, start in [(0, 32, 62), (9102, 32, 62), (32769, 32, 63),
                             (32769, 0x120, 37)]:
        packed = roll(side)
        tick(29, held=key, pressed=key)
        assert not any(g.cpu.mem_read(p + 9392, 48))
        tick(30, held=key, pressed=key)
        assert g.half(p + 1204) == packed
        # The earlier choice survives release and a later competing input.
        if key == 32:
            tick(31, held=0x120, pressed=0x120)
        tick(43)
        assert g.half(p + 1204) == packed
        tick(44)
        assert clip() == start
        assert not g.get(p + 9384)
    for side, close in [(0, 54), (9102, 64), (32769, 64)]:
        packed = roll(side)
        tick(close, held=32, pressed=32)
        assert g.half(p + 1204) == packed
        roll(side)
        tick(close - 0.5, held=32, pressed=32)
        assert clip() == (63 if side == 32769 else 62)

    # Guard recovery uses the native common B command with no pre-input.
    for stick, strength, expected in [(0x4000, 0, 28), (0x8000, 120, 12), (0, 120, 13)]:
        action(32, 0)
        g.cpu.mem_write(p + 10057, b"\x01")
        g.put(p + 4076, 0)
        g.put(p + 5280, 0)
        h(5132, stick)
        h(5136, strength)
        h(1204, g.run(g.hooks[ENCODE], p, 5005))
        f(1284, 80)
        f(1268, 0)
        phase(1)
        g.run(0x115FDEC, p)  # GU's GS guard startup binds table 2010.
        bound = g.get(p + 9384)
        assert g.get(bound + 8) == 2 and g.get(g.get(bound + 20)) == command
        f(5328, 1)
        tick(55, pressed=0x80)  # GU's projected motion frame is already 56.
        tick(56)
        assert g.half(p + 9478) == 32
        f(5328, 0)
        tick(56, pressed=0x80)
        assert (g.half(p + 9478), g.half(p + 9482)) == (1, 6)
        assert g.run(ENCODE, p, 28) & 255 == expected
    h(5136, 0)
    for key, expected in [(32, 31), (0x100, 47), (0x120, 48)]:
        action(1, 0)
        phase(1)
        g.cpu.mem_write(p + 10057, b"\x00")
        tick(0, held=key, pressed=key, draw=True)
        assert clip() == expected
    h(9480, 16)
    for group in (1, 2):
        action(group, 0)
        phase(1)
        g.cpu.mem_write(p + 10057, b"\0")
        tick(0, held=0x120, pressed=0x120, draw=True)
        assert clip() == 48
    # Native sprint accepts the same draw request with R still held. A tap
    # switches at GU's ten-frame draw decision without entering charge 1401.
    action(2, 3)
    phase(1)
    g.cpu.mem_write(p + 10057, b"\0")
    tick(0, held=0x60, pressed=0x20, draw=True)
    assert clip() == 31
    tick(4, held=0x40)
    tick(10, held=0x40)
    assert clip() == 10 and g.half(p + 9482) == 0x8000 + 13
    assert g.run(0x2A263C, p) == 1
    tick(36, held=0x40)
    assert clip() == 28
    assert struct.unpack("<2f", g.cpu.mem_read(p + 12320 + 92, 8)) == (4., 10.)
    swing = (resources[r"sound\se\pl\frontier_gs\iron"], 7)
    queued.discard(swing)
    tick(39)
    assert swing not in queued
    tick(40)
    assert swing in queued
    action(2, 3)
    phase(1)
    g.cpu.mem_write(p + 10057, b"\0")
    tick(0, held=0x60, pressed=0x20, draw=True)
    tick(10, held=0x60)
    tick(54, held=0x60)
    assert clip() == 1 and g.half(p + 9482) == 0x8000 + 20
    # Hold X; an early release buffers, and may execute only on frame 58.
    neutral()
    tick(held=32, pressed=32)
    assert clip() == 1
    tick(34, held=32)
    tick(42)
    assert clip() == 1
    tick(58)
    assert clip() == 28 and value() == 60
    # A release applies the source's single 22.5-degree adjustment, using
    # GU's real camera-relative stick query (including wrap across zero).
    for facing, stick, expected in ((0, 0x8000, 0x1000), (0, 0, 0xF000),
                                    (0xF000, 0x5000, 0), (0, 0x4000, 0)):
        h(5136, 0)
        neutral()
        g.put(p + 4076, facing)
        g.put(p + 5280, facing)
        tick(held=32, pressed=32)
        tick(34, held=32)
        tick(42)
        h(5136, 120)
        h(5132, stick)
        tick(58)
        assert clip() == 28 and g.get(p + 4076) == expected
        assert g.get(p + 5280) == expected
        tick(60)
        assert g.get(p + 4076) == expected  # no repeated rotation during swing
    h(5136, 0)
    joints, lookup = g.alloc(13 * 160), g.alloc(256)
    g.cpu.mem_write(lookup, bytes(range(13)) + bytes([255]) * 243)
    g.put(p + 1172, joints)
    g.put(p + 1176, lookup)
    g.put(0x188A0D4, manager)
    for key, end_charge in ((32, "release"), (0x120, "release"), (32, "hit"), (0x120, "unequip")):
        units.clear()
        failed_allocations[:] = [False, True, False, True]
        neutral()
        tick(0, pressed=key, held=key)
        for frame in range(2, 80, 2):
            tick(frame, held=key)
            if units:
                break
        assert len(units) == 2
        frame += 2
        tick(frame, held=key)
        assert sorted(g.get(u + 68) for u in units) == list(routes[9])
        proofs = units.copy()
        assert len(proofs) == 4
        # Three EFL children are already drawing. The fourth proof is
        # registered with the scene but still waiting for its resource.
        children = []
        for proof in proofs[:3]:
            child = g.alloc(896)
            g.put(child, 0x1752880)
            g.put(child + 12, 0x402)
            g.put(proof + 336, child)
            g.put(proof + 348, 1)
            children.append(child)
        if end_charge == "unequip":
            weapon = g.get(p + 5060) + 1236
            g.cpu.mem_write(weapon, b"\x01")
            g.run(g.hooks[DISPATCH], p)
            g.cpu.mem_write(weapon, b"\0")
        else:
            if end_charge == "hit":
                action(64, 1)
            tick(frame + 2)
        assert all(g.get(child + 12) & 7 == 3 for child in children)
        assert all(g.cpu.mem_read(proof + 48, 1) == b"\x02" for proof in proofs)
    # Charge during the loop; releasing a long hold uses a different SDT row.
    units.clear()
    charge_rises.clear()
    neutral()
    tick(held=32, pressed=32)
    for n in range(2, 202, 2):
        tick(n if n < 106 else 56 + (n - 106) % 50, held=32)
        if n == 100:
            assert len(units) >= 4
            assert all(g.cpu.mem_read(u + 344, 1)[0] == 0 for u in units)
    assert charge_rises == [128, 129, 130]
    tick(85)
    g.put(0x188A0D4, 0)
    assert clip() == 28 and value() == 146
    # X+A has a separate charge and release program.
    neutral()
    tick(held=0x120, pressed=0x120)
    assert clip() == 37
    tick(54)
    assert clip() == 39 and value() == 58
    for side, slot in [(9102, 12), (32769, 13), (0, 28)]:
        neutral()
        tick(0, held=0x120, pressed=0x120)
        tick(54)
        assert clip() == 39
        g.put(p + 4076, 0)
        g.put(p + 5280, 0)
        h(5132, {0: 0x4000, 9102: 0x8000, 32769: 0}[side])
        h(5136, 120 if side else 0)
        tick(66, pressed=0x80)
        assert g.half(p + 9478) == 16
        tick(68, pressed=0x80)
        assert (g.half(p + 9478), g.half(p + 9482)) == (1, 6)
        h(5136, 0)
        assert g.run(g.hooks[ENCODE], p, 28) & 255 == slot
    # GU's clock can pause. A long pause at one frame must not charge up.
    for key, hold, release, expected, interval in [
        (32, 0, True, 60, 20),
        (32, 90, False, 146, 20),
        (0x120, 100, True, 200, 20),
        (0x120, 111, False, 112, 20),
        (32, 32, True, 85, 16),
        (32, 47, True, 60, 24),
    ]:
        skills.clear()
        if interval != 20:
            skills.add(152 if interval == 16 else 153)
        neutral()
        tick(0, held=key, pressed=key)
        arm_at, loop_at, end = (34, 56, 106) if key == 32 else (54, 54, 122)
        tick(arm_at, held=key)
        for _ in range(45):
            tick(arm_at, held=key)
        for t in range(1, hold + 1):
            frame = arm_at + 2 * t
            tick(
                frame if frame < end else loop_at + (frame - end) % (end - loop_at),
                held=key,
            )
        if release:
            frame = max(58, arm_at + 2 * hold)
            tick(frame if frame < end else loop_at + (frame - end) % (end - loop_at))
        assert clip() in (28, 38) and value() == expected, (key, hold, clip(), value())
        if key == 0x120 and expected == 200:
            g.put(0x188A0D4, manager)
            units.clear()
            origin = bytes(g.cpu.mem_read(p + 64, 12))
            tick(27)
            assert not units
            tick(29)  # Cross Frontier event 29 at motion frame 28.
            tick(30)
            assert [g.get(u + 68) for u in units] == list(routes[1])
            assert all(bytes(g.cpu.mem_read(u + 304, 12)) == origin for u in units)
            g.put(0x188A0D4, 0)
            # Charged uppercut -> A joins the sweep at frame 44. SDT row 2
            # opens the same 52..66 window as a sweep started from idle.
            tick(76, held=0x100, pressed=0x100)
            assert clip() == 21
            tick(200)
            assert clip() == 6 and value() == 46
            assert struct.unpack("<f", g.cpu.mem_read(p + 1280, 4))[0] == 44.
            assert struct.unpack("<2f", g.cpu.mem_read(p + 12320 + 92, 8)) == (8., 14.)
    skills.clear()
    # Lion's Maw applies to the first registered strike even on a whiff.
    # Exercise GU's bonus and clear routines with all three native tiers.
    for tier in (1, 2, 3):
        neutral()
        g.put(p + 13096, 1)
        g.put(p + 13100, tier)
        tick(0)
        assert (g.get(p + 13096), g.get(p + 13100)) == (1, tier)
        tick(0, held=0x100, pressed=0x100)
        assert clip() == 6 and value() == 46 + tier
        assert g.get(p + 12320 + 76) & 0x8000
        assert (g.get(p + 13096), g.get(p + 13100)) == (0, 0)
        neutral()
        tick(0, held=0x100, pressed=0x100)
        assert value() == 46 and g.get(p + 12320 + 76) & 0x8000 == 0
    # Confirm a real monster-contact callback and reject a remote hunter's volume.
    # At frame 100 the whiff window is closed, but a monster hit extends it to
    # frame 104 (FZ state 0, predicate 2). Use the real GU result writer.
    for owner, kind, expected in [(remote, 1, 6), (p, 0, 6), (p, 1, 41)]:
        neutral()
        tick(held=0x100, pressed=0x100)
        assert clip() == 6 and value() == 46
        g.run(0x169D44, owner + 12320, 0x7000100, kind)
        tick(100, held=0x120, pressed=0x120)
        assert clip() == expected, (owner, kind, clip())
    # Interrupted attack cannot replace a GU damage reaction on the next tick.
    action(64, 1)
    h(1204, 0x101)
    tick(50, pressed=0x100)
    assert g.half(p + 1204) == 0x101

    # Give the native command interpreter a pending art. Confirm the request
    # reaches GU's resolver only once the current move allows an art cancel.
    art_pending = False
    art_prepared = []

    def resolve_art(_, group, slot, flags):
        for address, value in ((group, 16), (slot, 54), (flags, 0)):
            g.cpu.mem_write(address, struct.pack("<H", value))
        return 1

    g.stub(0x2A8B34, lambda *_: int(art_pending))
    g.stub(0x2ACB60, resolve_art)
    g.stub(0x2ACBEC, lambda *_: art_prepared.append(True))
    neutral()
    tick(0, held=0x100, pressed=0x100)
    art_pending = True
    tick(40)
    assert g.half(p + 9482) == 0x8000 and not art_prepared
    tick(72)
    assert g.half(p + 9482) == 54 and art_prepared == [True]
    art_pending = False
    # Only an actual landed, drawn, living launch victim can use the two FZ
    # recoveries. A rejected input costs nothing. The attack bank stays bound.
    g.put(p + 3844 + 4 * 10, 0)
    for direction, motion in [(6, 12), (7, 13)]:
        action(64, direction)
        h(1204, 0x10F)  # retail 1015, the landing clip selected by 2BB164
        h(5078, 100)
        g.cpu.mem_write(p + 10057, b"\x01")
        g.cpu.mem_write(p + 11988, b"\x01")
        phase(1)
        tick(0, pressed=0x80)
        assert g.half(p + 1204) == 0x10F
        phase(2)
        f(5088, 149)
        tick(2, pressed=0x80)
        assert g.half(p + 1204) == 0x10F
        f(5088, 450)
        tick(4, pressed=0x80)
        assert (
            clip() == motion
            and struct.unpack("<f", g.cpu.mem_read(p + 5088, 4))[0] == 300
        )
        assert struct.unpack("<f", g.cpu.mem_read(p + 11072, 4))[0] == 104
    f(5088, 900)

    # Pressing R is not credit. The guard collision test must accept, then GU
    # must actually enter recoil, and a monster must own the incoming hit.
    heavy_recoil = []
    g.stub(0x2F4CC8, lambda *_: heavy_recoil.append(True) or 1)
    g.stub(0x2831E0, lambda *_: 0)
    damage = g.alloc(4)

    def guard(when, accepted, monster=True, cost=None):
        action(32, 0)
        f(11072, 0)
        tick()
        packed = g.run(g.hooks[ENCODE], p, 5005)
        h(1204, packed)
        phase(1)
        f(1280, when)
        g.put(p + 9488, 16 if accepted else 0)
        h(12828, 0)
        result = g.run(g.hooks[BLOCK], p, 0)
        if accepted:
            heavy_recoil.clear()
            g.run(GUARD_REACTION, p, 2, p + 12820, damage)
            assert len(heavy_recoil) == int(not result)
        h(12826, 123 if monster else 0xFFFF)
        g.run(GS_CHANGE_ACTION, p, 32, 3, 0)
        if result:
            assert struct.unpack("<f", g.cpu.mem_read(p + 11072, 4))[0] == 8
        if cost is not None:
            power, flags, amount, loss = cost
            g.cpu.mem_write(p + 12823, bytes([power]))
            h(12820, flags)
            g.put(p + 10100, 100)
            g.put(p + 10104, 100)
            local_flags = g.get(p + 5068)
            g.put(p + 5068, 1)
            g.run(SHARPNESS, p, amount & 0xFFFFFFFF, 0)
            g.put(p + 5068, local_flags)
            assert g.get(p + 10100) == 100 - loss, (power, flags, skills, g.get(p + 10100))
            h(12820, 0)
        tick()
        if result:
            assert struct.unpack("<f", g.cpu.mem_read(p + 11072, 4))[0] == 14
        return result

    assert guard(8, 0) == 0 and clip() != 74
    assert guard(32, 1) == 0 and clip() != 74
    assert guard(28, 1) == 0 and clip() != 74
    skills.add(169)
    assert guard(28, 1, False) == 1
    assert guard(30, 1) == 0
    skills.clear()
    skills.add(168)
    assert guard(30, 1, False) == 1
    assert guard(32, 1) == 0
    skills.clear()
    assert guard(8, 1, False) == 1 and clip() == 74
    # Run the real GU sharpness consumer and equipment skill rules. Only the
    # scene's sharpness-colour table is supplied; no post-hit refund is used.
    g.stub(0x29CDFC, lambda *_: 0)
    for power, flags, amount, loss in ((14, 0, -1, 0), (39, 0, -2, 0),
                                       (40, 0, -10, 10), (20, 0x20, -2, 2)):
        assert guard(8, 1, False, (power, flags, amount, loss)) == 1
    skills.add(169)
    assert guard(8, 1, False, (49, 0, -2, 0)) == 1
    skills.clear()
    skills.add(85)
    assert guard(8, 1, False, (40, 0, -10, 5)) == 1
    skills.clear()
    assert guard(32, 1, False, (20, 0, -2, 2)) == 0
    assert guard(8, 1, False) == 1
    # GU's timer update uses rate 1, actor delta 2 at 30 Hz or 1 at 60 Hz.
    for delta in (1, 2):
        f(11072, 14)
        f(28, delta)
        for elapsed in range(delta, 15, delta):
            g.cpu.reg_write(arm.UC_ARM_REG_S0, 0x3F800000)
            g.run(0x53AA84, p, p + 11072)
            assert g.run(0x2A680C, p) == int(elapsed < 14)
    f(28, 2)
    # Guard follow-up points at the requested world direction, not the
    # direction frozen at guard entry, and stays fixed after the branch.
    for stick, expected in ((0, 0xC000), (0x8000, 0x4000), (0xC000, 0x8000)):
        h(5136, 0)
        g.put(p + 4076, 0)
        g.put(p + 5280, 0)
        assert guard(8, 1, False) == 1
        h(5136, 120)
        h(5132, stick)
        tick(20, held=32, pressed=32)
        assert g.half(p + 9482) == 0x8000 + 68
        assert g.get(p + 4076) == g.get(p + 5280) == expected
        tick(2)
        assert g.get(p + 4076) == expected
    h(5136, 0)
    g.put(p + 4076, 0)
    g.put(p + 5280, 0)
    # X counter. The contact is active before the shockwave appears.
    assert guard(8, 1, False) == 1
    tick(20, held=32, pressed=32)
    assert clip() == 76
    g.put(0x188A0D4, manager)
    f(64, 100.)
    f(68, 20.)
    f(72, 300.)
    g.put(p + 4076, 0x4000)
    units.clear()
    tick(85)
    assert whiff() and not units
    tick(86)
    hit = p + 12320
    assert value() == 200 and not whiff() and not units
    assert hitstop_rates() == (0x3E4CCCCD, 0)  # Blade contact: 0.2x or stopped.
    shape = resources[r"player\hit\frontier_gs\guard_counter"]
    assert g.get(hit + 132) == shape and g.get(hit + 136) == 0
    assert not g.get(hit + 48) & 0x08000000
    assert struct.unpack("<2f", g.cpu.mem_read(hit + 92, 8)) == (0., 20.)
    collision = g.alloc(10304)
    g.put(0x1857830, collision)
    def host_float(fn):
        reg = arm.UC_ARM_REG_S0
        arg, = struct.unpack("<f", struct.pack("<I", g.cpu.reg_read(reg)))
        g.cpu.reg_write(reg, int.from_bytes(struct.pack("<f", fn(arg)), "little"))
    g.stub(0x13EFDE8, lambda *_: host_float(math.sin))
    g.stub(0x13EFDF4, lambda *_: host_float(math.cos))
    assert g.run(HIT_EXPAND, hit) == 1 and g.get(collision + 10288) == 1
    kind, *capsule = struct.unpack("<B3x7f", g.cpu.mem_read(collision + 8240, 32))
    assert kind == 2 and all(abs(a-b) < 0.01 for a,b in zip(capsule, [200,300,270,300,300,70,300])), capsule
    for frame, expected in [(89, ()), (90, (19, 20)), (90, ()), (92, (21,)), (99, ()), (108, (7, 7))]:
        units.clear()
        tick(frame)
        layers = [layer for route in expected for layer in routes[route]]
        assert [g.get(u + 68) for u in units] == layers, frame
        assert all(struct.unpack("<3f", g.cpu.mem_read(u + 304, 12)) == (300.,20.,300.) for u in units)
    g.put(0x188A0D4, 0)
    for offset in (64, 68, 72):
        f(offset, 0.)
    g.put(p + 4076, 0)
    assert guard(8, 1, False) == 1
    tick(10, held=0x100, pressed=0x100)
    assert clip() == 74
    tick(20)
    assert clip() == 75 and whiff()
    tick(35)
    assert whiff()
    tick(36)
    assert value() == 125 and not whiff()
    g.put(0x188A0D4, manager)
    ready = routes[8]
    for block in range(1, 4):
        units.clear()
        assert guard(8, 1) == 1 and clip() == 74
        aura = [u for u in units if g.get(u + 68) in ready]
        assert len(aura) == (len(ready) if block == 3 else 0)
        assert all(g.get(u + 132) == 0x28 and g.half(u + 194) == 12 for u in aura)
    g.put(0x188A0D4, 0)
    tick(20, pressed=0x1000)
    assert clip() == 64
    g.put(0x188A0D4, manager)
    f(64, 100.)
    f(68, 20.)
    f(72, 300.)
    g.put(p + 4076, 0x4000)
    release = routes[7]
    def release_origins():
        return [struct.unpack("<3f", g.cpu.mem_read(u + 304, 12))
                for u in units if g.get(u + 68) in release]
    units.clear()
    tick(82)
    assert clip() == 65 and whiff() and not release_origins()
    f(28, 1.)
    # Simulate four game-time units of hit stop at motion frame 88 on GU 60 Hz.
    pulses = range(84, 280, 12)
    for elapsed in range(1, 281):
        motion_time = elapsed - min(max(elapsed - 88, 0), 4)
        frame = motion_time if motion_time < 136 else 108 + (motion_time - 136) % 28
        units.clear()
        g.cpu.mem_write(p + 12325, b"\0")
        g.put(0x188A0D4, 0 if elapsed == 108 else manager)
        tick(frame)
        hit = elapsed in pulses
        origin = (300., 20., 300.) if elapsed == 84 else (100., 20., 500.)
        assert release_origins() == ([origin] * len(release) if hit and elapsed != 108 else []), elapsed
        if hit and elapsed != 108:
            assert all(struct.unpack("<f", g.cpu.mem_read(u + 164, 4))[0] == 90.
                       for u in units if g.get(u + 68) in release)
        if elapsed == 84:
            g.put(p + 4076, 0)
        assert whiff() == (not hit), elapsed
        if hit:
            assert value() == 27 and g.get(p + 12320 + 136) == 0
            assert hitstop_rates() == (0x3F800000, 0x3F800000), "wave slowed its owner"
            assert g.get(p + 12320 + 48) & 0x08000000
            assert struct.unpack("<f", g.cpu.mem_read(p + 12320 + 96, 4))[0] == 12.0
            g.put(collision + 10288, 0)
            f(64, -1000.)  # The world capsule stays at the emission origin.
            assert g.run(HIT_EXPAND, p + 12320) == 1
            assert g.get(collision + 10288) == 1
            x, y, z = origin
            assert struct.unpack("<B3x7f", g.cpu.mem_read(collision + 8240, 32)) == (
                2, 200., x, y + 150., z, x, y + 50., z
            )
            f(64, 100.)
    f(28, 2.)
    g.put(0x188A0D4, 0)
    # Special can be synthesized by GU with no physical 0x1000 edge. Reject
    # the native kick request before consuming command 8 from drawn idle.
    for _ in range(3):
        assert guard(8, 1) == 1
    neutral()
    g.run(GS_CHANGE_ACTION, p, 16, 800, 0)
    assert (g.half(p + 9478), g.half(p + 9482)) == (1, 0)
    h(5112, 0)
    g.put(p + 5188, 0)
    g.put(p + 5140, 1 << 8)
    g.run(g.hooks[DISPATCH], p)
    assert clip() == 64 and g.half(p + 9482) == 0x8000 + 59
    blade = resources[r"sound\se\pl\frontier_gs\blade"]
    common = resources[r"sound\se\pl\frontier_gs\common"]
    iron = resources[r"sound\se\pl\frontier_gs\iron"]
    assert {
        (blade, 1),
        (blade, 2),
        (blade, 3),
        (common, 210),
        (iron, 5),
        (iron, 28),
        (iron, 13),
        (iron, 29),
    } <= queued
    assert (iron, 12) not in queued
    # Dash: use the occupied hunter's normal movement state and stick.
    ledge = False
    g.stub(0x2A8458, lambda *_: int(ledge))
    action(1, 0)
    h(1204, g.run(g.hooks[ENCODE], p, 5004))
    phase(1)
    h(5136, 0)
    tick(0)
    h(5136, 120)
    tick(2)
    assert clip() == 4
    h(5136, 0)
    tick(4)
    h(5136, 120)
    tick(6)
    assert clip() == 30 and whiff()
    assert g.cpu.mem_read(p + 10057, 1)[0] & 2
    before = struct.unpack("<f", g.cpu.mem_read(p + 5088, 4))[0]
    tick(30, held=32, pressed=32)
    assert struct.unpack("<f", g.cpu.mem_read(p + 5088, 4))[0] == before - 2
    assert g.cpu.mem_read(p + 10057, 1)[0] & 2 == 0
    assert clip() == 69 and whiff()
    tick(45)
    assert whiff()
    tick(46)
    assert value() == 60 and not whiff()
    g.cpu.hook_del(g.traps.pop(0x2831E0))
    verify_dash_climb(g, p, tick, phase)
    # The next drop uses GU's drawn roll-off action and preserves the run on landing.
    ledge = True
    tick(2, held=32, pressed=32)
    assert (g.half(p + 9478), g.half(p + 9482)) == (4, 14)
    assert g.get(p + 5280) == g.get(p + 4076)
    ledge = False
    g.stub(g.get(0x1749A88 + 828), lambda *_: 0)
    action(4, 3)
    tick(0)
    neutral()
    tick(0)
    assert clip() == 30 and g.half(p + 9482) == 0x8000 + 200
    # A normal B roll has no run to resume. The landing buffers its own flick.
    for landing in ((4, 3), (1, 138)):
        neutral()
        tick(0)
        action(1, 6)
        tick(0)
        action(4, 14)
        tick(0)
        action(*landing)
        double_flick(g, p, tick, 0)
        assert (g.half(p + 9478), g.half(p + 9482)) == landing
        neutral()
        tick(0)
        assert clip() == 30 and g.half(p + 9482) == 0x8000 + 200
    g.run(CHANGE_ACTION, p, 16, 0x8000 + 64, 0)
    tick(0)
    # Switching to another weapon retires an owned action before GU dispatch.
    eq = g.get(p + 5060)
    g.cpu.mem_write(eq + 1236, b"\x01")
    tick()
    assert (g.half(p + 9478), g.half(p + 9482)) == (1, 0)
    g.cpu.mem_write(eq + 1236, b"\0")
    verify_jump_slash(g, p, tick, neutral)
    print(
        "GU controls: early/held releases, guard rejection, monster-only credit, counters, dash contact and interruption passed"
    )


def double_flick(g, p, tick, start):
    for index, strength in enumerate((0, 120, 0, 120)):
        g.cpu.mem_write(p + 5136, struct.pack("<H", strength))
        tick(start + 2 * index)


def verify_dash_climb(g, p, tick, phase):
    table, entries, entry, resource, walk, walk_command, tables, ids = (
        g.alloc(n) for n in (24, 8, 24, 124, 24, 124, 120, 4)
    )
    g.cpu.mem_write(table, struct.pack("<6I", 0, 0, 2, 2, 0, entries))
    g.cpu.mem_write(entries, struct.pack("<2I", walk, entry))
    g.cpu.mem_write(walk, struct.pack("<3I3f", 0, 1, walk_command, 0, 0, 0))
    g.cpu.mem_write(walk_command + 100, struct.pack("<4H4I", 2, 0, 0, 4, 0, 0, 0, 0))
    g.cpu.mem_write(entry, struct.pack("<3I3f", 0, 1, resource, 0, 0, 0))
    ledge = next(
        zlib.decompress(raw)
        for name, _, raw in quest_entries("w00_m0.arc")
        if name.endswith(b"_0x0001_137b")
    )
    words = [struct.unpack_from("<H", ledge, offset)[0] for offset in (504, 510, 516, 522)]
    params = [struct.unpack_from("<I", ledge, offset)[0] for offset in (528, 536, 544, 552)]
    g.cpu.mem_write(resource + 100, struct.pack("<4H4I", *words, *params))
    g.put(tables + 104, 1)
    g.put(tables + 116, ids)
    g.put(ids, table)
    g.stub(0x285F68, lambda *_: tables)
    climbable = False
    g.stub(0x2A8384, lambda *_: int(climbable))
    g.stub(0x27FF28, lambda *_: 1)  # hunter faces the climbable edge
    g.run(CHANGE_ACTION, p, 16, 0x8000 + 200, 0)
    tick(0)
    assert g.get(p + 9488) & 0x180 == 0x180
    bound = g.get(p + 9384)
    assert bound != table and g.get(bound + 8) == 1
    assert g.get(g.get(bound + 20)) == entry
    tick(2)
    assert g.half(p + 9482) == 0x8000 + 200
    climbable = True
    tick(4)
    assert (g.half(p + 9478), g.half(p + 9482)) == (1, 137)
    climbable = False
    g.cpu.hook_del(g.traps.pop(0x285F68))

    # Let the native drawn climb finish with the stick still held. It chooses
    # walk 2:30 while the climb clip is still current.
    g.stub(0x2E8C24, lambda actor, height, *_: g.put(height, g.get(actor + 68)) or 1)
    phase(2)
    motion_flags = g.cpu.mem_read(p + 1206, 1)[0]
    g.cpu.mem_write(p + 1206, bytes([motion_flags | 4]))
    g.run(0x2CDEC4, p, 7)
    assert (g.half(p + 9478), g.half(p + 9482)) == (2, 30)
    g.cpu.mem_write(p + 1206, bytes([motion_flags]))
    g.cpu.hook_del(g.traps.pop(0x2E8C24))
    tick(0)
    assert g.half(p + 9482) == 0x8000 + 200
    assert g.half(p + 1204) & 255 == 30


def verify_jump_slash(g, p, tick, neutral):
    lmt = next(
        zlib.decompress(raw) for name, kind, raw in quest_entries("w00.arc")
        if kind == 0x76820D81 and name.endswith(b"\\w00")
    )
    count = struct.unpack_from("<H", lmt, 6)[0]
    bank, header, events = g.alloc(104), g.alloc(8 + 4 * count), g.alloc(288)
    g.put(p + 3844 + 5 * 4, bank)
    g.put(bank + 100, header)
    g.cpu.mem_write(header, lmt[:8])
    for slot in (135, 150, 233):
        (off,) = struct.unpack_from("<I", lmt, 8 + 4 * slot)
        clip = g.alloc(64)
        g.cpu.mem_write(clip + 8, lmt[off + 8 : off + 48])
        g.put(clip + 52, events)
        g.put(header + 8 + 4 * slot, clip)

    # Provide the retail strong wide slash row and a common command entry.
    table, entries, common, stock, resource, tables, ids = (
        g.alloc(n) for n in (24, 8, 24, 24, 124, 120, 48)
    )
    g.cpu.mem_write(table, struct.pack("<6I", 0, 0, 2, 2, 0, entries))
    g.cpu.mem_write(entries, struct.pack("<2I", common, stock))
    g.cpu.mem_write(stock, struct.pack("<3I3f", 0, 1, resource, 0, 54, 158))
    g.cpu.mem_write(resource + 100, struct.pack("<4H4I", 16, 44, 0x1000, 8, 25, 0, 0, 0))
    g.put(tables + 104, 12)
    g.put(tables + 116, ids)
    g.put(ids + 8 * 4, table)
    g.stub(0x285F68, lambda *_: tables)

    # Mock terrain and rendering while leaving the native flight action,
    # motion setter, command binding and Frontier dispatch in place.
    grounded = False
    hit_rows = []
    g.stub(0x2A84D8, lambda *_: int(grounded))
    g.stub(0x539F4C, lambda *_: 0)
    g.stub(0x2A063C, lambda *_: 0)
    g.stub(0x27C664, lambda *_: 1)  # hunter model is loaded
    g.stub(0x19998, lambda *_: 0)  # animation-change notification
    g.stub(0x1160294, lambda *_: 0)  # motion-linked rendering
    g.stub(0x2B7240, lambda _, __, row, flags: hit_rows.append((row, flags)))
    for slot, motion, key, destination, source in (
        (38, 5233, 0x20, 40, 1451),
        (67, 5150, 0x100, 0, 1406),
    ):
        neutral()
        grounded = False
        g.cpu.mem_write(p + 10057, bytes([int(slot == 67)]))
        g.run(GS_CHANGE_ACTION, p, 16, slot, 0)
        tick(0)
        assert g.half(p + 9482) == slot
        actual = g.run(DECODE, p, g.half(p + 1204))
        assert actual == motion, (slot, actual, motion)
        tick(54, held=key, pressed=key)
        assert g.half(p + 9482) == slot and not g.get(p + 9384)
        grounded = True
        tick(54)
        assert g.run(DECODE, p, g.half(p + 1204)) == 5135
        assert hit_rows[-1] == (7, 3)
        bound = g.get(p + 9384)
        assert bound != table and g.get(bound + 8) == 3
        assert g.get(g.get(bound + 20)) == common
        for frame in (53, 158):
            tick(frame, held=key, pressed=key)
            assert g.half(p + 9482) == slot
        tick(54, held=key, pressed=key)
        assert g.half(p + 9482) == 0x8000 + destination
        assert g.half(p + 1204) & 255 == source % 100
        assert g.cpu.mem_read(p + 13115, 1) == b"\0"
    tick(72, held=0x20, pressed=0x20)
    assert g.half(p + 9482) == 0x8000 + 22  # sweep -> charge
    neutral()
    grounded = False
    g.run(GS_CHANGE_ACTION, p, 16, 67, 0)
    tick(0)
    double_flick(g, p, tick, 54)
    assert g.half(p + 9482) == 67  # flicks in midair cannot start a run
    grounded = True
    tick(62)
    double_flick(g, p, tick, 54)
    assert g.half(p + 9482) == 0x8000 + 200
    print("GU jump slash: both native motions, grounded command window, Frontier finisher and sweep continuation passed")


def verify_sdk_registration(elf):
    # Exercise the actual SDK caller in the great-sword NRO, not only the raw ABI.
    config = b"[Weapons]\r\n0 = 1\r\n1 = 1\r\n"
    for case in ("ready", "conflict", "worker"):
        g = boot(elf, config, ("registry",))
        register, selected = service(g)
        original_thread = g.thread
        if case == "conflict":
            assert g.run(register, style_descriptor(g, 1, 1), 0, 0) == 0
        elif case == "worker":
            g.thread = original_thread + 0x100
        calls = []
        g.cpu.hook_add(UC_HOOK_CODE, lambda *_: calls.append(1), begin=register, end=register)
        ex = g.load(CONTENT, 0x9200000)
        g.run(ex["forge_onLoad"], MANAGER)
        g.run(ex["forge_onInit"], MANAGER)
        assert "forge_onUpdate" not in ex
        assert len(calls) == (0 if case == "worker" else 1)
        g.thread = original_thread
        assert g.run(selected, 0) == int(case == "ready")
        assert g.run(selected, 1) == int(case == "conflict")
        assert sum("Conflict" in msg for _, msg in g.logs) == int(case == "conflict")


def verify():
    elf = (REPO / "mhgu/main.elf").read_bytes()
    on = b"[Weapons]\r\n0 = 1\r\n"
    verify_action_lifecycle(elf, on)
    verify_sdk_registration(elf)
    verify_valor(elf, on)
    g = boot(elf, on)
    verify_great_sword(g)
    g = boot(elf, on, ("sword", "registry"))
    assert g.run(service(g)[1], 0) == 0
    actor, _, _ = hunters(g)
    assert g.run(g.hooks[ENCODE], actor, 5001) == 0x501
    g = boot(elf, on, ("sword",))
    assert not g.hooks and g.files == {CONFIG: on}
    g = boot(elf, on, ("registry",))
    assert g.run(service(g)[1], 0) == 0
    for cfg in [
        None,
        b"[Weapons]\n0=0",
        b"1",
        b"[Weapons]\n0=-2",
        b"[Weapons]\n0=999",
        b"[General]\nstyle=1",
    ]:
        g = boot(elf, cfg)
        assert g.run(service(g)[1], 0) == 0
        actor, _, _ = hunters(g)
        assert g.run(g.hooks.get(ENCODE, ENCODE), actor, 5001) == 0x501
        if cfg is None:
            assert not g.files
    g = boot(elf, None, mount_ok=False)
    assert not g.files and g.run(service(g)[1], 0) == 0

    # A second plugin may register after config load. Its ID applies only to its weapon.
    g = boot(elf, b"[Weapons]\n0=1\n1=42")
    register, selected = service(g)
    actor, eq, remote = hunters(g)

    enc = g.alloc(4)
    g.stub(enc, lambda a, r, *_: 0x801 if a == actor and r == 5001 else 0xFFFFFFFF)
    other = style_descriptor(g, 42, 1, enc)
    assert g.run(selected, 1) == 0
    assert g.run(register, other, 0, 0) == g.run(register, other, 0, 0) == 0
    assert (
        g.run(register, style_descriptor(g, 42, 0), 0, 0) == 2
        and g.run(register, style_descriptor(g, 1, 1), 0, 0) == 2
    )
    for bad in [0, style_descriptor(g, 0, 0), style_descriptor(g, 65536, 0),
                style_descriptor(g, 3, 255), style_descriptor(g, 3, 0, art_slots=4)]:
        assert g.run(register, bad, 0, 0) == 1
    assert g.run(selected, 0) == 1 and g.run(selected, 1) == 42
    g.cpu.mem_write(eq + 1236, b"\x01")
    assert g.run(g.hooks[ENCODE], actor, 5001) == 0x801
    assert g.run(g.hooks[ENCODE], remote, 5001) == 0x501
    assert g.run(g.hooks[ENCODE], actor, 5101) == 0x565
    assert g.run(g.hooks[DECODE], actor, 0x801) == 8001
    print(
        "Forge INI create/read/fallback; independent style IDs, init-only registration, late or missing provider fallback: passed"
    )
    print(
        "GU local male/female great sword: occupied banks, motion roundtrip, weapon switch and native equipment preservation: passed"
    )


if __name__ == "__main__":
    verify()
