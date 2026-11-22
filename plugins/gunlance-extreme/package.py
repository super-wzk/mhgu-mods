"""Build the Extreme Gunlance Forge overlay from local game data."""

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
    subprocess.run(
        [
            "cargo",
            "run",
            "--release",
            "--manifest-path",
            CONVERTER / "Cargo.toml",
            "--",
            "gunlance",
            dat,
            "--gu-weapon",
            romfs / "nativeNX/arc/player/quest/w00.arc",
            "-o",
            assets,
        ],
        check=True,
    )
    env = os.environ.copy()
    env.pop("RUSTUP_TOOLCHAIN", None)
    subprocess.run(
        ["cargo", "forge", "build", "--release"], cwd=ROOT, env=env, check=True
    )
    overlay = ROOT / "dist/gunlance-extreme"
    nx = overlay / "romfs/nativeNX"
    for sex in "mf":
        unpack(assets / f"{sex}.arc", nx, "player/mot/frontier_gl/", MOTION)
    sounds = unpack(assets / "sound.arc", nx, "sound/se/pl/frontier_gl/", SOUND)
    for wave in (nx / "sound/se/pl/frontier_gl").glob("*/w[0-9]*.adpcm"):
        if wave not in sounds:
            wave.unlink()
    effects = unpack(assets / "effects/geometry.arc", nx, "effect/frontier_gl/", EFFECT)
    for emitter in (nx / "effect/frontier_gl").glob("e*.efl"):
        if emitter.stem[1:].isdigit() and emitter not in effects:
            emitter.unlink()
    (nx / "GUI/frontier_gl/ignition.tex").unlink(missing_ok=True)
    (nx / "plugins").mkdir(parents=True, exist_ok=True)
    shutil.copy2(ROOT / "gunlance_extreme.nro", nx / "plugins")
    return overlay


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("dat", type=Path)
    parser.add_argument("romfs", type=Path)
    arguments = parser.parse_args()
    print(package(arguments.dat, arguments.romfs))
