"""Check packed ARC directory ordering, shared references and runtime mounting."""
from collections import Counter
from pathlib import Path
import argparse
import struct
import sys
import zlib

TOOLS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(TOOLS))
from package_weapon_styles import KINDS, MAGNET_SPIKE, PLUGINS, RELEASE_DIR, REPO, WEAPONS

sys.path.insert(0, str(PLUGINS / "great-sword-extreme/tests"))
sys.path.insert(0, str(PLUGINS / "mhgu-sdk-runtime/tests"))
from gu_replay import SDK_NRO
from verify_style import MANAGER, boot

def assets(path):
    b = path.read_bytes()
    assert b[:6] == b"ARC\0\x11\0"
    result = {}
    for i in range(struct.unpack_from("<H", b, 6)[0]):
        name, k, cs, ds, p = struct.unpack_from("<64s4I", b, 12 + 80 * i)
        key = name.rstrip(b"\0").decode(), k
        v = zlib.decompress(b[p : p + cs])
        assert len(v) == ds & 0x1FFFFFFF and key not in result
        result[key] = v
    return result


LOAD = 0xBCAD18
PARSE = 0xB39F58


def material_refs(data):
    count, _, table = struct.unpack_from("<3I", data, 12)
    return [data[at + 12:at + 76].split(b"\0", 1)[0].decode()
            for at in range(table, table + count * 76, 76)]


def sound_refs(data):
    groups, count = struct.unpack_from("<2I", data, 8)
    assert groups == 1
    at = 32
    for _ in range(count):
        end = data.index(0, at)
        yield data[at:end].decode(), data[end + 1:end + 81]
        at = end + 81
    assert at == len(data)


def verify_magnet_effects(resources, ordered):
    key = (r"effect\frontier_ms\particles", KINDS[".pel"])
    pel = resources[key]
    assert pel[:8] == b"PEL\0\x0f\0\0\0"
    assert struct.unpack_from("<2I", pel, 8) == (0, 105)
    at = 16
    for i in range(105):
        name = pel[at + 1:at + 1 + pel[at]].decode()
        assert name == rf"effect\frontier_ms\e{i}"
        at += 1 + pel[at]
        assert pel[at] == 0
        at += 1 + 160
        effect_key = name, KINDS[".efl"]
        assert ordered.index(effect_key) < ordered.index(key)
        efl = resources[effect_key]
        assert efl.startswith(b"EFL\0") and struct.unpack_from("<HH", efl, 16) == (1, 1)
        model_offset = 48 + (struct.unpack_from("<I", efl, 52)[0] >> 8)
        mesh = efl[model_offset + 80:efl.index(0, model_offset + 80)].decode()
        material_key = mesh, KINDS[".mrl"]
        assert ordered.index(material_key) < ordered.index((mesh, KINDS[".mod"])) < ordered.index(effect_key)
        assert all(ordered.index((ref, KINDS[".tex"])) < ordered.index(material_key)
                   for ref in material_refs(resources[material_key]))
    assert pel[at:] == bytes(4)


def verify_payloads(release, *, include_magnet_spike=False):
    path = "romfs/nativeNX/arc/frontier"
    sdk = release / "mhgu-sdk"
    assert (sdk / "romfs/nativeNX/plugins/mhgu-sdk.nro").read_bytes() == SDK_NRO.read_bytes()
    common = assets(sdk / path / "common.arc")
    used = Counter()
    expected_files = 14 + 3 * int(include_magnet_spike)
    assert len([f for f in release.rglob("*") if f.is_file()]) == expected_files
    if include_magnet_spike:
        relative = Path(MAGNET_SPIKE) / "romfs/nativeNX/plugins/magnet_spike.nro"
        assert (release / relative).read_bytes() == (
            PLUGINS / MAGNET_SPIKE / "dist" / relative
        ).read_bytes()
        arc = Path(MAGNET_SPIKE) / path / "magnet_spike.arc"
        assert (release / arc).read_bytes() == (PLUGINS / MAGNET_SPIKE / "dist" / arc).read_bytes()
        icon = Path(MAGNET_SPIKE) / "romfs/nativeNX/GUI/frontier_magnet_spike/icon.tex"
        assert (release / icon).read_bytes() == (PLUGINS / MAGNET_SPIKE / "dist" / icon).read_bytes()
        resources = assets(release / arc)
        model = r"weapon\frontier\magnet_spike\wk451"
        mesh = resources[model, KINDS[".mod"]]
        assert mesh[:5] == b"MOD\0\xd4" and all(struct.unpack_from("<3H", mesh, 6))
        refs = material_refs(resources[model, KINDS[".mrl"]])
        assert len(refs) == 2 and all((ref, KINDS[".tex"]) in resources for ref in refs)
        ordered = list(resources)
        for bank, required in (("blade", {8, 9, 12, 13, 15}), ("common", {74, 241})):
            name = rf"sound\se\pl\frontier_ms\{bank}"
            bank_key, request_key = (name, KINDS[".sbkr"]), (name, KINDS[".srqr"])
            bank_index = ordered.index(bank_key)
            assert bank_index < ordered.index(request_key)
            voices = list(sound_refs(resources[bank_key]))
            requests = resources[request_key]
            assert requests[:5] == b"SRQR\x05" and struct.unpack_from("<I", requests, 8)[0] == len(voices)
            cues = set()
            for i, (wave, _) in enumerate(voices):
                key = wave, KINDS[".adpcm"]
                assert ordered.index(key) < bank_index
                assert len(resources[key]) > 120 and any(resources[key][120:]), wave
                cues.add(struct.unpack_from("<H", requests, 52 + 132 * i)[0] - 4096)
            assert required <= cues
        motion_keys = [key for key in resources if key[1] == KINDS[".lmt"]]
        assert ordered.index((model, KINDS[".mrl"])) < ordered.index((model, KINDS[".mod"]))
        verify_magnet_effects(resources, ordered)
        anchors = [(0x4005, 0x4007), (0x4004,), (0x4005, 0x4007), (0x4004,)] + [(255,)] * 4
        for selector, joints in enumerate(anchors, 1):
            hts = resources[rf"player\hit\frontier_ms\melee_{selector}", KINDS[".hts"]]
            assert hts[:4] == b"HTS\0"
            assert struct.unpack_from("<3I", hts, 8) == (1, 0, len(joints))
            assert struct.unpack_from("<I", hts, 20) == (24,)
            assert len(hts) == 24 + 48 * len(joints)
            for i, joint in enumerate(joints):
                assert struct.unpack_from("<3H", hts, 24 + 48 * i) == (joint, 0, 1)
        expected = {rf"player\mot\frontier_ms\{g}": (128 + n, None)
                    for g, n in ((0, 85), (2, 25), (4, 100))}
        expected.update({rf"weapon\frontier\magnet_spike\mot\{g}": (n, live)
                         for g, n, live in ((0, 85, 81), (1, 70, 3), (2, 25, 4), (4, 100, 75))})
        assert {name for name, _ in motion_keys} == expected.keys()
        for name, kind in motion_keys:
            lmt = resources[name, kind]
            slot_count, live = expected[name]
            assert lmt[:6] == b"LMT\0C\0" and struct.unpack_from("<H", lmt, 6)[0] == slot_count
            offsets = struct.unpack_from(f"<{slot_count}I", lmt, 8)
            assert all(8 + 4 * slot_count <= at < len(lmt) - 56 for at in offsets if at)
            if live is not None:
                assert sum(bool(at) for at in offsets) == live, name
        for name, frames in ((r"player\mot\frontier_ms\4", 89),
                             (r"weapon\frontier\magnet_spike\mot\4", 91)):
            lmt = resources[name, KINDS[".lmt"]]
            clip = struct.unpack_from("<I", lmt, 8 + 4 * 35)[0]
            assert clip and struct.unpack_from("<II", lmt, clip + 8) == (frames, 52)
            assert all(struct.unpack_from("<2I", lmt, 8 + 4 * 51)), (name, "boost motions")
    for name, tag in WEAPONS.items():
        package = f"{name}-extreme"
        nx = PLUGINS / package / "dist" / package / "romfs/nativeNX"
        plugin = f"plugins/{package.replace('-', '_')}.nro"
        assert (release / package / "romfs/nativeNX" / plugin).read_bytes() == (nx / plugin).read_bytes()
        private = assets(release / package / path / f"{tag}.arc")
        available = dict(common)
        source = {}
        for file in nx.rglob("*"):
            if file.is_file() and file.suffix in KINDS:
                key = (str(file.relative_to(nx).with_suffix("")).replace("/", "\\"),
                       KINDS[file.suffix])
                source[key] = file.read_bytes()
        resolved = set()
        for (name, kind), data in private.items():
            assert (name, kind) not in available
            original = source[name, kind]
            if kind == KINDS[".mrl"]:
                old_refs, new_refs = material_refs(original), material_refs(data)
                assert len(old_refs) == len(new_refs)
                targets = zip(old_refs, new_refs, [KINDS[".tex"]] * len(old_refs))
            elif kind == KINDS[".sbkr"]:
                old_rows, new_rows = list(sound_refs(original)), list(sound_refs(data))
                assert original[:32] == data[:32] and len(old_rows) == len(new_rows)
                assert [row for _, row in old_rows] == [row for _, row in new_rows]
                targets = zip((n for n, _ in old_rows), (n for n, _ in new_rows),
                              [KINDS[".adpcm"]] * len(old_rows))
            else:
                assert data == original
                targets = ()
            for old, new, target_kind in targets:
                old_key, new_key = (old, target_kind), (new, target_kind)
                assert source[old_key] == available[new_key], (old_key, new_key)
                if new_key in common:
                    used[new_key] += 1
                    resolved.add(old_key)
            available[name, kind] = data
        assert private.keys() | resolved == source.keys(), (tag, source.keys() - (private.keys() | resolved))
    assert used.keys() == common.keys()
    print(f"{expected_files} installed files, {len(common)} common resources, all original payloads accounted for")


def verify_hooks():
    elf = (REPO / "mhgu/main.elf").read_bytes()
    g = boot(elf, None, ("registry",))
    manager = g.alloc(32)
    archive_class = 0x2124DF0
    calls = []
    mounted = {}
    resource = g.alloc(128)

    def locate(mgr, kind, path, flags):
        assert mgr == manager
        name = g.string(path)
        calls.append((name, flags))
        if kind == archive_class:
            assert flags == 1 and name not in mounted
            mounted[name] = g.alloc(128)
            return mounted[name]
        return resource

    g.stub(g.originals[LOAD], locate)

    def call(path, flags):
        addr = g.alloc(len(path) + 1)
        g.cpu.mem_write(addr, path.encode() + b"\0")
        assert g.run(g.hooks[LOAD], manager, 0x2125210, addr, flags) == resource

    call(r"player\mot\frontier_gs\0", 1)
    call(r"effect\frontier_gs\particles", 2)
    call(r"effect\frontier_sa\particles", 2)  # Can also be a remote hunter's first request.
    call(r"effect\retail\particles", 2)
    assert calls == [
        (r"arc\frontier\common", 1),
        (r"arc\frontier\gs", 1),
        (r"player\mot\frontier_gs\0", 1),
        (r"effect\frontier_gs\particles", 2),
        (r"arc\frontier\sa", 1),
        (r"effect\frontier_sa\particles", 2),
        (r"effect\retail\particles", 2),
    ]

    # Verify the real native parser hook selects the ordered load branch
    # on private ARC streams and preserves the flag for retail archives.
    def parse(obj, *_):
        name = g.string(obj + 8)
        assert bool(g.get(obj + 76) & 0x200) == name.startswith("arc\\frontier\\")
        return 1

    g.stub(g.originals[PARSE], parse)
    for name in (r"arc\frontier\common", r"arc\frontier\sa", r"arc\retail"):
        obj = g.alloc(128)
        g.cpu.mem_write(obj + 8, name.encode() + b"\0")
        g.put(obj + 76, 0x10)
        assert g.run(g.hooks[PARSE], obj, 0) == 1
        assert g.get(obj + 76) == 0x10
    print("GU registry NRO mounts common before the requested weapon, once per archive")


def verify_native_directory(release, *, include_magnet_spike=False):
    # Run GU's actual rArchive parser for both the common bundle and the
    # largest private bundle. The child-loading callback substitutes resource
    # creation and GPU/audio services, retaining the native directory order.
    elf = (REPO / "mhgu/main.elf").read_bytes()
    g = boot(elf, None, ("registry",))
    bundles = [("mhgu-sdk", "common"), ("switch-axe-extreme", "sa")]
    if include_magnet_spike:
        bundles.append((MAGNET_SPIKE, "magnet_spike"))
        # Register the actual equipment archive so the runtime selects sequential loading.
        nro = release / MAGNET_SPIKE / "romfs/nativeNX/plugins/magnet_spike.nro"
        plugin = g.load(nro, 0x9200000)
        for callback in ("forge_onLoad", "forge_onInit"):
            g.run(plugin[callback], MANAGER)
    heap, heap_vt = g.alloc(4), g.alloc(56)
    g.put(heap, heap_vt)
    g.put(0x18A6308, heap)
    g.stub(0x7AA47C, lambda *_: heap)
    g.stub(0x13EFD7C, lambda dst, n, *_: g.cpu.mem_write(dst, bytes(n)))
    for off, fn in ((28, lambda _, size, *__: g.alloc(size)),
                    (32, lambda _, size, *__: g.alloc(size)),
                    (52, lambda *_: 0)):
        fnptr = g.alloc(4)
        g.put(heap_vt + off, fnptr)
        g.stub(fnptr, fn)

    manager, manager_vt = g.alloc(4), g.alloc(56)
    g.put(manager, manager_vt)
    g.put(0x2126A64, manager)
    g.stub(0x7AEF68, lambda kind, *_: kind)
    g.stub(0x121DC7C, lambda *_: 0)  # zlib member init, simulated by child loading.
    for off in (4, 44):
        g.stub(g.get(0x1792740 + off), lambda *_: 0)

    observed = []
    def child(_, stream, kind, row):
        observed.append((g.string(row), kind))
        assert g.get(stream + 28) == g.get(row + 72) & 0x1FFFFFFF
        return 0x1234

    child_fn = g.alloc(4)
    g.put(manager_vt + 52, child_fn)
    g.stub(child_fn, child)

    for directory, tag in bundles:
        path = release / directory / "romfs/nativeNX/arc/frontier" / f"{tag}.arc"
        blob = path.read_bytes()
        expected = list(assets(path))
        consumed = 0
        observed.clear()
        archive, stream, vt = g.alloc(128), g.alloc(4), g.alloc(84)
        g.cpu.mem_write(archive + 8, f"arc\\frontier\\{tag}\0".encode())
        g.put(archive + 76, 0x10)
        g.put(archive + 80, 0x200)
        g.put(stream, vt)

        def read(_, dst, size, *__):
            nonlocal consumed
            g.cpu.mem_write(dst, blob[consumed:consumed + size])
            consumed += size
            return size

        for off, fn in ((36, lambda *_: 1), (52, read), (76, lambda *_: len(blob))):
            fnptr = g.alloc(4)
            g.put(vt + off, fnptr)
            g.stub(fnptr, fn)
        assert g.run(g.hooks[PARSE], archive, stream) == 1
        assert observed == expected and g.get(archive + 104) == len(expected)
    print("Native rArchive parser consumed in dependency order:", ", ".join(tag for _, tag in bundles))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--include-magnet-spike", action="store_true")
    args = parser.parse_args()
    verify_payloads(RELEASE_DIR, include_magnet_spike=args.include_magnet_spike)
    verify_hooks()
    verify_native_directory(RELEASE_DIR, include_magnet_spike=args.include_magnet_spike)
