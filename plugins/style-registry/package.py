"""Build the standalone MHGU 1.4 Forge style registry."""

import os
from pathlib import Path
import shutil
import subprocess

ROOT = Path(__file__).resolve().parent

if __name__ == "__main__":
    env = os.environ.copy()
    env.pop("RUSTUP_TOOLCHAIN", None)
    subprocess.run(
        ["cargo", "forge", "build", "--release"], cwd=ROOT, env=env, check=True
    )
    destination = ROOT / "dist/style-registry/romfs/nativeNX/plugins"
    destination.mkdir(parents=True, exist_ok=True)
    print(shutil.copy2(ROOT / "style_registry.nro", destination))
