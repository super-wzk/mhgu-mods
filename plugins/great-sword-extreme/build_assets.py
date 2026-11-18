"""Build private GU hunter motion banks and their Frontier state program."""

import argparse
import os
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parent
REPO = ROOT.parents[1]


def build(dat, output):
    romfs = os.environ.get("MHGU_ROMFS")
    if not romfs:
        raise SystemExit("Set MHGU_ROMFS to the GU RomFS root containing nativeNX.")
    gu = Path(romfs) / "nativeNX/arc/player/quest/w00.arc"
    manifest = REPO / "tools" / "mhfz2mhgu" / "Cargo.toml"
    output.mkdir(parents=True, exist_ok=True)
    subprocess.run(
        ["cargo", "build", "--release", "--manifest-path", manifest], check=True
    )
    tool = manifest.parent / "target/release/mhfz2mhgu"
    for sex in ("m", "f"):
        subprocess.run(
            [
                tool,
                "great-sword-motions",
                dat,
                *(["--female"] if sex == "f" else []),
                "-o",
                output / f"frontier_gs_{sex}.arc",
            ],
            check=True,
        )
    subprocess.run(
        [tool, "great-sword-actions", dat, "-o", output / "great_sword.fgsa"],
        check=True,
    )
    subprocess.run(
        [tool, "great-sword-sound", dat, "-o", output / "frontier_gs_sound.arc"],
        check=True,
    )
    subprocess.run(
        [tool, "great-sword-effects", dat, "--gu-weapon", gu, "-o", output / "effects"],
        check=True,
    )


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("dat", type=Path)
    parser.add_argument("--output", type=Path, default=ROOT / "target/assets")
    args = parser.parse_args()
    build(args.dat, args.output)
