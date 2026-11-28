"""Shared ARM32 GU replay harness with in-memory Forge savedata."""

import struct
from pathlib import Path

from unicorn import UC_ARCH_ARM, UC_HOOK_CODE, UC_MODE_ARM, Uc
from unicorn import arm_const as arm

SDK_NRO = Path(__file__).resolve().parents[1] / "dist/mhgu-sdk/romfs/nativeNX/plugins/mhgu-sdk.nro"

REGS = [arm.UC_ARM_REG_R0, arm.UC_ARM_REG_R1, arm.UC_ARM_REG_R2, arm.UC_ARM_REG_R3]
STOP, STACK, HEAP = 0x7380000, 0x73FF000, 0x7200000
QUEST, PLAYER, MANAGER = 0x7010000, 0x7020000, 0x7030000


class Game:
    # Tests may add GU entries whose first ARM instruction is PC-independent.
    inline_hooks = {0x27EBBC, 0x50B64, 0x152960, 0x4A858, 0x3D7E90, 0x3A75C8, 0x3A3178, 0x56EF50, 0x592FE0, 0x593514, 0x6EB9FC, 0x6EB6C0, 0x6EC2CC, 0x6ECE8C, 0x6ED128, 0x6ED200, 0x6F53E4, 0x6F8648, 0x6F9B18, 0x6FCABC, 0x6FB224, 0x6FB364, 0x6ED344, 0x6FBF4C, 0x54F260, 0x6ED600, 0x6FC2C8, 0x6F9438, 0x6FF02C, 0x6FFD40, 0x702D9C, 0x7045DC, 0x6E8558, 0x6F1AE8}
    grid_hooks = {0x143390, 0x1437E0, 0x143A14, 0x14426C, 0x144DAC, 0x144F94,
                  0x1450F0, 0x14526C, 0x1454A8, 0x1456BC, 0x148340, 0x72C7E0,
                  0x72CEC4, 0x26E964, 0x5B90A8, 0x718084, 0x1418E0, 0x14394C, 0x143894,
                     0x144C04, 0x1440C8, 0x144190, 0x141944, 0x144590, 0x1449B4,
                     0x144B68, 0x145B58, 0x144768, 0x14935C, 0x1424EC, 0x149E6C,
                     0x72AA48, 0x72AB38, 0x56EA38, 0x5B973C, 0x5B91E0, 0x5B6C78}
    inline_hooks |= grid_hooks

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
        self.mount_available = True
        self.put(0x188AD90, QUEST)
        self.put(0x1897F78, PLAYER)
        # Run GU's actual startup initializer, including the allocator's owner-thread
        # assignment. Supply only the OS thread/mutex, formatting and atexit services.
        self.thread = MANAGER + 0x800
        for addr in (0x13EFFB0, 0x13EFE00, 0x13EFE0C, 0x13EFF2C, 0x288):
            self.stub(addr, lambda *_: 0)
        self.stub(0x13EFFD4, lambda *_: self.thread)
        self.run(0x7AF404)
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
        if addr in self.inline_hooks or not saved:
            if saved:
                # The Switch Forge loader copies two ARM instructions verbatim.
                # Keep the single-instruction fixture for unrelated hook families.
                size = 8 if addr in self.grid_hooks else 4
                trampoline = self.alloc(size + 8)
                self.cpu.mem_write(
                    trampoline,
                    bytes(self.cpu.mem_read(addr, size))
                    + struct.pack("<II", 0xE51FF004, addr + size),
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
        def delete(path, *_):
            assert self.io_mount
            return 0 if self.files.pop(self.string(path), None) is not None else 1

        def rename(src, dest, *_):
            assert self.io_mount
            src, dest = self.string(src), self.string(dest)
            if src not in self.files or dest in self.files:
                return 1
            self.files[dest] = self.files.pop(src)
            return 0

        self.stub(0x13F0424, delete)
        self.stub(0x13F0430, rename)
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
        if not self.mount_available:
            return 0
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
