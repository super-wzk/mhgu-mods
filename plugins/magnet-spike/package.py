"""Build the Magnet Spike weapon archive, private GUI icon, and plugin."""

import argparse
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parent
CONVERTER = ROOT.parents[1] / "tools/mhfz2mhgu"


def package(dat, romfs):
    env = os.environ.copy()
    env.pop("RUSTUP_TOOLCHAIN", None)
    nx = ROOT / "dist/magnet-spike/romfs/nativeNX"
    with tempfile.TemporaryDirectory() as staging:
        arc = Path(staging) / "magnet_spike.arc"
        icon = Path(staging) / "icon.tex"
        subprocess.run([
            "cargo", "run", "--locked", "--release", "--", "magnet-spike-battle", dat,
            "--material-template", romfs / "nativeNX/arc/enemy/em030_00.arc",
            "--gu-weapon", romfs / "nativeNX/arc/player/quest/w00.arc",
            "-o", staging,
        ], cwd=CONVERTER, env=env, check=True)
        subprocess.run([
            "cargo", "run", "--locked", "--release", "--", "magnet-spike-icon",
            dat, "-o", icon,
        ], cwd=CONVERTER, env=env, check=True)
        assets = ROOT / "target/assets"
        assets.mkdir(parents=True, exist_ok=True)
        for table in ("magnet_spike.fmsa", "frontier_ms_sound.flse"):
            shutil.copy2(Path(staging) / table, assets)
        shutil.copy2(Path(staging) / "effects/routes.bin", assets / "frontier_ms_routes.bin")
        shutil.copy2(Path(staging) / "effects/motion_fx.bin", assets / "frontier_ms_motion_fx.bin")
        shutil.copy2(Path(staging) / "effects/selectors.bin", assets / "frontier_ms_selectors.bin")
        subprocess.run(["cargo", "forge", "build", "--release"], cwd=ROOT, env=env, check=True)
        for src, dest in (
            (arc, nx / "arc/frontier"),
            (icon, nx / "GUI/frontier_magnet_spike"),
            (ROOT / "magnet_spike.nro", nx / "plugins"),
        ):
            dest.mkdir(parents=True, exist_ok=True)
            shutil.copy2(src, dest)
    return nx


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("dat", type=Path)
    parser.add_argument("romfs", type=Path)
    args = parser.parse_args()
    print(package(args.dat.resolve(), args.romfs.resolve()))
