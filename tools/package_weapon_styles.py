"""Assemble the built SDK runtime and weapon overlays as a packed release."""

import argparse
from collections import defaultdict
from hashlib import sha256
from pathlib import Path
import shutil
import struct
import tempfile
import zlib

REPO = Path(__file__).resolve().parents[1]
PLUGINS = REPO / "plugins"
RELEASE_DIR = REPO / "dist/weapon-styles"
SDK = "mhgu-sdk"
MAGNET_SPIKE = "magnet-spike"
WEAPONS = {
    "great-sword": "gs",
    "dual-blades": "db",
    "long-sword": "ls",
    "switch-axe": "sa",
    "sword-shield": "sns",
    "gunlance": "gl",
}
# Native rArchive loads in directory order. Put dependencies before users.
KINDS = {
    ".tex": 0x241F5DEB,
    ".adpcm": 0x79C47B59,
    ".mrl": 0x2749C8A8,
    ".mod": 0x58A15856,
    ".efl": 0x6D5AE854,
    ".pel": 0x5A525C16,
    ".sbkr": 0x15D782FB,
    ".srqr": 0x1BCC4966,
    ".hts": 0x5653C1B0,
    ".lmt": 0x76820D81,
}


def read_overlay(name):
    crate = PLUGINS / ("mhgu-sdk-runtime" if name == SDK else name)
    root = crate / "dist" / name / "romfs/nativeNX"
    nro = name if name == SDK else name.replace("-", "_")
    plugin = root / "plugins" / f"{nro}.nro"
    if not plugin.is_file():
        raise ValueError(f"Build {name} before assembling the release")
    resources = {}
    for file in sorted(root.rglob("*")):
        if file.is_file() and file.suffix != ".nro" and file.name != ".DS_Store":
            if file.suffix not in KINDS:
                raise ValueError(f"Unknown resource type: {file}")
            path = str(file.relative_to(root).with_suffix("")).replace("/", "\\").encode()
            resources[path, file.suffix] = file.read_bytes()
    return plugin, resources


def common_resources(weapons):
    groups = defaultdict(list)
    for tag, (_, resources) in weapons.items():
        references = set()
        for (_, ext), data in resources.items():
            if ext == ".mrl":
                for at in texture_fields(data):
                    references.add((data[at:at + 64].split(b"\0", 1)[0], ".tex"))
            elif ext == ".sbkr":
                for start, end in wave_fields(data):
                    references.add((data[start:end], ".adpcm"))
        for path, ext in sorted(references):
            data = resources[path, ext]
            groups[ext, sha256(data).digest()].append((tag, path))
    common = {}
    aliases = {}
    for (ext, digest), uses in sorted(groups.items()):
        if len({tag for tag, _ in uses}) < 2:
            continue
        prefix = (b"effect\\frontier_common\\t" if ext == ".tex"
                  else b"sound\\se\\pl\\frontier_common\\w")
        name = prefix + digest.hex()[:16].encode()
        tag, path = uses[0]
        data = weapons[tag][1][path, ext]
        for tag, path in uses:
            original = weapons[tag][1].pop((path, ext))
            if original != data:
                raise ValueError(f"Hash collision for {path!r}")
            aliases[path, ext] = name
        common[name, ext] = data
    return common, aliases


def texture_fields(data):
    count, _, start = struct.unpack_from("<3I", data, 12)
    return range(start + 12, start + 12 + count * 76, 76)


def redirect_material(data, aliases):
    out = bytearray(data)
    for at in texture_fields(data):
        path = data[at:at + 64].split(b"\0", 1)[0]
        if shared := aliases.get((path, ".tex")):
            out[at:at + 64] = shared.ljust(64, b"\0")
    return bytes(out)


def wave_fields(data):
    # The converter writes one note group. Its NUL-terminated sample paths
    # are each followed by an 80-byte voice record.
    groups, count = struct.unpack_from("<2I", data, 8)
    if groups != 1:
        raise ValueError("Expected one note group in a generated weapon SBKR")
    at = 32
    for _ in range(count):
        end = data.index(0, at)
        yield at, end
        at = end + 81


def redirect_sound_bank(data, aliases):
    at = 32
    out = bytearray(data[:at])
    for start, end in wave_fields(data):
        path = data[start:end]
        out.extend(aliases.get((path, ".adpcm"), path))
        at = end + 81
        out.extend(data[end:at])
    return bytes(out) + data[at:]


def write_arc(path, resources):
    row_count = len(resources)
    out = bytearray((12 + 80 * row_count + 0x7FFF) & ~0x7FFF)
    struct.pack_into("<4sHHI", out, 0, b"ARC\0", 17, row_count, 0)
    rank = {ext: i for i, ext in enumerate(KINDS)}
    ordered = sorted(resources, key=lambda key: (rank[key[1]], key[0]))
    for i, (name, ext) in enumerate(ordered):
        if len(name) >= 64:
            raise ValueError(f"ARC resource path too long: {name!r}")
        data = resources[name, ext]
        packed = zlib.compress(data)
        struct.pack_into(
            "<64s4I", out, 12 + i * 80, name, KINDS[ext],
            len(packed), 0x40000000 | len(data), len(out),
        )
        out.extend(packed)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(out)


def package(*, include_magnet_spike=False):
    weapons = {tag: read_overlay(f"{name}-extreme") for name, tag in WEAPONS.items()}
    runtime, other = read_overlay(SDK)
    if other:
        raise ValueError("Unexpected resources in the SDK development overlay")
    magnet_root = PLUGINS / MAGNET_SPIKE / "dist" / MAGNET_SPIKE / "romfs/nativeNX"
    magnet_files = ("plugins/magnet_spike.nro", "arc/frontier/magnet_spike.arc",
                    "GUI/frontier_magnet_spike/icon.tex")
    if include_magnet_spike and not all((magnet_root / f).is_file() for f in magnet_files):
        raise ValueError("Build the Magnet Spike NRO, model ARC and icon TEX before assembling the release")
    common, aliases = common_resources(weapons)
    for _, resources in weapons.values():
        for key, data in resources.items():
            if key[1] == ".mrl":
                resources[key] = redirect_material(data, aliases)
            elif key[1] == ".sbkr":
                resources[key] = redirect_sound_bank(data, aliases)

    destination = RELEASE_DIR
    destination.parent.mkdir(parents=True, exist_ok=True)
    # Stage the entire release before replacing a previous generated package.
    with tempfile.TemporaryDirectory(dir=destination.parent) as staging:
        release = Path(staging) / destination.name
        nx = release / SDK / "romfs/nativeNX"
        (nx / "plugins").mkdir(parents=True)
        shutil.copy2(runtime, nx / "plugins")
        write_arc(nx / "arc/frontier/common.arc", common)
        for tag, (plugin, resources) in weapons.items():
            nx = release / plugin.parents[3].name / "romfs/nativeNX"
            (nx / "plugins").mkdir(parents=True)
            shutil.copy2(plugin, nx / "plugins")
            write_arc(nx / "arc/frontier" / f"{tag}.arc", resources)
        if include_magnet_spike:
            for relative in magnet_files:
                dst = release / MAGNET_SPIKE / "romfs/nativeNX" / relative
                dst.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(magnet_root / relative, dst)
        previous = Path(staging) / "previous"
        if destination.exists():
            destination.rename(previous)
        try:
            release.rename(destination)
        except OSError:
            if previous.exists():
                previous.rename(destination)
            raise
    return destination


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--include-magnet-spike", action="store_true",
                        help="include the Magnet Spike transaction test plugin")
    args = parser.parse_args()
    print(package(include_magnet_spike=args.include_magnet_spike))
