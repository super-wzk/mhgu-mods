"""Run GU's motion sound dispatcher and bank selector on the shipping Espinas ARC."""

import struct
import sys
from pathlib import Path

from game_files import GU_ELF, REPO, assets

sys.path.insert(0, str(REPO / "plugins/mhgu-sdk-runtime/tests"))
from verify_espinas_quest import Game


def verify(arc):
    a = assets(arc)
    g = Game(GU_ELF.read_bytes())

    def load(data):
        p = g.alloc(len(data))
        g.cpu.mem_write(p, data)
        return p

    mss = a[r"sound\call\em\em107", 0x10EBE843]
    m = load(mss)
    res, ctl, actor = g.alloc(128), g.alloc(620), g.alloc(48000)
    g.put(res + 100, m)
    g.put(res + 104, m + 16)
    g.put(res + 108, m + 1040)
    g.put(ctl + 4, 1)
    g.put(ctl + 8, 1)
    g.put(ctl + 336, res)
    g.put(ctl + 356, actor)
    g.put(ctl + 556, 0xFFFFFFFF)
    g.put(actor + 12, 1)
    g.stub(0x6FF44, lambda *_: 0)  # audible actor
    g.stub(0x4F1974, lambda _manager, owner, cue, bone: seen.append((frame, cue, bone)))
    params = a[r"sound\se\em\em107_00\snd_em107_00", 0x1BCC4966]
    requests, rows = g.alloc(180), load(params)
    g.put(requests + 104, g.get(rows + 8))
    g.put(requests + 108, rows + 52)
    bank = a[r"sound\se\em\em107_00\snd_em107_00", 0x15D782FB]
    ng, nw = struct.unpack_from("<II", bank, 8)
    sb, groups, waves = g.alloc(128), load(bank[20 : 20 + 12 * ng]), g.alloc(84 * nw)
    g.put(sb + 100, ng)
    g.put(sb + 104, groups)
    g.put(sb + 112, waves)
    for i in range(ng):
        p = groups + 12 * i + 4
        g.put(p, waves + 84 * g.get(p))
    p = 20 + 12 * ng
    names = {}
    for i in range(nw):
        e = bank.index(0, p)
        name = bank[p:e].decode()
        names[waves + 84 * i] = name
        g.cpu.mem_write(waves + 84 * i + 4, bank[e + 1 : e + 81])
        p = e + 81
    picked = []
    cb = g.alloc(4)

    def select(row, *_):
        picked.append(names[row])
        return 1

    g.stub(cb, select)

    def waveform(cue):
        row = g.run(0xB68280, requests, cue)
        assert row and g.get(row + 84) == 0, cue
        note = g.cpu.mem_read(row + 2, 1)[0]
        group = g.half(row + 30)
        picked.clear()
        assert g.run(0x876AB0, sb, cb, 0, group, note, 127) == 1 and len(picked) == 1
        name = picked[0]
        assert r"em107_00\wav\fz" in name
        wave = a[name, 0x79C47B59]
        count, rate = struct.unpack_from("<II", wave, 96)
        assert (
            count > 0
            and rate == 48000
            and any(wave[120:])
            and len(wave) >= 120 + ((count + 13) // 14) * 8
        )
        return name

    h = a[r"enemy\hit_data\em107_00_attackdata", 0x70BB64BA]
    d, n = struct.unpack_from("<II", h, len(h) - 20)
    ids = {}
    for i in range(d, d + n):
        offset = 16 + i * 32
        ids[tuple(h[offset : offset + 2])] = struct.unpack_from("<H", h, offset + 2)[0]
    for request, frames in [
        ((255, 39), [(2, 23), (2, 87), (64, 3), (102, 16)]),
        ((0, 41), [(12, 23), (14, 12), (72, 21), (200, 19)]),
        ((3, 3), [(18, 32), (20, 0), (24, 23), (54, 3), (90, 52), (242, 0), (270, 3)]),
        ((3, 1), [(6, 51), (22, 2), (46, 1), (74, 2)]),
        ((252, 115), [(12, 51), (22, 24), (28, 23), (150, 1), (158, 33), (212, 3)]),
    ]:
        seen = []
        g.put(ctl + 80, 0xFFFFFFFF)
        g.put(actor + 1204, ids[request])
        for frame in range(frames[-1][0] + 1):
            g.cpu.mem_write(actor + 1280, struct.pack("<f", frame))
            g.run(0x4D5E90, ctl)
        assert [(f, c - 4096) for f, c, _ in seen] == frames, (request, seen)
        assert all(bone < 255 for _, _, bone in seen)
    custom = [
        g.half(rows + 52 + i * 132)
        for i in range(g.get(rows + 8))
        if 4096 <= g.half(rows + 52 + i * 132) < 4196
    ]
    sounds = {waveform(cue) for cue in custom}
    assert waveform(4096 + 87) in sounds
    print(
        f"OK: native GU dispatch for lie-down, sleeping pose, rage roar, charge and fire breath; {len(custom)} requests resolve to {len(sounds)} Espinas waves."
    )


if __name__ == "__main__":
    verify(
        Path(sys.argv[1])
        if len(sys.argv) > 1
        else Path(__file__).resolve().parents[1]
        / "dist/monster-espinas/romfs/nativeNX/arc/enemy/em107_00.arc"
    )
