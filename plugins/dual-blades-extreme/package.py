"""Build the Extreme Dual Blades Forge overlay from local game data."""

import argparse
import os
import re
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


def package_peer_trails(directory):
    # Each render callback is keyed by model path. Give each peer its own
    # five strip models and redirect just those EFL entries in its PEL.
    prefix = rb"effect\\frontier_db\\"
    strips = re.compile(prefix + rb"m50([1-5])(?=\x00)")
    pel = (directory / "particles.pel").read_bytes()
    layers = [(p, p.read_bytes()) for p in directory.glob("e*.efl")]
    for slot, letter in enumerate("abcd", 1):
        output = pel
        for path, layer in layers:
            if not strips.search(layer):
                continue
            old_name = path.stem
            new_name = letter + old_name[1:]
            old = ("effect\\frontier_db\\" + old_name).encode() + b"\0"
            new = ("effect\\frontier_db\\" + new_name).encode() + b"\0"
            if old not in output:
                raise ValueError(f"Missing particle layer {old_name}")
            output = output.replace(old, new)
            renamed = strips.sub(lambda m: m[0][:-2] + str(slot).encode() + m[1], layer)
            (directory / (new_name + ".efl")).write_bytes(renamed)
        (directory / f"peer{slot}.pel").write_bytes(output)
        for channel in range(1, 6):
            for ext in ("mod", "mrl"):
                shutil.copyfile(directory / f"m50{channel}.{ext}", directory / f"m5{slot}{channel}.{ext}")


def package(dat, romfs):
    assets = ROOT / "target/assets"
    assets.mkdir(parents=True, exist_ok=True)
    subprocess.run(
        ["cargo", "build", "--release", "--manifest-path", CONVERTER / "Cargo.toml"],
        check=True,
    )
    tool = CONVERTER / "target/release" / CONVERTER.name
    for sex in "mf":
        subprocess.run(
            [
                tool, "dual-blades-motions", dat,
                *(["--female"] if sex == "f" else []),
                "-o", assets / f"{sex}.arc",
            ],
            check=True,
        )
    subprocess.run(
        [tool, "dual-blades-actions", dat, "-o", assets / "dual_blades.fdba"],
        check=True,
    )
    sound = assets / "frontier_db_sound.arc"
    subprocess.run([tool, "dual-blades-sound", dat, "-o", sound], check=True)
    effects = assets / "effects"
    subprocess.run(
        [
            tool, "dual-blades-effects", dat,
            "--gu-weapon", romfs / "nativeNX/arc/player/quest/w00.arc",
            "-o", effects,
        ],
        check=True,
    )
    env = os.environ.copy()
    env.pop("RUSTUP_TOOLCHAIN", None)
    subprocess.run(["cargo", "forge", "build", "--release"], cwd=ROOT, env=env, check=True)

    overlay = ROOT / "dist/dual-blades-extreme"
    nx = overlay / "romfs/nativeNX"
    hud = nx / "GUI/frontier_db/gauge.tex"
    hud.parent.mkdir(parents=True, exist_ok=True)
    subprocess.run([tool, "dual-blades-hud", "-o", hud], check=True)
    for sex in "mf":
        unpack(assets / f"{sex}.arc", nx, "player/mot/frontier_db/", MOTION)
    unpack(sound, nx, "sound/se/pl/frontier_db/", SOUND)
    unpack(effects / "geometry.arc", nx, "effect/frontier_db/", EFFECT)
    package_peer_trails(nx / "effect/frontier_db")
    (nx / "plugins").mkdir(parents=True, exist_ok=True)
    shutil.copy2(ROOT / "dual_blades_extreme.nro", nx / "plugins/dual_blades_extreme.nro")
    return overlay


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("dat", type=Path)
    parser.add_argument("romfs", type=Path)
    arguments = parser.parse_args()
    print(package(arguments.dat, arguments.romfs))
