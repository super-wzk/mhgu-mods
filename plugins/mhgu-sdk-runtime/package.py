"""Build the shared MHGU 1.4 SDK runtime overlay."""

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
    destination = ROOT / "dist/mhgu-sdk/romfs/nativeNX/plugins/mhgu-sdk.nro"
    destination.parent.mkdir(parents=True, exist_ok=True)
    print(shutil.copy2(ROOT / "mhgu_sdk_runtime.nro", destination))
