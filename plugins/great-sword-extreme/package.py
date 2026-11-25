"""Build the GU 1.4 Forge overlay from the user's Frontier data."""

import argparse
import os
import shutil
import struct
import subprocess
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parent
CONVERTER = ROOT.parents[1] / "tools" / "mhfz2mhgu"


def package(dat, romfs):
    assets = ROOT / "target/assets"
    assets.mkdir(parents=True, exist_ok=True)
    subprocess.run(
        ["cargo", "build", "--release", "--manifest-path", CONVERTER / "Cargo.toml"],
        check=True,
    )
    tool = CONVERTER / "target/release" / CONVERTER.name
    motions = assets / "motions.arc"
    subprocess.run(
        [tool, "great-sword-motions", dat, "-o", motions],
        check=True,
    )
    subprocess.run(
        [tool, "great-sword-actions", dat, "-o", assets / "great_sword.fgsa"],
        check=True,
    )
    sound = assets / "frontier_gs_sound.arc"
    subprocess.run([tool, "great-sword-sound", dat, "-o", sound], check=True)
    effects = assets / "effects"
    subprocess.run(
        [
            tool, "great-sword-effects", dat,
            "--gu-weapon", romfs / "nativeNX/arc/player/quest/w00.arc",
            "-o", effects,
        ],
        check=True,
    )
    env = os.environ.copy()
    env.pop("RUSTUP_TOOLCHAIN", None)
    subprocess.run(
        ["cargo", "forge", "build", "--release"], cwd=ROOT, env=env, check=True
    )

    # Install private LMTs as loose resources: the game loads them by resource
    # class and path. No overlay of player/quest/w00.arc is necessary.
    overlay = ROOT / "dist/great-sword-extreme"
    nx = overlay / "romfs/nativeNX"
    for arc, prefix, types in [
        (
            sound,
            "sound/se/pl/frontier_gs/",
            {0x1BCC4966: "srqr", 0x15D782FB: "sbkr", 0x79C47B59: "adpcm"},
        ),
        (
            effects / "geometry.arc",
            "effect/frontier_gs/",
            {
                0x5A525C16: "pel",
                0x6D5AE854: "efl",
                0x58A15856: "mod",
                0x2749C8A8: "mrl",
                0x241F5DEB: "tex",
            },
        ),
    ]:
        archive = arc.read_bytes()
        generated = set()
        for i in range(struct.unpack_from("<H", archive, 6)[0]):
            name, kind, packed, size, start = struct.unpack_from(
                "<64s4I", archive, 12 + 80 * i
            )
            name = name.rstrip(b"\0").decode().replace("\\", "/")
            assert name.startswith(prefix) and ".." not in name
            ext = types[kind]
            file = nx / (name + "." + ext)
            data = zlib.decompress(archive[start : start + packed])
            assert len(data) == size & 0x1FFFFFFF
            file.parent.mkdir(exist_ok=True, parents=True)
            file.write_bytes(data)
            generated.add(file)
        for old in (nx / prefix).rglob("*"):
            if old.suffix[1:] in types.values() and old not in generated:
                old.unlink()
    path = nx / "player/mot/frontier_gs"
    path.mkdir(parents=True, exist_ok=True)
    footer = (assets / "great_sword.fgsa").read_bytes()[-128:]
    assert footer[:4] == b"CTR0" and footer[44:48] == b"FX08"
    hit = nx / "player/hit/frontier_gs"
    hit.mkdir(parents=True, exist_ok=True)
    for name, offset in (("guard_counter", 4), ("radiance", 88)):
        radius, *ends = struct.unpack_from("<7f", footer, offset + 12)
        assert radius > 0
        (hit / f"{name}.hts").write_bytes(
            struct.pack(
                "<4s5I4H10f", b"HTS\0", 0x20131015, 1, 0, 1, 24,
                255, 0, 1, 0, 0.0, radius, *ends[:3], 0.0, *ends[3:], 0.0,
            )
        )
    arc = motions.read_bytes()
    assert arc[:8] == b"ARC\0\x11\0\x03\0"
    for i, group in enumerate((0, 2, 4)):
        name, kind, packed, size, start = struct.unpack_from(
            "<64s4I", arc, 12 + 80 * i
        )
        stem = str(group)
        assert (
            name.rstrip(b"\0") == f"player\\mot\\frontier_gs\\{stem}".encode()
            and kind == 0x76820D81
        )
        data = zlib.decompress(arc[start : start + packed])
        assert len(data) == size & 0x1FFFFFFF and data.startswith(b"LMT\0C\0")
        (path / f"{stem}.lmt").write_bytes(data)
    for sex in "mf":
        (assets / f"frontier_gs_{sex}.arc").unlink(missing_ok=True)
        for group in (0, 2, 4):
            (path / f"{sex}_{group}.lmt").unlink(missing_ok=True)
    (nx / "plugins").mkdir(exist_ok=True)
    shutil.copy2(
        ROOT / "great_sword_extreme.nro", nx / "plugins/great_sword_extreme.nro"
    )
    return overlay


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("dat", type=Path)
    parser.add_argument("romfs", type=Path)
    arguments = parser.parse_args()
    print(package(arguments.dat, arguments.romfs))
