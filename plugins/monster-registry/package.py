"""Build this provider NRO into its own installable overlay; never installs it."""

import os
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parent

if __name__ == "__main__":
    env = os.environ.copy()
    env.pop("RUSTUP_TOOLCHAIN", None)
    subprocess.run(
        ["cargo", "forge", "build", "--release"], cwd=ROOT, env=env, check=True
    )
    dest = ROOT / "dist/monster-registry/romfs/nativeNX/plugins/monster_registry.nro"
    dest.parent.mkdir(parents=True, exist_ok=True)
    (ROOT / dest.name).replace(dest)
    print(dest)
