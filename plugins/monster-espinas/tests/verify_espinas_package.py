"""Independent reader for mhfz2mhgu package and charge-test (stdlib; optional Pillow for PNG comparison)."""

from pathlib import Path
import argparse, struct as s, zlib

from game_files import VITA_ELF, assets

M, L, R, T, C = 0x58A15856, 0x76820D81, 0x2749C8A8, 0x241F5DEB, 0x3AABBA02


def image(b):
    assert b[:4] == b"TEX\0"
    enc, sz = s.unpack_from("<2I", b, 4)
    m = sz & 63
    w = (sz >> 6) & 8191
    h = sz >> 19
    assert enc == 0x200080A0 and b[12:16] == bytes([1, 7, 1, 0])
    offsets = s.unpack_from("<%dI" % m, b, 20)
    data = b[20 + 4 * m :]
    assert len(data) == s.unpack_from("<I", b, 16)[0]
    top = None
    for i, start in enumerate(offsets):
        wi = max(1, w >> i)
        hi = max(1, h >> i)
        cols = (wi + 15) // 16
        bh = min(16, 1 << (max(1, (hi + 7) // 8) - 1).bit_length())
        end = offsets[i + 1] if i + 1 < m else len(data)
        assert end - start == cols * ((hi + 8 * bh - 1) // (8 * bh)) * bh * 512
        if i == 0:
            top = bytearray()
            for y in range(h):
                for x in range(w):
                    gob = ((y // (bh * 8)) * cols + x // 16) * bh + (y // 8) % bh
                    pos = (
                        gob * 512
                        + ((x & 8) << 5)
                        + ((y & 6) << 5)
                        + ((x & 4) << 3)
                        + ((y & 1) << 4)
                        + ((x & 3) << 2)
                    )
                    top.extend(data[pos : pos + 4])
    return bytes(top)


def check_body(trial, donor, model, donor_bones, elf):
    """Compare every converted shape with the ordinary Vita table, independently of the writer."""
    src = elf.read_bytes()
    ph = s.unpack_from("<I", src, 28)[0]
    stride, count = s.unpack_from("<HH", src, 42)

    def file_offset(va):
        for i in range(count):
            typ, off, addr, _, size = s.unpack_from("<5I", src, ph + i * stride)
            if typ == 1 and addr <= va < addr + size:
                return off + va - addr
        raise ValueError(f"unmapped VA {va:#x}")

    table_va = s.unpack_from("<I", src, file_offset(0x8257DDF4 + 40 * 80))[0]
    p = file_offset(table_va)
    source = [s.unpack_from("<6H7f", src, p + i * 40) for i in range(32)]
    assert source[2][:3] == (125, 1, 1) and source[-1][0] == 0xFFFF
    assert [(v[0], v[4]) for v in source[28:31]] == [(1, 5), (1, 0x300), (1, 0x1800)]
    key = (r"enemy\body_data\em001_00_bodydata", 0x1AC7B52D)
    data = trial[key]
    n = s.unpack_from("<I", data, 8)[0]
    assert n == 28 and len(data) == 16 + 44 * (n + 1)
    joint_table = s.unpack_from("<I", model, 36)[0]
    original = [(i, v) for i, v in enumerate(source[:28]) if i != 2]
    for target, (i, v) in enumerate(original):
        r = s.unpack_from("<8H7f", data, 16 + target * 44)
        logical = model[joint_table + 24 * (donor_bones + v[0])]
        bone_count = s.unpack_from("<H", model, 6)[0]
        assert model[joint_table + 152 * bone_count + logical] == donor_bones + v[0]
        assert r[:5] == (logical, 0xFFFF, v[1], v[2], v[3]), (
            f"FZ body row {i}: binding or part differs"
        )
        assert r[6] == int(i == 3) and r[8:] == v[6:], (
            f"FZ body row {i}: condition or volume differs"
        )
    # The final native actor-level sphere and sentinel survive intact.
    assert data[-88:] == donor[key][-88:]
    return len(original)


def joint_path(lmt, clip, model, logical, endpoints, frames_to_sample=None):
    """Unit actor scale, identity starting transform, flat unobstructed ground."""
    import math

    table, n, frames = s.unpack_from("<3I", lmt, clip)
    tracks = {(lmt[t + 3], lmt[t + 1]): t for t in range(table, table + 36 * n, 36)}

    def sample(bone, usage, frame):
        t = tracks[bone, usage]
        fmt = lmt[t]
        if fmt in (1, 2):
            return s.unpack_from("<4f", lmt, t + 16)
        size, buf = s.unpack_from("<2I", lmt, t + 8)
        assert size == 8 * frames
        pos = buf + 8 * frame
        if fmt == 4:
            x, y, z, delta = s.unpack_from("<4H", lmt, pos)
            mm = s.unpack_from("<I", lmt, t + 32)[0]
            ranges = s.unpack_from("<8f", lmt, mm)
            val = tuple(
                ranges[k + 4] + ranges[k] * v / 65535 for k, v in enumerate((x, y, z))
            )
        elif fmt == 6:
            packed = s.unpack_from("<Q", lmt, pos)[0]
            delta = packed >> 56
            raw = [(packed >> shift) & 0x3FFF for shift in (42, 28, 14, 0)]
            val = tuple((v if v <= 8191 else v - 16383) / 4095.75 for v in raw)
        else:
            raise ValueError(f"unexpected test clip codec {fmt}")
        assert delta == int(frame + 1 < frames)
        return val

    def cross(a, b):
        return (
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        )

    count = s.unpack_from("<H", model, 6)[0]
    joints = s.unpack_from("<I", model, 36)[0]
    remap = joints + 152 * count
    idx = model[remap + logical]
    chain = []
    while idx != 255:
        bone, parent = model[joints + 24 * idx : joints + 24 * idx + 2]
        assert idx < count and bone not in chain and model[remap + bone] == idx
        chain.append(bone)
        idx = parent
    assert chain[0] == logical
    for frame in range(frames) if frames_to_sample is None else frames_to_sample:
        root = sample(255, 4, frame)[:3]
        pts = endpoints
        for bone in chain:
            scale = sample(bone, 2, frame)
            q = sample(bone, 0, frame)
            move = sample(bone, 1, frame)
            assert abs(math.hypot(*q) - 1) < 0.001
            q = tuple(x / math.hypot(*q) for x in q)
            result = []
            for pt in pts:
                v = tuple(x * k for x, k in zip(pt, scale))
                uv = cross(q, v)
                uuv = cross(q, uv)
                result.append(
                    tuple(
                        move[i] + v[i] + 2 * (q[3] * uv[i] + uuv[i]) for i in range(3)
                    )
                )
            pts = result
        yield root, [tuple(x + y for x, y in zip(p, root)) for p in pts]


def check_opening_feet(model, bank, neutral, motions):
    """Skin both soles from the shipping MOD/LMT, at unit scale on a flat floor."""
    from collections import defaultdict

    word = lambda b, at: s.unpack_from("<I", b, at)[0]
    bones, vb = word(model, 36), word(model, 52)
    n = s.unpack_from("<H", model, 6)[0]
    first_skin = 0 if model[bones] == 148 else n - 48
    local, inverse = bones + 24 * n, bones + 88 * n

    def transform(at, xyz):
        m = s.unpack_from("<16f", model, at)
        return tuple(
            m[12 + i] + sum(m[4 * j + i] * xyz[j] for j in range(3)) for i in range(3)
        )

    points = defaultdict(list)
    feet = {}
    for i in range(word(model, 12)):
        p = vb + 28 * i
        joints = model[p + 16 : p + 20]
        foot = joints[0] - first_skin
        if foot not in (20, 21, 22, 26, 27, 28):
            continue
        feet[i] = int(foot >= 26)
        x, y, z, w = s.unpack_from("<4h", model, p)
        a, b = s.unpack_from("<2e", model, p + 24)
        weights = (w / 32767.0, a, b, 1.0 - w / 32767.0 - a - b)
        for joint, weight in zip(joints, weights):
            if weight > 0:
                xyz = transform(
                    inverse + 64 * joint, (x / 32767.0, y / 32767.0, z / 32767.0)
                )
                points[joint].append((i, weight, xyz))
    assert set(feet.values()) == {0, 1}

    def soles(ys):
        return [min(y for v, y in ys.items() if feet[v] == leg) for leg in (0, 1)]

    base_y = defaultdict(float)
    for joint, vertices in points.items():
        for v, weight, xyz in vertices:
            parent = joint
            while parent != 255:
                xyz = transform(local + 64 * parent, xyz)
                parent = model[bones + 24 * parent + 1]
            base_y[v] += weight * xyz[1]
    assert max(map(abs, soles(base_y))) < 3, ("unanimated model", soles(base_y))

    clips = {
        request: (bank, slot & 255)
        for slot, request in motions.items()
        if request in ((255, 39), (255, 34), (0, 41), (0, 42), (255, 37), (255, 62))
    }
    clips["before AI"] = (neutral, 1)
    for request, (lmt, slot) in clips.items():
        clip = word(lmt, 8 + slot * 4)
        frames = word(lmt, clip + 8)
        samples = sorted({0, frames // 2, frames - 1})
        ys = [defaultdict(float) for _ in samples]
        for joint, vertices in points.items():
            logical = model[bones + 24 * joint]
            track = joint_path(lmt,clip,model,logical,[p for _,_,p in vertices],samples)
            for target,(_,positions) in zip(ys,track):
                for (v,weight,_),(_,y,_) in zip(vertices,positions):
                    target[v] += weight*y
        for f, y in zip(samples,ys):
            lowest = min(soles(y))
            assert lowest > -0.5, (request, f, soles(y))
            if request == "before AI" or (request == (255,39) and f == 0):
                assert max(abs(y[v]-base_y[v]) for v in feet) < 0.4


def trace_charge(lmt, clip, model, logical, hde, hts, offset):
    import math

    endpoints = [s.unpack_from("<3f", hts, offset + off) for off in (16, 32)]
    radius = s.unpack_from("<f", hts, offset + 12)[0]
    start, duration = s.unpack_from("<hh", hde)
    rows = []
    for frame, (root, (p0, p1)) in enumerate(
        joint_path(lmt, clip, model, logical, endpoints)
    ):
        assert (
            all(math.isfinite(x) for x in (*root, *p0, *p1)) and math.dist(p0, p1) > 0
        )
        rows.append(
            (frame, int(start <= frame < start + duration), *root, *p0, *p1, radius)
        )
    assert math.dist(rows[0][2:5], (0, 0, 0)) < 0.1
    assert math.dist(rows[-1][2:5], s.unpack_from("<3f", lmt, clip + 16)) < 0.1
    assert math.dist(rows[74][2:5], rows[75][2:5]) < 0.1, (
        "root jumps at the 107/118 join"
    )
    assert sum(row[1] for row in rows) == 49 and not any(row[1] for row in rows[75:])
    return rows


def check_camera(tr, donor, model, lmt, dn):
    key = (r"enemy\dt_tune\em001_00_dttune", 0x48E8AC29)
    cur, old = tr[key], donor[key]
    assert cur[:1430] == old[:1430] and cur[1574:] == old[1574:]
    base = s.unpack_from("<I", model, 36)[0]
    heights = []
    for row in range(1430, 1574, 36):
        assert cur[row + 32 : row + 36] == old[row + 32 : row + 36]
        for off, source in [(0, 3), (16, 30)]:
            logical, dx, dy, dz = s.unpack_from("<I3f", cur, row + off)
            assert logical == model[base + 24 * (dn + source)] and (dx, dy, dz) == (
                0,
                0,
                0,
            )
            for slot in [1, 4]:
                clip = s.unpack_from("<I", lmt, 8 + 4 * slot)[0]
                heights.extend(
                    pts[0][1]
                    for _, pts in joint_path(lmt, clip, model, logical, [(0, 0, 0)])
                )
    assert min(heights) > 0
    camera = tr[r"enemy\cam_body_data\em001_00_cambodydata", 0x1AC7B52D]
    nb = s.unpack_from("<H", model, 6)[0]
    for off in range(16, len(camera) - 44, 44):
        logical = s.unpack_from("<H", camera, off)[0]
        assert dn <= model[base + 152 * nb + logical] < nb
    print(
        f"Both focus blocks / both camera modes: local Y {min(heights):.1f}..{max(heights):.1f} during idle and charge."
    )


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("visual", type=Path)
    p.add_argument("trial", type=Path)
    p.add_argument("donor", type=Path)
    p.add_argument(
        "--png-dir",
        type=Path,
        help="directory with decoded PAC image entries pac_1_0 through pac_1_4",
    )
    p.add_argument(
        "--vita-elf",
        type=Path,
        default=VITA_ELF,
    )
    p.add_argument(
        "--trace",
        type=Path,
        help="write per-frame root and horn positions as CSV (model units)",
    )
    a = p.parse_args()
    v, tr, donor = map(assets, (a.visual, a.trial, a.donor))
    base = next(n for n, k in v if k == M)
    mrl = v[base, R]
    mod = v[base, M]
    nm, nt, _, to, mo = s.unpack_from("<5I", mrl, 8)
    assert (nm, nt) == (22, 6)
    paths = [
        s.unpack_from("<64s", mrl, to + 76 * i + 12)[0].rstrip(b"\0").decode()
        for i in range(nt)
    ]
    for i, name in enumerate(paths):
        data = image(v[name, T])
        if i < 5 and a.png_dir:
            from PIL import Image

            src = a.png_dir / ("pac_1_" + str(int(name.split("_BM")[-1])))
            assert Image.open(src).convert("RGBA").tobytes() == data
    names = s.unpack_from("<I", mod, 44)[0]
    expected = [4, 4, 4, 0, 1, 1, 2, 2, 3, 4, 2, 2, 4, 0, 0, 1, 2, 2, 2, 2, 4, 4]
    for i in range(nm):
        words = s.unpack_from("<15I", mrl, mo + 60 * i)
        assert words[13] % 16 == 0, "unaligned MRL parameter block"
        name = mod[names + i * 128 : names + (i + 1) * 128].rstrip(b"\0")
        assert words[1] == (zlib.crc32(name) ^ 0xFFFFFFFF)
        refs = {
            k: paths[v - 1]
            for j in range(words[6] & 4095)
            for f, v, k in [s.unpack_from("<III", mrl, words[13] + 12 * j)]
            if f & 15 == 3
        }
        assert int(refs[0xCD06F371].split("_BM")[-1]) == expected[i]
    key = next(n for n, k in donor if k == M and n.endswith("mod\\em001_00"))
    tm = tr[key, M]
    dm = donor[key, M]
    order = {k: i for i, k in enumerate(tr)}
    trial_mrl = tr[key, R]
    tmn, ttn, _, tto, tmo = s.unpack_from("<5I", trial_mrl, 8)
    assert (tmn, ttn) == (nm, nt)
    for i in range(ttn):
        path = (
            s.unpack_from("<64s", trial_mrl, tto + 76 * i + 12)[0]
            .split(b"\0")[0]
            .decode()
        )
        assert order[path, T] < order[key, R] < order[key, M]
    assert all(
        s.unpack_from("<I", trial_mrl, tmo + i * 60 + 52)[0] % 16 == 0
        for i in range(tmn)
    )
    dn = s.unpack_from("<H", dm, 6)[0]
    nb = s.unpack_from("<H", tm, 6)[0]
    assert nb == dn + 48
    ob = s.unpack_from("<I", tm, 36)[0]
    remap = tm[ob + 152 * nb : ob + 152 * nb + 256]
    bid = tm[ob + 24 * (dn + 35)]
    assert remap[bid] == dn + 35
    k = next(k for k in tr if k[0].endswith("mot\\em001_00_0") and k[1] == L)
    l = tr[k]
    check_camera(tr, donor, tm, l, dn)
    a4 = s.unpack_from("<I", l, 24)[0]
    track, n, frames, loop = s.unpack_from("<4I", l, a4)
    travel = s.unpack_from("<f", l, a4 + 24)[0]
    assert frames == 234 and loop == 0xFFFFFFFF and travel > 1800
    assert any(
        l[t + 1 : t + 4] == bytes((4, 0, 255))
        and s.unpack_from("<I", l, t + 8)[0] == frames * 8
        for t in range(track, track + n * 36, 36)
    )
    for t in range(track, track + n * 36, 36):
        assert l[t + 3] == 255 or remap[l[t + 3]] >= dn and remap[l[t + 3]] < nb
    h = tr[r"enemy\hit_data\em001_00_attackdata", 0x70BB64BA]
    r = h[16 + 19 * 32 : 16 + 20 * 32]
    assert s.unpack_from("<hh", r) == (26, 49)
    hts = tr[r"enemy\hit_data\em001_00_hitsize", 0x5653C1B0]
    cnt, off = s.unpack_from("<II", hts, 16 + 8 * r[25])
    assert cnt == 3 and s.unpack_from("<3H", hts, off) == (bid, 0, 1)
    for attack in (8, 50):
        group = h[16 + 32 * attack + 25]
        assert s.unpack_from("<I", hts, 16 + 8 * group)[0] == 0, (
            "unexpected second attack"
        )
    schedule = [
        (103, 19, 26, 49, 64, 3),
        (74, 64, 26, 49, 64, 9),
        (82, 65, 77, 140, 35, 2),
        (101, 66, 91, 36, 35, 1),
        (78, 67, 95, 50, 55, 3),
    ]
    src = a.vita_elf.read_bytes()

    def va(addr):
        ph = s.unpack_from("<I", src, 28)[0]
        for j in range(s.unpack_from("<H", src, 44)[0]):
            kind, p, v, _, n = s.unpack_from("<5I", src, ph + 32 * j)
            if kind == 1 and v <= addr < v + n:
                return p + addr - v
        raise ValueError(hex(addr))

    for effect, slot, t, d, power, num in schedule:
        row = h[16 + slot * 32 : 48 + slot * 32]
        rec = s.unpack_from("<40s", src, va(0x82707B5C + 40 * effect))[0]
        assert s.unpack_from("<HHH", row) == (t, d, power)
        assert row[4:8] == rec[4:8] and row[11] == rec[14]
        assert row[16:20] == rec[20:24] and row[29] == rec[24]
        sg = s.unpack_from("<H", rec, 16)[0]
        at = va(s.unpack_from("<I", src, va(0x82C779E8 + 4 * sg))[0])
        volumes = []
        while True:
            fields = s.unpack_from("<6H7f", src, at)
            at += 40
            if fields[0] == 0xFFFF:
                break
            if fields[0] != 125:
                volumes.append(fields)
        gn, go = s.unpack_from("<II", hts, 16 + 8 * row[25])
        assert gn == num == len(volumes)
        for j, source in enumerate(volumes):
            q = go + j * 48
            logical, _, mode, _ = s.unpack_from("<4H", hts, q)
            assert remap[logical] == dn + source[0] and mode == source[1]
            assert (
                s.unpack_from("<f", hts, q + 12)
                + s.unpack_from("<3f", hts, q + 16)
                + s.unpack_from("<3f", hts, q + 32)
            ) == source[6:]
        if effect == 74:
            sn, so = s.unpack_from("<II", hts, 16 + 8 * (row[25] + 1))
            assert sn == gn - 1 and hts[so : so + 48 * sn] == hts[go : go + 48 * sn]
    assert (
        max(sum(t <= f < t + d for _, _, t, d, _, _ in schedule) for f in range(frames))
        == 3
    )
    shapes = check_body(tr, donor, tm, dn, a.vita_elf)
    rows = trace_charge(l, a4, tm, bid, r, hts, off)
    if a.trace:
        import csv

        with a.trace.open("w", newline="") as f:
            writer = csv.writer(f)
            writer.writerow(
                "frame,primary_attack_enabled,root_x,root_y,root_z,horn_a_x,horn_a_y,horn_a_z,horn_b_x,horn_b_y,horn_b_z,horn_radius,active_fz_effects".split(
                    ","
                )
            )
            writer.writerows(
                (
                    *r,
                    "|".join(
                        str(e) for e, _, t, d, _, _ in schedule if t <= r[0] < t + d
                    ),
                )
                for r in rows
            )
    cmd = tr[r"enemy\cmd_tbl\em001_00_cmdtbl", C]
    cnt, off = s.unpack_from("<2I", cmd, 16)
    for i in range(cnt):
        n, p = s.unpack_from("<II", cmd, off + 16 * i)
        assert cmd[p : p + n] == bytes.fromhex("00 01 00 ff")
    assert donor.keys() <= tr.keys()
    print(
        f"Checked {nm} material bindings, {nt} textures, {len(tr)} overlay entries; {frames} charge frames, root travel Z={travel:.2f}, horn logical ID {bid}, {shapes} FZ body volumes; five source melee instances (peak overlap: 3)."
    )


if __name__ == "__main__":
    main()
