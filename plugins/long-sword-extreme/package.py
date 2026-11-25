"""Build the Extreme Long Sword Forge overlay from local game data."""

import argparse
import os
import shutil
import struct
import subprocess
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parent
CONVERTER = ROOT.parents[1] / "tools" / "mhfz2mhgu"
MOTION = {0x76820D81: "lmt"}
SOUND = {0x1BCC4966: "srqr", 0x15D782FB: "sbkr", 0x79C47B59: "adpcm"}
EFFECT = {
    0x5A525C16: "pel",
    0x6D5AE854: "efl",
    0x58A15856: "mod",
    0x2749C8A8: "mrl",
    0x241F5DEB: "tex",
}


def unpack(archive, destination, prefix, kinds):
    data = archive.read_bytes()
    magic, version, count = struct.unpack_from("<4sHH", data)
    if (magic, version) != (b"ARC\0", 17):
        raise ValueError(f"Invalid ARC {archive.name}")
    paths = set()
    for i in range(count):
        name, kind, packed, size, start = struct.unpack_from(
            "<64s4I", data, 12 + 80 * i
        )
        name = name.rstrip(b"\0").decode().replace("\\", "/")
        if not name.startswith(prefix) or ".." in name.split("/"):
            raise ValueError(f"Unexpected resource path {name}")
        raw = zlib.decompress(data[start : start + packed])
        if len(raw) != (size & 0x1FFFFFFF):
            raise ValueError(f"Incorrect size for {name}")
        path = destination / f"{name}.{kinds[kind]}"
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(raw)
        paths.add(path)
    return paths


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
        [tool, "long-sword-motions", dat, "-o", motions],
        check=True,
    )
    subprocess.run(
        [tool, "long-sword-actions", dat, "-o", assets / "long_sword.flsa"],
        check=True,
    )
    sound = assets / "frontier_ls_sound.arc"
    subprocess.run([tool, "long-sword-sound", dat, "-o", sound], check=True)
    effects = assets / "effects"
    subprocess.run(
        [
            tool, "long-sword-effects", dat,
            "--gu-weapon", romfs / "nativeNX/arc/player/quest/w00.arc",
            "-o", effects,
        ],
        check=True,
    )
    env = os.environ.copy()
    env.pop("RUSTUP_TOOLCHAIN", None)
    subprocess.run(["cargo", "forge", "build", "--release"], cwd=ROOT, env=env, check=True)

    overlay = ROOT / "dist/long-sword-extreme"
    nx = overlay / "romfs/nativeNX"
    unpack(motions, nx, "player/mot/frontier_ls/", MOTION)
    for sex in "mf":
        (assets / f"{sex}.arc").unlink(missing_ok=True)
        for group in (0, 2, 4):
            (nx / f"player/mot/frontier_ls/{sex}_{group}.lmt").unlink(missing_ok=True)
    hud = nx / "GUI/frontier_ls/gauge.tex"
    hud.parent.mkdir(parents=True, exist_ok=True)
    subprocess.run([tool, "long-sword-hud", dat, "-o", hud], check=True)
    hit = nx / "player/hit/frontier_ls/release.hts"
    hit.parent.mkdir(parents=True, exist_ok=True)
    for selector in (0, 32, 33):
        shutil.copy2(assets / f"blade_{selector}.hts", hit.parent)
    # One sphere on hunter joint 0. The delayed strike updates its local
    # coordinates from the marked monster part before native expansion.
    hit.write_bytes(struct.pack("<4s5I4H10f", b"HTS\0", 0x20131015, 1, 0, 1, 24,
                                0, 0, 0, 0, 0., 10., *([0.] * 8)))
    # GU retains the group sizes for collision traversal. The world-contact
    # callback supplies the actual discharge and instant-slash capsules.
    header = struct.pack("<4s3I", b"HTS\0", 0x20131015, 1, 0)
    capsule = struct.pack("<4H10f", 0, 0, 1, 0, 0., 40., *([0.] * 8))
    (hit.parent / "discharge.hts").write_bytes(
        header + struct.pack("<2I", 4, 24) + capsule * 4
    )
    (hit.parent / "instant.hts").write_bytes(header + struct.pack("<2I", 1, 24) + capsule)
    unpack(sound, nx, "sound/se/pl/frontier_ls/", SOUND)
    effect_files = unpack(effects / "geometry.arc", nx, "effect/frontier_ls/", EFFECT)
    # The private output directory belongs to this converter. Selector indices
    # can shrink when an emitter is removed, leaving unreachable files in dist.
    for path in (nx / "effect/frontier_ls").iterdir():
        if path.is_file() and path.suffix[1:] in EFFECT.values() and path not in effect_files:
            path.unlink()
    (nx / "plugins").mkdir(parents=True, exist_ok=True)
    shutil.copy2(ROOT / "long_sword_extreme.nro", nx / "plugins/long_sword_extreme.nro")
    return overlay


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("dat", type=Path)
    parser.add_argument("romfs", type=Path)
    arguments = parser.parse_args()
    print(package(arguments.dat, arguments.romfs))
