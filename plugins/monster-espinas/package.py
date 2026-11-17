"""Prepare Espinas resources and package the mod locally; never installs into the game."""

import argparse
import os
import shutil
import subprocess
import tempfile
from pathlib import Path

from quest_package import build_quest

PLUGIN = Path(__file__).resolve().parent
REPO = PLUGIN.parents[1]
MOD_NAME = "monster-espinas"


def build(pac, elf, enemy):
    dist = PLUGIN / "dist"
    dist.mkdir(exist_ok=True)
    dest = dist / MOD_NAME
    with tempfile.TemporaryDirectory(prefix=".build-", dir=dist) as temp:
        package = Path(temp) / MOD_NAME
        nx = package / "romfs/nativeNX"
        arc = nx / "arc/enemy/em107_00.arc"
        arc.parent.mkdir(parents=True)
        converter = [
            "cargo",
            "run",
            "--release",
            "--manifest-path",
            "tools/mhfz2mhgu/Cargo.toml",
            "--",
        ]
        subprocess.run(
            [
                *converter,
                "hunt",
                pac,
                "--vita-elf",
                elf,
                "--donor",
                enemy / "em001_00.arc",
                "--material-template",
                enemy / "em030_00.arc",
                "-o",
                arc,
            ],
            cwd=REPO,
            check=True,
        )
        icon = nx / "GUI/monster/espinas.tex"
        icon.parent.mkdir(parents=True)
        subprocess.run(
            [*converter, "hunt-icon", "--data", pac.parent.parent, "-o", icon],
            cwd=REPO,
            check=True,
        )
        build_quest(enemy.parents[1], nx)
        # Forge uses the plugin's pinned nightly, even when invoked from a host Cargo process.
        env = os.environ.copy()
        env.pop("RUSTUP_TOOLCHAIN", None)
        subprocess.run(
            ["cargo", "forge", "build", "--release"], cwd=PLUGIN, env=env, check=True
        )
        plugins = nx / "plugins"
        plugins.mkdir()
        nro = PLUGIN / "monster_espinas.nro"
        shutil.move(nro, plugins / nro.name)

        previous = dist / f".previous-{MOD_NAME}"
        had_previous = dest.exists()
        if had_previous:
            dest.rename(previous)
        try:
            package.rename(dest)
        except OSError:
            if had_previous:
                previous.rename(dest)
            raise
        if had_previous:
            shutil.rmtree(previous)
    print(dest)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pac", required=True, type=Path)
    parser.add_argument("--enemy", required=True, type=Path)
    parser.add_argument("--vita-elf", type=Path, default=REPO / "mhfz/eboot.bin.elf")
    args = parser.parse_args()
    try:
        build(args.pac.resolve(), args.vita_elf.resolve(), args.enemy.resolve())
    except subprocess.CalledProcessError as e:
        raise SystemExit(e.returncode) from None
