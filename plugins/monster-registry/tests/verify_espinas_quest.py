"""Run both shipping ARM32 NROs against GU 1.4 code. No game process or save files."""

import argparse
import math
import struct
import sys
import zlib
from pathlib import Path

from unicorn import UC_ARCH_ARM, UC_HOOK_CODE, UC_MODE_ARM, Uc
from unicorn import arm_const as arm

REPO = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(REPO / "plugins/monster-espinas/tests"))
from game_files import assets

REGS = [arm.UC_ARM_REG_R0, arm.UC_ARM_REG_R1, arm.UC_ARM_REG_R2, arm.UC_ARM_REG_R3]
STOP, STACK, HEAP = 0x7300000, 0x73FF000, 0x7200000
QUEST, PLAYER, MANAGER = 0x7010000, 0x7020000, 0x7030000


class Game:
    def __init__(self, elf):
        self.cpu = c = Uc(UC_ARCH_ARM, UC_MODE_ARM)
        c.ctl_set_cpu_model(arm.UC_CPU_ARM_MAX)
        ph = struct.unpack_from("<I", elf, 28)[0]
        stride, n = struct.unpack_from("<HH", elf, 42)
        segs = [struct.unpack_from("<8I", elf, ph + i * stride) for i in range(n)]
        for typ, off, va, _, size, mem, *_ in segs:
            if typ == 1:
                c.mem_map(va, (mem + 4095) & ~4095)
                c.mem_write(va, elf[off : off + size])
        c.mem_map(0x7000000, 0x400000)
        c.reg_write(
            arm.UC_ARM_REG_C1_C0_2, c.reg_read(arm.UC_ARM_REG_C1_C0_2) | 0xF00000
        )
        c.reg_write(arm.UC_ARM_REG_FPEXC, 0x40000000)
        self.next, self.allocs, self.hooks = HEAP, {}, {}
        self.traps, self.files, self.opened = {}, {}, {}
        self.logs = []
        self.write_success, self.io_mount = True, False
        self.put(0x188AD90, QUEST)
        self.put(0x1897F78, PLAYER)
        # Run GU's actual startup initializer, including the allocator's owner-thread
        # assignment. Supply only the OS thread/mutex, formatting and atexit services.
        self.thread = MANAGER + 0x800
        for addr in (0x13EFFB0, 0x13EFE00, 0x13EFE0C, 0x13EFF2C, 0x288):
            self.stub(addr, lambda *_: 0)
        self.stub(0x13EFFD4, lambda *_: self.thread)
        self.run(0x7AF404)
        # Stub services not under test. The GU ID map, DTI registry, native hunt,
        # original result transfer and both NRO detours still execute real ARM code.
        for addr in [
            0x227A98,
            0x264240,
            0x3A400C,
            0x3A83A4,
            0x65DDC4,
            0x65CB3C,
            0x524580,
            0x5245A8,
            0x38DCDC,
            0x1C95A4,
        ]:
            self.stub(addr, lambda *_: 0)
        self.w(0x7031000 + 56, 1)  # no size/crown UI in the result screen stub
        self.stub(0x3E21B8, lambda mgr, *_: self.put(mgr + 60, 17))
        self.stub(0x3E0980, lambda *_: 1)
        self.stub(0x3E1308, self.save_progress)
        self.mount_string = self.alloc(16)
        c.mem_write(self.mount_string, b"savedata\0")
        self.install_file_service()

    def get(self, p):
        return int.from_bytes(self.cpu.mem_read(p, 4), "little")

    def put(self, p, x):
        self.cpu.mem_write(p, struct.pack("<I", x))

    w = put

    def half(self, p):
        return int.from_bytes(self.cpu.mem_read(p, 2), "little")

    def alloc(self, n, *_):
        p = self.next
        self.next += (max(n, 1) + 15) & ~15
        assert self.next < STOP
        self.allocs[p] = n
        return p

    def resize(self, p, n, *_):
        dest = self.alloc(n)
        if p:
            self.cpu.mem_write(
                dest, bytes(self.cpu.mem_read(p, min(n, self.allocs[p])))
            )
        return dest

    def string(self, p):
        out = bytearray()
        while b := self.cpu.mem_read(p + len(out), 1)[0]:
            out.append(b)
        return out.decode()

    def stub(self, p, fn):
        def callback(c, _pc, _size, _):
            val = fn(*(c.reg_read(r) for r in REGS))
            c.reg_write(REGS[0], val or 0)
            c.reg_write(arm.UC_ARM_REG_PC, c.reg_read(arm.UC_ARM_REG_LR))

        self.traps[p] = self.cpu.hook_add(UC_HOOK_CODE, callback, begin=p, end=p)

    def hook(self, ret, addr, detour, saved):
        self.hooks[addr] = detour
        if addr in (0x50B64, 0x152960, 0x4A858, 0x3D7E90, 0x3A75C8, 0x3A3178, 0x56EF50):
            # Each starts with a PC-independent instruction in GU 1.4.
            trampoline = self.alloc(12)
            self.cpu.mem_write(
                trampoline,
                bytes(self.cpu.mem_read(addr, 4))
                + struct.pack("<II", 0xE51FF004, addr + 4),
            )
            self.put(saved, trampoline)
            self.cpu.hook_add(
                UC_HOOK_CODE,
                lambda c, *_: c.reg_write(arm.UC_ARM_REG_PC, detour),
                begin=addr,
                end=addr,
            )
        else:
            self.put(saved, addr)
        self.cpu.mem_write(ret, bytes(24))
        return ret

    def run(self, pc, *args, until=STOP):
        c = self.cpu
        c.reg_write(arm.UC_ARM_REG_SP, STACK)
        c.reg_write(arm.UC_ARM_REG_LR, STOP)
        for r, x in zip(REGS, args):
            c.reg_write(r, x)
        for i, x in enumerate(args[4:]):
            self.put(STACK + 4 * i, x)
        try:
            c.emu_start(pc, until, count=4500000)
        except Exception as e:
            raise RuntimeError(
                f"guest {pc:#x}, PC={c.reg_read(arm.UC_ARM_REG_PC):#x} LR={c.reg_read(arm.UC_ARM_REG_LR):#x}"
            ) from e
        end = c.reg_read(arm.UC_ARM_REG_PC)
        assert end == until, hex(end)
        return c.reg_read(REGS[0])

    def save_progress(self, mgr, *_):
        self.put(mgr + 60, 24 if self.write_success else 22)
        self.cpu.mem_write(mgr + 112, bytes([self.write_success]))

    def install_file_service(self):
        # GU MtFile type for this test only; leave the real DTI lookup intact.
        dti = self.alloc(32)
        name = self.alloc(8)
        table = self.alloc(64)
        self.cpu.mem_write(name, b"MtFile\0")
        self.run(0x7AEDCC, dti, name, 0, 4, 0, 0, 0)
        self.put(dti, table)
        self.files_vt = self.alloc(80)

        def slot(t, i, fn):
            addr = self.alloc(4)
            self.put(t + 4 * i, addr)
            self.stub(addr, fn)

        def new(*_):
            obj = self.alloc(4)
            self.put(obj, self.files_vt)
            return obj

        def open_(obj, p, mode, _):
            assert self.io_mount
            path = self.string(p)
            if mode == 19:
                self.files[path] = b""
            if path not in self.files:
                return 0
            self.opened[obj] = path
            return 1

        def read(obj, p, n, _):
            data = self.files[self.opened[obj]][:n]
            self.cpu.mem_write(p, data)
            return len(data)

        def write(obj, p, n, _):
            self.files[self.opened[obj]] = bytes(self.cpu.mem_read(p, n))
            return n

        slot(table, 2, new)
        for i, fn in [
            (1, lambda *_: 0),
            (6, open_),
            (7, lambda obj, *_: self.opened.pop(obj) and 0),
            (8, read),
            (12, write),
            (15, lambda obj, *_: len(self.files[self.opened[obj]])),
        ]:
            slot(self.files_vt, i, fn)

    def mount(self, *_):
        assert not self.io_mount
        self.io_mount = True
        return 1

    def unmount(self, *_):
        assert self.io_mount and not self.opened
        self.io_mount = False

    def load(self, path, base, host=None):
        b = path.read_bytes()
        c = self.cpu
        c.mem_map(base, 0x100000)
        c.mem_write(base, b)
        mod = self.get(base + 4)
        d = base + mod + self.get(base + mod + 4)
        tags = {}
        while self.get(d):
            tags[self.get(d)] = self.get(d + 4)
            d += 8
        names, ex = {}, {}
        for i in range(self.get(base + tags[4] + 4)):
            off, val = struct.unpack_from("<II", b, tags[6] + 16 * i)
            name = self.string(base + tags[5] + off)
            names[i] = name
            if val:
                ex[name] = base + val
        services = {
            "forge_hook_create": self.hook,
            "forge_log": lambda level, fmt, msg, _: self.logs.append(
                (level, self.string(msg))
            ),
            "malloc": self.alloc,
            "realloc": self.resize,
            "free": lambda *_: 0,
            "forge_mem_getMainTextAddr": lambda *_: 0,
            "forge_fs_mountSaveData": self.mount,
            "forge_fs_unmountSaveData": self.unmount,
            "forge_fs_getSaveDataMountPoint": lambda *_: self.mount_string,
            "forge_fs_fileExists": lambda p, *_: int(self.string(p) in self.files),
        }
        if host:
            services.update(host)
        for tab, sz in [(17, 18), (23, 2)]:
            for off in range(tags[tab], tags[tab] + tags[sz], 8):
                dest, info = struct.unpack_from("<II", b, off)
                kind = info & 255
                if kind == 23:
                    self.put(base + dest, base + self.get(base + dest))
                else:
                    assert kind in (21, 22)
                    p = base + 0x80000 + 4 * (info >> 8)
                    name = names[info >> 8]
                    if p not in self.traps:

                        def unsupported(*_, name=name):
                            raise AssertionError(f"unexpected call: {name}")

                        ignored = name in (
                            "forge_log_getLevel",
                            "forge_hook_getContext",
                        )
                        self.stub(
                            p,
                            services.get(
                                name, (lambda *_: 0) if ignored else unsupported
                            ),
                        )
                    self.put(base + dest, p)
        return ex


def service(g):
    p = g.alloc(64)
    g.cpu.mem_write(p, b"mhgu::MonsterRegistryV2\0")
    dt = g.run(0x7AEF68, g.run(0x7CBDB4, p, 0xFFFFFFFF) & 0x7FFFFFFF)
    assert dt and g.get(dt + 32) == int.from_bytes(b"MRG2", "little")
    api = g.get(dt + 36)
    return [g.get(api + 4 * i) for i in range(3)]


def verify_all_targets(g):
    def run_goal(callback, first, second):
        q = g.alloc(7144)
        g.put(q + 236, 0x1008)  # all-monsters goal
        g.put(q + 4900, second)
        g.put(q + 4900 + 3 * 36, first)
        g.put(q + 4824 + 3 * 16, first)

        def hunt(key):
            completed = g.run(callback, q, key, 0, 100, 1)
            return completed, g.half(q + 242), g.get(q + 236)

        # One configured monster has not spawned. Its slot must not be skipped,
        # and the second slot's bit must wait until its remaining count is zero.
        before_spawn = hunt(first)
        g.cpu.mem_write(q + 4824, struct.pack("<HB", second, 1))
        still_alive = hunt(second)
        g.cpu.mem_write(q + 4826, b"\0")
        return before_spawn, still_alive, hunt(second)

    native = run_goal(0x3A75C8, 1, 3)
    assert [step[0] for step in native] == [0, 0, 1] and native[-1][1] == 0b1001
    assert run_goal(g.hooks[0x3A75C8], 107, 108) == native


def verify_icon(g, root, manager, manager_vt, quest, retail):
    data = (root / "GUI/monster/espinas.tex").read_bytes()
    assert data[:4] == b"TEX\0"
    w = struct.unpack_from("<I", data, 8)[0]
    assert ((w >> 6) & 8191, w >> 19, data[13]) == (128, 128, 7)
    assert not list(root.rglob("*micon*.tex"))
    tex, fn = g.alloc(240), g.alloc(4)
    g.put(tex + 80, 1)
    g.put(tex + 220, 128)
    g.put(tex + 224, 128)
    reads = []

    def load(m, dti, path, flags):
        assert (m, dti, g.string(path), flags) == (
            manager,
            0x21255D0,
            r"GUI\monster\espinas",
            1,
        )
        reads.append(path)
        return tex

    g.stub(fn, load)
    g.put(manager_vt + 48, fn)

    # The quest card draws its first two monsters in group 43, elements 3 and 5.
    ui, tree, groups, group, children = [
        g.alloc(n) for n in (96, 680, 44 * 4, 240, 12 * 4)
    ]
    g.put(ui + 36, tree)
    g.put(tree + 664, 44)
    g.put(tree + 248, groups)
    g.put(groups + 43 * 4, group)
    g.put(group + 216, children)
    gui, directory, descriptor, stock_tex = (g.alloc(n) for n in (128, 244, 64, 240))
    g.put(gui + 100, directory)
    g.put(directory + 232, descriptor)
    g.put(directory + 112, 1)
    atlas = 0x1634EFC  # the card's native pair of atlas handles
    g.put(descriptor, g.get(atlas))
    g.put(descriptor + 40, stock_tex)
    widget_vt, refresh = g.alloc(100), g.alloc(4)
    g.put(widget_vt + 80, refresh)
    g.stub(refresh, lambda *_: 1)
    widgets = [g.alloc(300), g.alloc(300)]
    for element, widget in zip((3, 5), widgets):
        g.put(children + 4 * element, widget)
        g.put(widget, widget_vt)
        g.put(widget + 120, gui)

    def picture(widget):
        row = g.get(widget + 260)
        assert row, "no texture descriptor bound"
        return g.get(row + 40), struct.unpack("<4f", g.cpu.mem_read(widget + 268, 16))

    espinas = tex, (0, 0, 128, 128)
    rathalos = stock_tex, (216, 0, 288, 72)
    rathian = stock_tex, (0, 0, 72, 72)
    for raw, expected in ((107, espinas), (1, rathian), (107, espinas)):
        index = g.run(g.hooks[0x560CA4], 0, raw)
        g.run(0x56EF50, ui, 43, 3, index, atlas)
        assert picture(widgets[0]) == expected

    # Run the real card from entry through all five icon slots. Only unrelated
    # panel setup, layout and visibility are stubbed; stop before the map text.
    for addr in (0x612FE8, 0x6135FC, 0x56E528, 0x56E538, 0x54533C):
        g.stub(addr, lambda *_: 0)
    drawn = []
    trap = g.cpu.hook_add(
        UC_HOOK_CODE,
        lambda *_: drawn.append(g.cpu.reg_read(REGS[2])),
        begin=0x56EF50,
        end=0x56EF50,
    )
    for desc, expected in (
        (retail, (rathalos, rathian)),
        (quest, (espinas,)),
        (retail, (rathalos, rathian)),
    ):
        drawn.clear()
        g.run(0x613170, ui, 0, desc, 0, until=0x613384)
        assert drawn == [3, 5][: len(expected)], (g.get(desc + 8), drawn)
        assert tuple(map(picture, widgets[: len(expected)])) == expected
    g.cpu.hook_del(trap)
    assert len(reads) == 1


def verify_travel(g, bank, hde, origin):
    """Run GU's root sampler and its before/after world-position sync at the quest spawn."""
    word = lambda data, p: struct.unpack_from("<I", data, p)[0]
    bits = lambda v: struct.unpack("<I", struct.pack("<f", v))[0]
    start = 16 + 32 * word(hde, len(hde) - 20)
    slots = {
        tuple(hde[i : i + 2]): struct.unpack_from("<H", hde, i + 2)[0]
        for i in range(start, start + 32 * word(hde, len(hde) - 16), 32)
    }
    actor, state = g.alloc(5200), g.alloc(1800)
    g.put(actor + 5160, state)
    g.put(actor + 1188, 1)
    g.put(actor + 28, bits(1.0))
    g.put(actor + 1292, bits(1.0))
    g.cpu.mem_write(actor + 96, struct.pack("<3f", 1, 1, 1))
    assert origin != (0, 0, 0)

    # Only the imported sinf/cosf are absent from the ELF.
    for addr, fn in ((0x13EFDE8, math.sin), (0x13EFDF4, math.cos)):

        def trig(*_, fn=fn):
            angle = struct.unpack(
                "<f", struct.pack("<I", g.cpu.reg_read(arm.UC_ARM_REG_S0))
            )[0]
            g.cpu.reg_write(arm.UC_ARM_REG_S0, bits(fn(angle)))

        g.stub(addr, trig)

    for request, takeoff in (((3, 2), None), ((3, 17), 76), ((252, 116), 36)):
        motion = slots[request]
        assert motion >> 8 == 1
        clip = word(bank, 8 + 4 * (motion & 255))
        track, count, length, loop = struct.unpack_from("<4I", bank, clip)
        roots = [
            i
            for i in range(track, track + count * 36, 36)
            if bank[i + 1 : i + 4] == bytes((4, 0, 255))
        ]
        assert len(roots) == 1
        row = bytearray(bank[roots[0] : roots[0] + 36])
        for field, size in ((12, word(row, 8)), (32, 32)):
            src, ptr = word(row, field), g.alloc(size)
            g.cpu.mem_write(ptr, bank[src : src + size])
            struct.pack_into("<I", row, field, ptr)
        ptr = g.alloc(36)
        g.cpu.mem_write(ptr, bytes(row))
        g.run(0xAFE6B4, actor + 1424, ptr)
        g.put(actor + 1284, bits(length - 1))
        g.put(actor + 1204, motion)
        g.cpu.mem_write(state + 64, struct.pack("<3f", *origin))
        z = origin[2]
        frames = list(range(1, length))
        if takeoff is None:
            assert loop == 24
            frames += list(range(loop, length)) * 2
        for f in frames:
            g.put(actor + 1280, bits(f))
            g.run(0xC27CC, actor)
            g.cpu.reg_write(arm.UC_ARM_REG_S0, bits(0))
            g.cpu.reg_write(arm.UC_ARM_REG_S1, bits(1))
            g.run(0x72170, actor)
            g.run(0xC2844, actor)
            x, y, next_z = struct.unpack("<3f", g.cpu.mem_read(state + 64, 12))
            assert (x, y) == origin[:2]
            if takeoff is None:
                assert 0 <= next_z - z < 50, (request, f, z, next_z)
            elif f >= takeoff:
                assert abs(next_z - z) < 0.01, (request, f, z, next_z)
            z = next_z
        if takeoff is None:
            assert z - origin[2] > 4000


def verify_quest_assets(nx, native):
    archive = assets(nx / "loc/arc/quest/q0010336.arc")
    loose_types = {0x1BBFD18E: ".ext", 0x242BB29A: ".gmd", 0xE0BB1C: ".qdl"}
    for (name, kind), payload in archive.items():
        if kind in loose_types:
            path = (nx / Path(*name.split("\\"))).with_suffix(loose_types[kind])
            assert path.read_bytes() == payload, path
    hub = assets(native / "loc/arc/quest/q0010308.arc")
    hub_data = hub[r"loc\quest\questData\questData_0010308", 0x1BBFD18E]
    assert struct.unpack_from("<H", hub_data, 32)[0] == 2
    assert {p.name for p in (nx / "loc/arc/quest").glob("q*.arc")} == {"q0010336.arc"}
    assert not list(nx.rglob("*0001056*"))
    tower = assets(native / "loc/arc/quest/q0010643.arc")
    data = archive[r"loc\quest\questData\questData_0010336", 0x1BBFD18E]
    assert data[18:20] == hub_data[18:20] == bytes([3, 1])  # stars, low-rank profile
    assert data[20:22] == bytes([17, 0])
    assert (
        data[36:100] == hub_data[36:100]
    )  # no second goal or subquest; low-rank rewards
    assert data[102:113] == hub_data[102:113]
    assert data[113:165] == bytes(52) and data[204:214] == bytes(10)
    assert data[214:219] == bytes((107, 0, 0, 0, 0))
    links = archive[r"loc\quest\questLink\questLink_0010336", 0xE0BB1C]
    linked = [
        (k, name.rstrip(b"\0").decode())
        for k, name in struct.iter_unpack("<I16s", links[8:])
    ]
    for kind, name in linked:
        assert any(
            k == kind and path.endswith(("\\" + name, "questPlus_" + name))
            for path, k in archive
        ), name
    tower_links = tower[r"loc\quest\questLink\questLink_0010643", 0xE0BB1C]
    assert linked[0][1] == "b_m17em107_00" and links[28:168] == tower_links[28:168]
    assert (
        archive[r"quest\boss\setEmMain\b_m17em107_00", 0x2553701D]
        == tower[r"quest\boss\setEmMain\b_m17em011_00", 0x2553701D]
    )
    assert (
        archive[r"quest\supp\supp_0010336", 0x54539AEE]
        == hub[r"quest\supp\supp_0010308", 0x54539AEE]
    )

    def messages(archive, qid, lang):
        value = archive[rf"{lang}\quest\questData\questData_{qid}_{lang}", 0x242BB29A]
        return value[-struct.unpack_from("<I", value, 32)[0] :].split(b"\0")

    for lang in ("chS", "chT", "eng", "fre", "ger", "ita", "spa"):
        actual = messages(archive, "0010336", lang)
        assert actual[0].decode() == {"chS": "棘龙", "chT": "棘龍"}.get(lang, "Espinas")
        assert actual[0] in actual[4]
        assert actual[3] == messages(tower, "0010643", lang)[3]
        assert actual[6] == messages(hub, "0010308", lang)[6]

    reward = archive[r"quest\rem\rem_110107", 0x5B3C302D]
    assert 2980 in {item for item, _, _ in struct.iter_unpack("<HBB", reward[24:])}
    assert linked[8] == (0x5B3C302D, "rem_110107")
    assert (
        archive[r"quest\rem\rem_210083", 0x5B3C302D]
        == hub[r"quest\rem\rem_210083", 0x5B3C302D]
    )
    placements = [
        archive[rf"quest\boss\setEmMain\{name}", kind] for kind, name in linked[:5]
    ]
    return data, placements


def setup_encounter(g, args):
    nx = args.espinas.parent.parent
    installed = assets(nx / "arc/enemy/em107_00.arc")
    vanilla = assets(args.native / "arc/enemy/em001_00.arc")
    assert not (nx / "arc/enemy/em001_00.arc").exists()
    assert all(installed[k] == vanilla[k] for k in installed.keys() & vanilla.keys())
    data, placements = verify_quest_assets(nx, args.native)

    # A resident monster has no zone routes. Compare against the GU encoder's
    # empty representation (same schema for all four classes, no species data).
    resident = assets(args.native / "arc/enemy/ems022_00.arc")
    for route,kind in (("area_link",0x4bf84f6f),("area_eat",0x56d892d0),("area_select",0x15e98d21),
                       ("area_patrol",0x0947f3c8),("map_info",0x197e4d7a)):
        assert installed[rf"enemy\{route}\em107_00",kind] == resident[rf"enemy\{route}\ems022_00",kind]
    # Execute the native stream decoders against the actual shipped EXT and retail QSG.
    # Supply only an already-buffered MT stream and the engine allocators.
    def buffer(obj, src, *_):
        n = g.allocs[src]
        g.put(obj, 0x1787AD8)
        g.put(obj + 8, src)
        g.put(obj + 12, 0)
        g.put(obj + 16, n)
        g.put(obj + 20, n)
        return obj

    g.stub(0x8260D4, buffer)
    g.stub(0x826130, lambda *_: 0)
    g.stub(
        0x13F1564,
        lambda p, value, n, _: g.cpu.mem_write(p, bytes([value & 255]) * n) or p,
    )
    g.stub(0x13EFD7C, lambda p, n, *_: g.cpu.mem_write(p, bytes(n)))
    g.stub(0x13EFDC4, lambda p, n, *_: g.cpu.mem_write(p, bytes(n)))
    g.stub(
        0x13EFE30,
        lambda dest, src, n, _: g.cpu.mem_write(dest, bytes(g.cpu.mem_read(src, n))),
    )
    for p in [0x370778, 0x37A5EC, 0x3914A8]:
        g.stub(p, g.alloc)
    g.stub(0x37A628, lambda *_: 0)

    # Name resource references resolve outside the decoded numeric quest fields.
    def read_name(stream, *_):
        # The packed rGUIMessage reference is a u32 class and a 16-byte name.
        g.put(stream + 12, g.get(stream + 12) + 20)
        return 0

    g.stub(0x2321E8, read_name)
    group_bytes = assets(args.native / "loc/arc/village/common.arc")[
        r"quest\quest_group", 0x61CF79AF
    ]
    group = g.alloc(128)
    src = g.alloc(len(group_bytes))
    g.cpu.mem_write(src, group_bytes)
    assert g.run(g.hooks[0x39BB30], group, src) == 1
    rows = [g.run(0x39BD1C, group, i) for i in range(g.get(group + 100))]
    ids = [g.get(row + 4) for row in rows]
    retail = [row[0] for row in struct.iter_unpack("<IBBB", group_bytes[8:])]
    assert 10336 not in retail and ids == retail + [10336]
    row = rows[-1]
    assert bytes(g.cpu.mem_read(row + 8, 3)) == bytes(3)  # regular, non-key Hub quest
    assert g.run(0x3B8090, MANAGER, 10336, 0) == 1  # Hub counter loader
    for village in (1, 2, 3, 4, 6):
        assert g.run(0x3B8090, MANAGER, 10336, village) == 0
    g.put(QUEST + 180, group)
    flags = g.alloc(3 * 256)
    state = bytes(range(256)) * 3
    g.cpu.mem_write(flags, state)
    g.run(0x523F30, flags, 10336, 1)
    assert g.run(0x523DA4, flags, 10336) == 1
    changed = bytearray(state)
    changed[len(retail) // 8] |= 1 << (len(retail) % 8)
    assert bytes(g.cpu.mem_read(flags, len(state))) == bytes(changed)
    g.run(0x523F30, flags, 10336, 0)
    assert g.run(0x523DA4, flags, 10336) == 0
    script, cache = g.alloc(2304), g.alloc(4)
    g.put(0x188C4C8, script)
    g.put(script + 567 * 4, 1)
    g.put(script + 568 * 4, cache)
    g.put(script + 569 * 4, 1)
    for opened in (0, 1):
        g.put(cache, 10308 | (opened << 31))
        assert g.run(0x3B2174, QUEST, 10336) == g.run(0x3B2174, QUEST, 10308) == opened

    def decode_quest(blob):
        src, resource = g.alloc(len(blob)), g.alloc(128)
        g.cpu.mem_write(src, blob)
        assert g.run(0x39B658, resource, src) == 1
        return g.run(0x39B848, resource, 0)

    desc = decode_quest(data)
    # Independent stock two-monster card for the icon-isolation check.
    stock_card = args.native / "loc/quest/questData/questData_0010316.ext"
    retail_desc = decode_quest(stock_card.read_bytes())
    assert g.get(desc + 8) == 10336
    g.put(QUEST + 164, desc)
    # Run the same Hub 3-star list builder used by the counter. The dummy
    # hunter has the source hunt unlocked; no UI or script interpreter is needed.
    counter, menu = g.alloc(57000), g.alloc(200)
    g.put(0x188AE38, counter)
    g.put(counter + 52, g.alloc(7128))
    # 3BC96C reads the current village from the GU game context at +76.
    g.put(0x188CA48, menu)
    g.cpu.mem_write(menu + 76, bytes([0]))
    g.stub(0x3BB9F0, lambda *_: 0)  # optional daily quest list
    rank = g.cpu.mem_read(desc + 14, 1)[0]
    assert rank == 3
    g.run(0x3BB318, counter, 10336, rank, desc)
    listing = g.run(0x3BC96C, counter, rank)
    assert listing and g.run(0x3BC9F8, counter, rank) == 1 and g.get(listing) == 10336
    for i, position in enumerate(placements):
        src, resource = g.alloc(len(position)), g.alloc(112)
        g.cpu.mem_write(src, position)
        assert g.run(0x39D270, resource, src) == 1
        g.put(QUEST + 5084 + i * 4, g.run(0x39D45C, resource, 0))
    g.run(0x3AADCC, QUEST)
    spawn = struct.unpack("<3f", g.cpu.mem_read(QUEST + 4912, 12))
    assert g.cpu.mem_read(QUEST + 4905, 1)[0] == 1 and spawn == (-777.0, -287.0, 781.0)
    verify_travel(
        g,
        installed[r"enemy\em107\em107_00\mot\em107_00_1", 0x76820D81],
        installed[r"enemy\hit_data\em107_00_attackdata", 0x70BB64BA],
        spawn,
    )
    assert [g.half(QUEST + 4900 + 36 * i) for i in range(5)] == [107, 0, 0, 0, 0]
    # Construct through the actual shared native factory and actor initializer.
    # Only the surrounding base actor allocation/services and scene registration are mocked.
    klass = g.get(0x1731E2C)
    dti = g.alloc(16)
    g.put(klass, dti)
    g.put(dti + 8, 0x4C48C)
    allocator, alloc_vt, alloc_fn = (g.alloc(n) for n in [8, 40, 4])
    g.put(allocator, alloc_vt)
    g.put(alloc_vt + 28, alloc_fn)
    g.put(alloc_vt + 32, alloc_fn)
    g.stub(0x7AA47C, lambda *_: allocator)
    g.stub(alloc_fn, lambda _, size, align, tag: g.alloc(size))

    def base_init(actor, *_):
        g.put(actor + 5160, g.alloc(24200))
        g.put(actor + 12, 1)
        return actor

    g.stub(0xC9CF0, base_init)
    g.stub(0xC067EC, lambda *_: 0)
    actors = {}
    # A separate retail Rathian is only a control for resource isolation.
    for raw in (1, g.half(QUEST + 4900)):
        actor = g.run(0x4EBCC, raw, 0, 0, 0, 0, 0)
        assert actor and g.get(actor) == 0x179ACF8 and g.get(actor + 46576) == raw
        assert bytes(g.cpu.mem_read(actor + 46580, 4)) == bytes((raw, 0, 0, 0))
        actors[raw] = actor
    rathian, espinas = actors[1], actors[107]
    assert rathian != espinas and g.run(0x4ED10, 1) == g.run(0x4ED10, 107)
    # Feed the resource manager both ARC directories simultaneously, keyed just as GU
    # does. Native callbacks must bind the HDE, HTS, model and motion of each actor.
    as_key = lambda name, kind: (~zlib.crc32(name.encode()) & 0xFFFFFFFF, kind)
    resources = {as_key(*k): g.alloc(160) for k in vanilla.keys() | installed.keys()}
    mgr, vt, lookup = [g.alloc(n) for n in [16, 64, 4]]
    g.put(0x2126A64, mgr)
    g.put(mgr, vt)
    g.put(vt + 56, lookup)
    private_only = False
    stock_only = {as_key(*key) for key in vanilla.keys()-installed.keys()}
    def resolve(mgr, fn, crc, kind):
        assert not private_only or (crc,kind) not in stock_only, (hex(crc),hex(kind))
        return resources.get((crc,kind), 0)
    g.stub(lookup, resolve)
    g.stub(0x887648, lambda *_: 0)
    g.stub(0x887660, lambda *_: 0)
    for raw, actor in actors.items():
        tag = f"em{raw:03}"
        key = rf"enemy\dt_tune\{tag}_00_dttune", 0x48E8AC29
        blob = (vanilla if raw == 1 else installed)[key]
        tune = g.alloc(296)
        resources[as_key(*key)] = tune
        # All requested bytes are buffered, including the final word.
        src = g.alloc(len(blob) + 1)
        g.cpu.mem_write(src, blob)
        assert g.run(0x57740, tune, src) == 1
        rage = struct.unpack("<hh3f", g.cpu.mem_read(g.get(tune + 140), 16))
        expected = (700, 10800, 1.3, 1.5, 1.) if raw == 107 else (450, 4200, 1.15, 1.25, 1.)
        assert all(math.isclose(a, b, rel_tol=1e-6) for a,b in zip(rage, expected)), (raw, rage)
        loader = actor + 30284
        g.put(loader + 4, raw)
        g.put(loader + 8, actor)
        g.put(loader + 84, 0x709A0)
        g.put(loader + 100, 0x709C0)
        g.put(loader + 148, 0x70A1C)
        got = {}
        for slot, idx in [(12, 0), (44, 1)]:
            cb = g.alloc(4)

            def capture(a, resource, *_, idx=idx, got=got):
                got[idx] = resource if idx == 0 else g.get(resource)

            g.stub(cb, capture)
            g.put(loader + slot, cb)
        sound_cb = g.alloc(4)
        def bind_sound(_, mss, requests, _unused):
            got[2], got[3] = mss, requests
        g.stub(sound_cb, bind_sound)
        g.put(loader + 60, sound_cb)
        private_only = raw == 107
        assert g.run(0x4A858, loader) == 1
        private_only = False
        for address, path, kind in [
            (got[2], rf"sound\call\em\{tag}", 0x10EBE843),
            (got[3], rf"sound\se\em\{tag}_00\snd_{tag}_00", 0x1BCC4966),
            (got[0], rf"enemy\{tag}\{tag}_00\mod\{tag}_00", 0x58A15856),
            (got[1], rf"enemy\{tag}\{tag}_00\mot\{tag}_00_0", 0x76820D81),
            (g.get(actor + 30164), rf"enemy\hit_data\{tag}_00_attackdata", 0x70BB64BA),
            (g.get(actor + 30172), rf"enemy\hit_data\{tag}_00_hitsize", 0x5653C1B0),
            (g.get(actor + 30188), *key),
        ]:
            assert address == resources[as_key(path, kind)], (tag, path, hex(address))
        assert (
            g.run(0x152960, 0, 30686)
            == resources[as_key(r"enemy\hit_data\em001_00_attackdata", 0x70BB64BA)]
        )
        assert bytes(g.cpu.mem_read(tune + 224, 20)) == blob[-20:]
        stock_row = bytes([100]) * 10
        g.run(0xBABF4, actor)
        for part in range(8):
            address = g.run(0xBAB60, actor, part)
            assert address == g.get(tune + 112) + 10 * part
            if part == 0:
                assert list(g.cpu.mem_read(address, 3)) == (
                    [90, 80, 70] if raw == 1 else [15, 20, 40]
                )
            if part == 7:
                assert bytes(g.cpu.mem_read(address, 10)) == stock_row
        if raw == 1:
            assert g.get(tune + 116) == 0
        else:
            assert list(g.cpu.mem_read(g.get(tune + 116), 3)) == [70, 40, 30]
            assert bytes(g.cpu.mem_read(g.get(tune + 116) + 70, 10)) == stock_row
    # Combat hooks reject a Rathian even if it is given Espinas's full HDE.
    fallback = g.alloc(4)
    g.stub(0xCF5F64, lambda *_: fallback)
    g.put(rathian + 30164, g.get(espinas + 30164))
    assert g.run(g.hooks[0xCF5F64], rathian, 0) == fallback
    # No detours on the retail shell selector/initializer. The combat test
    # exercises the private factory with the entire native shell cache empty.
    assert 0xD0CA90 not in g.hooks and 0x3F97EC not in g.hooks
    assert all("em001" not in path and b"em001" not in payload
               for (path, _), payload in installed.items())
    preload = installed[r"arc\shell\em\em107_00_shell", 0x73850d05]
    published = {as_key(n, k) for n,k in installed}
    assert set(struct.iter_unpack("<II",preload[12:])) <= published
    stem = r"shell\em\em107_00_shell00\em107_00_00"
    descriptor = installed[stem, 0x4aa69872]
    import re
    paths = re.findall(rb"(?:shell|sound|effect)\\[A-Za-z0-9_\\]+", descriptor)
    assert paths and all(any(n == dep.decode() for n,k in installed) for dep in paths)
    damage = installed[stem+"_hitdata", 0x4eb68cee]
    spatial = installed[stem+"_hitsize", 0x5653c1b0]
    g.stub(0x16b920, lambda *_: 1)  # contact sequence allocator
    for i, (duration, radius) in enumerate(((66,150.),(10,200.))):
        row = damage[16+56*i:72+56*i]
        assert struct.unpack_from("<4H",row) == (1,duration,75,1)
        assert (row[16],row[25],row[29]) == (6,i,40)
        at = struct.unpack_from("<I",spatial,20+8*i)[0]
        assert struct.unpack_from("<f",spatial,at+12)[0] == radius
        # GU's real HDS-to-contact initializer, independent of the baker.
        hds, shell, contact, geometry = (g.alloc(sz) for sz in (56,5800,196,160))
        g.cpu.mem_write(hds,row)
        g.run(0x168e98, contact, 5, shell, hds, geometry, i, 23, 2)
        assert struct.unpack("<2f",g.cpu.mem_read(contact+92,8)) == (1.,float(duration))
        assert (g.half(contact+56), g.get(contact+72), g.cpu.mem_read(contact+86,1)[0],
                g.get(contact+136)) == (75,6,40,i)

    # Run the actual 11:16 program selector and verify the tracks it plays.
    state = g.get(espinas + 5160)
    g.cpu.mem_write(state + 942, struct.pack("<H",6))
    g.run(0x9a828, espinas, until=0x76afc)
    group, slot = (g.cpu.reg_read(r) for r in REGS[1:3])
    assert (group,slot) == (11,16)
    g.cpu.mem_write(state+942, bytes(2))
    g.cpu.mem_write(espinas + 29664, bytes([group,slot]))
    g.run(0xcf1550, espinas, until=0xa134c)
    program = g.cpu.reg_read(REGS[1])
    model = installed[r"enemy\em107\em107_00\mod\em107_00",0x58a15856]
    joints = struct.unpack_from("<H",model,6)[0]
    mapping = struct.unpack_from("<I",model,36)[0] + 152*joints
    for i,want in enumerate((782,19)):
        assert g.get(program+8*i) == 0
        motion = g.get(g.get(program+8*i+4))
        assert motion == want
        bank = installed[rf"enemy\em107\em107_00\mot\em107_00_{motion>>8}",0x76820d81]
        clip = struct.unpack_from("<I",bank,8+4*(motion&255))[0]
        track,count,frames,loop = struct.unpack_from("<4I",bank,clip)
        animated = {model[mapping + bone] for j in range(count) if (bone:=bank[track+36*j+3]) != 255}
        assert animated == set(range(48)) and frames > 1
        assert (loop < frames) == (i == 1)
    # The other trapped-capture entry ordinarily shares 3:17 with a kill.
    # Invoke the compiled override, preserving the interpreter's final command.
    g.cpu.mem_write(espinas+29664, bytes((11,17)))
    g.run(g.hooks[0xa134c], espinas, program, 0, until=0xa134c)
    alternate = g.cpu.reg_read(REGS[1])
    assert g.get(g.get(alternate+4)) == 0x314 and g.get(alternate+8) == 7 and g.get(alternate+16) == 255
    g.cpu.mem_write(espinas+29664, bytes(2))
    # Decode retail rItemData and run the shipping material adapter. Added
    # items keep their own sort and save IDs.
    items_blob = (args.native / "table/itemData.itm").read_bytes()
    src = g.alloc(len(items_blob))
    g.cpu.mem_write(src, items_blob)
    g.stub(0x1848F8, g.alloc)
    item_table = g.alloc(128)
    assert g.run(g.hooks[0x192340], item_table, src) == 1
    game_items = g.get(item_table + 108)
    assert g.get(item_table + 100) == 2991
    declared = range(2980,2987)
    # mhfdat +252: original FZ items 1046..1052, 36 bytes each.
    prices = [1750,2000,3500,10000,4200,10000,4200]
    for key,sell in zip(declared,prices):
        target = g.run(0x192530, item_table, key)
        assert target == game_items + key * 56 and g.get(target + 8) == key
        assert g.half(target + 16) == struct.unpack_from("<H", items_blob, 8 + 44 * key + 7)[0]
        assert bytes(g.cpu.mem_read(target+13,2)) == bytes((4,99))
        assert (g.get(target+28),g.get(target+32)) == (sell,sell*10)
        assert not any(g.cpu.mem_read(target+38,16))  # no borrowed crafting categories

    def drops(data):
        return {row[2] for row in struct.iter_unpack("<BBH", data[26:])}

    low = installed[r"enemy\hagi\hagi_s1_em107_00", 0x33A84E14]
    carve_rows = list(struct.iter_unpack("<BBH", low[26:]))
    tail = low[8]
    assert carve_rows[tail:tail+4] == [(50,1,2984),(35,1,2980),(15,1,2981),(255,255,0)]
    assert carve_rows[low[5]:low[5]+6] == [(45,1,2980),(25,1,2981),(14,1,452),(4,1,2982),(4,1,2983),(8,1,432)]
    assert carve_rows[low[14] + 1][2] == 2980  # dropped scale
    assert not (
        set(declared) & drops(vanilla[r"enemy\hagi\hagi_s1_em001_00", 0x33A84E14])
    )
    item_gmd, tag = g.alloc(160), g.alloc(16)
    g.cpu.mem_write(tag, b"itemData\0")
    g.put(item_gmd + 140, tag)
    for lang, label in ((1, "Espinas Scale"), (8, "棘龙的上鳞"), (7, "棘龍的上鱗")):
        g.put(item_gmd + 104, lang)
        assert g.string(g.run(g.hooks[0xB84114], item_gmd, 5960)) == label
        assert g.string(g.run(g.hooks[0xB84114], item_gmd, 5961))
    verify_icon(g, nx, mgr, vt, desc, retail_desc)
    gmd = g.alloc(160)
    index = g.run(g.hooks[0x55BDA4], 0, 107)
    for lang, name in [(1, "Espinas"), (7, "棘龍"), (8, "棘龙")]:
        g.put(gmd + 104, lang)
        assert g.string(g.run(g.hooks[0xB84114], gmd, index)) == name
    assert g.run(g.hooks[0x55BDA4], 0, 1) == 0
    clock = g.alloc(112)
    g.put(0x2126764, clock)
    g.put(clock + 56, 0x42700000)  # quest timing, 60 FPS
    # Stop immediately after 3ABC00 has initialized all three goals; the rest
    # configures unrelated map/weather services.
    g.run(0x3ABC00, QUEST, until=0x3AC0B8)
    assert (g.get(QUEST + 236), g.half(QUEST + 240), g.half(QUEST + 242)) == (9, 107, 1)
    assert g.get(QUEST + 248) == g.get(QUEST + 260) == 0
    print(
        "OK: Hub 3-star solo card, seven titles, one tower spawn, no intruder, stock resources and icons isolated"
    )
    return espinas


def verify(args, reverse, captured=False):
    g = Game(args.elf.read_bytes())
    mods = [(args.registry, 0x5000000), (args.espinas, 0x6000000)]
    if reverse:
        mods.reverse()
    for path, base in mods:
        ex = g.load(path, base)
        if path == args.espinas:
            assert "forge_onUpdate" not in ex
        g.run(ex["forge_onLoad"], MANAGER)
        g.run(ex["forge_onInit"], MANAGER)
        if path == args.espinas:
            assert (0xCF5F64 in g.hooks) != reverse
            assert (0x39BB30 in g.hooks) != reverse
    register, contains, counts = service(g)
    if reverse:
        assert g.run(contains, 107) == 0
        assert sum("Unavailable" in msg for _, msg in g.logs) == 1
        print(
            "OK: late provider leaves the new encounter unavailable and all stock quests untouched"
        )
        return
    assert g.run(contains, 107) == 1
    p = g.alloc(64)
    for key, raw, status in [
        (b"frontier:espinas", 107, 0),
        (b"no-namespace", 108, 3),
        (b"another:wyvern", 1, 1),
        (b"another:wyvern", 107, 2),
        (b"frontier:espinas", 108, 2),
        (b"another:wyvern", 108, 0),
    ]:
        g.cpu.mem_write(p, key)
        assert g.run(register, raw, p, len(key), 1) == status
    assert g.run(0x50B64, 107) == g.run(0x50B64, 1) == 0
    assert g.run(0x50B64, 109) == 0xFFFFFFFF and g.run(0x50B64, 0x201) == 1
    assert g.run(register, 108, p, len(key), 107) == 4
    assert g.run(register, 108, p, len(key), 2) == 2

    def select(slot):
        g.put(MANAGER + 60, 14)
        g.put(MANAGER + 64, slot)
        g.run(g.hooks[0x3E21B8], MANAGER)

    def save(success, slot=0):
        g.write_success = success
        g.run(g.hooks[0x3E0980], MANAGER, slot)
        g.put(MANAGER + 60, 21)
        g.run(g.hooks[0x3E1308], MANAGER)

    select(0)
    espinas = setup_encounter(g, args)
    g.put(QUEST + 224, 10336)
    g.cpu.mem_write(QUEST + 4820, struct.pack("<IHB", 0, 107, 1))
    if captured:
        g.cpu.mem_write(espinas + 29664, bytes([11, 16]))
    assert g.run(0x81640, espinas) == int(captured)
    g.stub(g.get(0x179ACF8 + 448), lambda *_: 0)
    native = lambda: bytes(g.cpu.mem_read(QUEST + 272, 137 * 8))
    h = g.hooks[0x3A75C8]
    check = g.hooks[0x3A3930]
    rows = native()
    g.run(0x3A6108, QUEST, espinas, 0)
    assert g.cpu.mem_read(QUEST + 92, 1)[0] == 1
    assert g.cpu.mem_read(QUEST + 4826, 1)[0] == 0
    assert g.get(QUEST + 236) & 32 and g.half(QUEST + 242) == 0
    assert (
        native() == rows
        and g.run(check, QUEST, 107) == 1
        and g.run(check, QUEST, 1) == 0
    )
    assert g.run(h, QUEST, 109, 0, 100, 1) == 0 and native() == rows
    g.run(g.hooks[0x38D990], 0x7031000)
    assert g.half(PLAYER + 2110) == g.half(PLAYER + 1836) == 0
    assert g.run(g.hooks[0x523104], PLAYER + 1836, 107) == 1
    assert g.run(g.hooks[0x526DD4], PLAYER + 1836, 107) == int(not captured)
    assert g.run(g.hooks[0x526E44], PLAYER + 1836, 107) == int(captured)
    if captured:
        print("OK: capture alone clears the hunt and credits one Espinas capture")
        return
    assert g.run(counts, 107) == 1
    for callback in (0x526D94, 0x526E00, 0x526EB0, 0x526EF8, 0x526F48):
        assert g.run(g.hooks[callback], PLAYER + 1836, 107, 20) == 0
    assert g.half(PLAYER + 1836) == 0 and g.half(PLAYER + 2110) == 0
    save(False)
    assert not g.files
    save(True, 1)  # A write for another slot cannot consume this hunter's records.
    assert not g.files
    save(True)
    contents = list(g.files.values())
    assert len(contents) == 1 and b"frontier:espinas\t1\t0\n" in contents[0]
    assert all(name.endswith(".tsv") for name in g.files)
    select(1)
    assert g.run(counts, 107) == 0
    select(0)
    assert g.run(counts, 107) == 1 and not g.io_mount
    # A valid generation survives an interrupted write to the other file.
    g.cpu.mem_write(QUEST + 236, struct.pack("<IHHHH", 9 | 0x80, 107, 1, 1, 0))
    assert g.run(h, QUEST, 107, 1, 100, 1) == 1
    g.run(g.hooks[0x38D990], 0x7031000)
    save(True)
    newer = next(
        name for name, data in g.files.items() if data.startswith(b"generation\t2\n")
    )
    g.files[newer] = g.files[newer].rsplit(b"# end", 1)[0]
    select(0)
    assert g.run(counts, 107) == 1
    # A kill cannot clear a capture-only objective.
    g.cpu.mem_write(QUEST + 236, struct.pack("<IHHHH", 9 | 0x80, 107, 1, 1, 0))
    assert g.run(h, QUEST, 107, 0, 100, 1) == 0
    assert g.get(QUEST + 236) & 64 and g.half(QUEST + 242) == 1
    verify_all_targets(g)
    print(
        "OK:",
        "provider first",
        "solo slay clears the hunt, all-target goal, native/extended totals, save failure, TSV reload, two slots",
    )


def verify_registration_failure(args, worker=False):
    g = Game(args.elf.read_bytes())
    registry = g.load(args.registry, 0x5000000)
    g.run(registry["forge_onLoad"], MANAGER)
    g.run(registry["forge_onInit"], MANAGER)
    register, _, _ = service(g)
    if worker:
        g.thread = MANAGER + 0x900
    else:
        key = b"another:wyvern"
        p = g.alloc(len(key))
        g.cpu.mem_write(p, key)
        assert g.run(register, 107, p, len(key), 1) == 0
    attempts = []
    g.cpu.hook_add(
        UC_HOOK_CODE, lambda *_: attempts.append(1), begin=register, end=register
    )
    espinas = g.load(args.espinas, 0x6000000)
    g.run(espinas["forge_onLoad"], MANAGER)
    g.run(espinas["forge_onInit"], MANAGER)
    if worker:
        assert not attempts and 0xCF5F64 not in g.hooks
        print("OK: registry unavailable off the game thread")
        return
    assert len(attempts) == 1 and 0x39BB30 not in g.hooks and 0xCF5F64 not in g.hooks
    assert sum("Conflict" in msg for _, msg in g.logs) == 1
    print("OK: conflicting declaration attempted and reported exactly once")


def verify_dps(args, before_espinas):
    """Run the installed DPS NRO's deferred instruction checks against actual mod hook sites."""
    g = Game(args.elf.read_bytes())

    def forge_hook(ret, addr, detour, saved):
        # Only test installation here. No patched gameplay routine is executed.
        assert addr not in g.hooks, hex(addr)
        g.hooks[addr] = detour
        g.put(saved, addr)
        g.cpu.mem_write(addr, struct.pack("<II", 0xE51FF004, detour))
        g.cpu.mem_write(ret, bytes(24))
        return ret

    host = {"forge_hook_create": forge_hook, "forge_fs_mountSaveData": lambda *_: 0}

    def start(path, base):
        exports = g.load(path, base, host)
        g.run(exports["forge_onLoad"], MANAGER)
        g.run(exports["forge_onInit"], MANAGER)
        return exports

    # A main-menu update: no local hunter, no rendering or real settings file.
    g.stub(0x3F8398, lambda *_: 0)
    g.put(0x1885B58, g.alloc(48))
    start(args.registry, 0x5000000)

    def dps():
        keys = set(g.hooks)
        assert g.get(0xBAB60) == 0xE3510008
        exports = start(args.dps, 0x5400000)
        g.run(exports["forge_onUpdate"], MANAGER)
        assert set(g.hooks) - keys == {0x9D6F8, 0x170968}
        assert any("hooks installed" in m for _, m in g.logs)
        assert not any(
            "hooks skipped" in m or "installation failed" in m for _, m in g.logs
        )

    if before_espinas:
        dps()
    start(args.espinas, 0x6000000)
    assert 0xCF5F64 in g.hooks and 0xBAB60 not in g.hooks
    if not before_espinas:
        dps()
    assert g.get(0xBAB60) == 0xE3510008
    print("OK: DPS hooks installed", "before" if before_espinas else "after", "Espinas")


if __name__ == "__main__":
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--elf", type=Path, default=REPO / "mhgu/main.elf")
    cli.add_argument("--native", type=Path, required=True, help="GU RomFS nativeNX directory")
    for key, name in [("registry", "monster-registry"), ("espinas", "monster-espinas")]:
        cli.add_argument(
            "--" + key,
            type=Path,
            default=REPO
            / "plugins"
            / name
            / "dist"
            / name
            / "romfs/nativeNX/plugins"
            / (name.replace("-", "_") + ".nro"),
        )
    cli.add_argument(
        "--dps", type=Path, help="also exercise the optional mhgu-dps-meter v1.2.0 NRO"
    )
    opts = cli.parse_args()
    for reverse in (False, True):
        verify(opts, reverse)
    verify(opts, False, captured=True)
    verify_registration_failure(opts)
    verify_registration_failure(opts, worker=True)
    if opts.dps:
        for first in (False, True):
            verify_dps(opts, first)
