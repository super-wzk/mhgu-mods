"""棘龙选招指令的离线移植实验；不安装 MOD，也不修改游戏文件。

在装有 unicorn==2.1.4 的 Python 环境中运行本文件。使用工作区内的
mhfz/eboot.bin.elf（Vita）和 mhgu/main.elf（Switch）。地址只对应这两个样本。

只抽取一条子脚本调用和其中的一条动作请求。未翻译外围条件；FZ 动作指令
末字节在本例为零，不推断其通用含义。用模拟的文件、堆和怪物实例执行 GU
原版加载器和解释器，在进入原生动作函数时停止，不会播放动画或执行判定。
"""
from pathlib import Path
import argparse
import struct

from unicorn import Uc, UC_ARCH_ARM, UC_MODE_ARM, UC_HOOK_CODE
from unicorn.arm_const import (
    UC_ARM_REG_LR, UC_ARM_REG_PC, UC_ARM_REG_R0, UC_ARM_REG_R1, UC_ARM_REG_R2,
    UC_ARM_REG_R4, UC_ARM_REG_SP,
)

from game_files import GU_ELF, VITA_ELF, elf


def source_fragment():
    data, segments = elf(VITA_ELF)

    def read(addr, n):
        offset = next(off + addr - va for _, off, va, _, size, *_ in segments
                      if va <= addr and addr + n <= va + size)
        return data[offset:offset + n]

    def word(addr):
        return int.from_bytes(read(addr, 4), "little")

    # Bank 0, Espinas (80), main script 3. 13 / 35 00 precede the call.
    tables = word(word(0x82BB0A80) + 80 * 4)
    main = word(word(tables) + 3 * 4)
    assert read(main, 3) == bytes.fromhex("13 35 00")
    call = read(main + 3, 2)
    assert call == bytes.fromhex("81 01")

    sub = word(word(tables + 4) + 4 * call[1])
    assert read(sub, 2) == bytes.fromhex("09 00")
    action = read(sub + 2, 4)
    assert action == bytes.fromhex("05 02 01 00")
    print(f"FZ Espinas: {main + 3:#x}: {call.hex(' ')}; {sub + 2:#x}: {action.hex(' ')}")
    return call[1], action[1], action[2]


def make_fixture(slot, group, number):
    end = bytes([0xff])
    call = bytes((0x14, slot))
    action = bytes((0x00, group, number))
    groups = [[] for _ in range(21)]
    groups[0] = [call + end]
    groups[1] = [end] * slot + [action + end]

    blob = bytearray(struct.pack("<4sIII", b"EMC\0", 0x20120804, 21, 0)) + bytearray(16 * 21)
    for i, scripts in enumerate(groups):
        table = len(blob)
        struct.pack_into("<II", blob, 16 + i * 16, len(scripts), table)
        blob.extend(bytes(16 * len(scripts)))
        for j, script in enumerate(scripts):
            struct.pack_into("<II", blob, table + j * 16, len(script), len(blob))
            blob.extend(script + bytes(-len(script) % 16))
    return bytes(blob), call, action


def verify(emc_path=None):
    slot, group, number = source_fragment()
    reference, call, action = make_fixture(slot, group, number)
    payload = emc_path.read_bytes() if emc_path else reference
    if emc_path:
        assert payload == reference, "EMC differs from the independently assembled Espinas call/action example"
    game, segments = elf(GU_ELF)
    cpu = Uc(UC_ARCH_ARM, UC_MODE_ARM)
    limit = max(va + size for _, _, va, _, _, size, *_ in segments)
    cpu.mem_map(0, (limit + 4095) & ~4095)
    for _, off, va, _, file_size, *_ in segments:
        cpu.mem_write(va, game[off:off + file_size])

    scratch = 0x5000000
    cpu.mem_map(scratch, 0x200000)
    stream, stream_vt, heap, heap_vt, resource = (scratch + k * 0x1000 for k in range(2, 7))
    actor, actor_vt, status = (scratch + k for k in (0x20000, 0x3A000, 0x40000))
    cursor, stack = scratch + 0x80000, scratch + 0x1E0000
    stop, noop, size, read, allocate, free = (scratch + k for k in range(0x100, 0x118, 4))

    def put(addr, value):
        cpu.mem_write(addr, struct.pack("<I", value))

    def get(addr):
        return int.from_bytes(cpu.mem_read(addr, 4), "little")

    for addr, value in (
        (stream, stream_vt), (stream_vt + 76, size), (stream_vt + 52, read),
        (heap, heap_vt), (heap_vt + 28, allocate), (heap_vt + 52, free),
        (0x18A6308, heap), (resource, 0x1732ED4), (actor, actor_vt),
        (actor_vt + 776, noop), (actor + 0x1428, status), (actor + 30196, resource),
    ):
        put(addr, value)
    cpu.mem_write(status + 0x230, b"\x07")

    def alloc(n):
        nonlocal cursor
        addr = cursor
        cursor += (n + 15) & ~15
        assert cursor < stack - 0x10000
        return addr

    reg, setreg = cpu.reg_read, cpu.reg_write
    fetched, requests = [], []

    def ret(value=0):
        setreg(UC_ARM_REG_R0, value)
        setreg(UC_ARM_REG_PC, reg(UC_ARM_REG_LR))

    def on_instruction(_, addr, insn_size, user_data):
        if addr in (noop, free, 0xC46C8, 0x496F0):  # outside the commands under test
            ret()
        elif addr == size:
            ret(len(payload))
        elif addr == read:
            assert reg(UC_ARM_REG_R2) == len(payload)
            cpu.mem_write(reg(UC_ARM_REG_R1), payload)
            ret(len(payload))
        elif addr == allocate:
            ret(alloc(reg(UC_ARM_REG_R1)))
        elif addr == 0x7AF3A0:
            ret(alloc(reg(UC_ARM_REG_R0)))
        elif addr == 0x7AA47C:
            ret(heap)
        elif addr == 0x13EFE30:  # memcpy
            dst = reg(UC_ARM_REG_R0)
            cpu.mem_write(dst, bytes(cpu.mem_read(reg(UC_ARM_REG_R1), reg(UC_ARM_REG_R2))))
            ret(dst)
        elif addr == 0x83770:    # genuine interpreter fetch
            fetched.append(get(reg(UC_ARM_REG_R4)))
        elif addr == 0x75574:    # observe the request before executing the native action
            requests.append(tuple(reg(r) for r in (UC_ARM_REG_R0, UC_ARM_REG_R1, UC_ARM_REG_R2)))
            cpu.emu_stop()
        elif addr == 0:
            raise RuntimeError("Unexpected null program counter")

    cpu.hook_add(UC_HOOK_CODE, on_instruction)

    def run(entry, *args):
        setreg(UC_ARM_REG_SP, stack)
        setreg(UC_ARM_REG_LR, stop)
        for register, value in zip((UC_ARM_REG_R0, UC_ARM_REG_R1, UC_ARM_REG_R2), args):
            setreg(register, value)
        cpu.emu_start(entry, stop, count=60000)
        return reg(UC_ARM_REG_R0), reg(UC_ARM_REG_PC)

    assert run(0x520A0, resource, stream) == (1, stop)
    first = get(get(resource + 104))
    second = get(get(resource + 108) + 4 * slot)
    assert bytes(cpu.mem_read(first, len(call))) == call
    assert bytes(cpu.mem_read(second, len(action))) == action
    assert run(0x826B4, actor)[1] == stop and get(actor + 29432) == first
    assert run(0x82F80, actor, 0, scratch + 0x7000)[1] == 0x75574
    assert fetched == [first, second]
    assert requests == [(actor, group, number)]
    print(f"GU: loaded {len(payload)} bytes; executed {call.hex(' ')} -> {action.hex(' ')};"
          f" native request ({group}, {number}) at 0x75574")


if __name__ == "__main__":
    if not __debug__:
        raise SystemExit("Run without -O: this probe relies on assertions.")
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("emc", nargs="?", type=Path, help="mhfz2mhgu emc --monster 80 --entry 0:3 --action-sample 的输出")
    verify(parser.parse_args().emc)
