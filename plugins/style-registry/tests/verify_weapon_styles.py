"""Replay shared registration, hunt preload and particle routing for every weapon."""

import struct
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(REPO / "plugins/great-sword-extreme/tests"))
from verify_native_fx import binding_stack
from verify_style import (
    ENCODE, GAME_STYLE, INIT_HUNTER, MANAGER, PROVIDER, SUBMIT_MODEL, api_for, boot, hunters,
    load_registered_style, service, setup_controller,
)


def verify_effects(g, nx, tag):
    namespace = f"effect/frontier_{tag}"
    folder = nx / namespace
    emitters = set()
    for path in sorted(folder.glob("*.pel")):
        data = path.read_bytes()
        assert data[:8] == b"PEL\0\x0f\0\0\0", path
        (rows,) = struct.unpack_from("<I", data, 12)
        offset = 16
        for _ in range(rows):
            size = data[offset]
            name = data[offset + 1:offset + 1 + size].decode().replace("\\", "/")
            assert name.startswith(namespace + "/"), (path, name)
            emitters.add(name)
            offset += 1 + size
            offset += 1 + data[offset] + 160
        assert data[offset:] == bytes(4), path

    models = set()
    for name in sorted(emitters):
        efl = nx / f"{name}.efl"
        data = efl.read_bytes()
        assert data[:4] == b"EFL\0" and struct.unpack_from("<HH", data, 16) == (1, 1), efl
        (block,) = struct.unpack_from("<I", data, 52)
        assert block & 255 == 5, efl  # GU model generator.
        start = 48 + (block >> 8) + 80
        field = data[start:start + 48]
        name, terminator, _ = field.partition(b"\0")
        assert terminator, efl
        models.add(name)

    assert models, folder
    for name in sorted(models):
        relative = name.decode().replace("\\", "/")
        assert relative.startswith(namespace + "/"), relative
        assert (nx / f"{relative}.mod").is_file(), relative
        mrl = nx / f"{relative}.mrl"
        data = mrl.read_bytes()
        assert data[:8] == b"MRL\0\x22\0\0\0", mrl
        texture_count, _, table = struct.unpack_from("<3I", data, 12)
        assert table + 76 * texture_count <= len(data), mrl
        for offset in range(table, table + 76 * texture_count, 76):
            texture = data[offset + 12:offset + 76].split(b"\0", 1)[0].decode().replace("\\", "/")
            assert texture.startswith(namespace + "/"), (mrl, texture)
            assert (nx / f"{texture}.tex").is_file(), (mrl, texture)

    # Draw the packaged model paths through the registry. A missed namespace
    # falls back to the native renderer and leaves the test's bindings changed.
    binding_stack(g, [(4, name) for name in sorted(models)])
    print(f"GU {namespace}: {len(emitters)} emitters, {len(models)} model paths and draw state passed")


def replay_style(repo, elf, name, weapon_id, style, hunter_vtable, clip, tag, audio, shapes, hud):
    config = f"[Weapons]\n{weapon_id}={style}\n".encode()
    g = boot(elf, config, ("registry",))
    plugin = name + "-extreme"
    nx = repo / "plugins" / plugin / "dist" / plugin / "romfs/nativeNX"
    # Exercise the registry's shared draw path before weapon-specific trail
    # callbacks can suppress idle strips that have no pose history yet.
    verify_effects(g, nx, tag)
    submit = g.hooks[SUBMIT_MODEL]
    desc = load_registered_style(g, nx / "plugins" / (plugin.replace("-", "_") + ".nro"))
    _, selected = service(g)
    assert (g.get(desc), g.get(desc + 4), g.run(selected, weapon_id)) == (style, weapon_id, style)
    assert g.hooks[SUBMIT_MODEL] == submit
    init = g.hooks[INIT_HUNTER]
    actor, equipped, remote = hunters(g)
    for hunter in (actor, remote):
        g.put(hunter, hunter_vtable)
    set_action = setup_controller(g, actor)
    g.stub(0x887648, lambda *_: 0)
    released = []
    g.stub(0x887660, lambda resource, *_: released.append(resource) or 0)

    manager, vtable, load_fn = (g.alloc(n) for n in (16, 52, 4))
    g.put(manager, vtable)
    g.put(vtable + 48, load_fn)
    g.put(0x2126A64, manager)
    paths = []
    resources = {}
    background = []
    native_calls = []
    extensions = {
        0x2125210: ".lmt", 0x2125BEC: ".srqr", 0x1889FDC: ".pel",
        0x1857810: ".hts", 0x21255D0: ".tex",
    }

    def load(_, kind, path, flags):
        critical = kind in (0x2125210, 0x1857810)
        assert flags == (1 if critical else 2)
        assert native_calls[-1] == actor
        relative = g.string(path).replace("\\", "/") + extensions[kind]
        data = (nx / relative).read_bytes()
        resource = g.alloc(228 if kind == 0x21255D0 else 176)
        if kind == 0x2125210:
            count, = struct.unpack_from("<H", data, 6)
            header = g.alloc(8 + 4 * count)
            g.cpu.mem_write(header, data[:8 + 4 * count])
            g.put(resource + 100, header)
        elif kind == 0x2125BEC:
            count, = struct.unpack_from("<I", data, 8)
            requests = data[52:52 + 132 * count]
            rows = g.alloc(len(requests))
            g.cpu.mem_write(rows, requests)
            g.put(resource + 104, count)
            g.put(resource + 108, rows)
        elif kind == 0x21255D0:
            dimensions, = struct.unpack_from("<I", data, 8)
            g.put(resource + 220, (dimensions >> 6) & 8191)
            g.put(resource + 224, dimensions >> 19)
        if critical:
            g.put(resource + 80, 1)
        else:
            background.append(relative)
        paths.append(relative)
        resources[relative] = resource
        return resource

    g.stub(load_fn, load)
    g.stub(g.originals[INIT_HUNTER], lambda hunter, *_: native_calls.append(hunter) or 7)
    g.put(actor + 240, g.alloc(128))
    g.put(0x1896F5C, g.alloc(128))
    queued = []
    g.stub(0x4EEAA4, lambda _, __, resource, cue: queued.append((resource, cue)))
    inputs = g.alloc(40)
    tick = g.get(desc + 20)
    banks = actor + 3844 + 8 * 4
    g.put(banks + 7 * 4, 0x1234)  # An occupied slot stays owned by its original bank.
    before = bytes(g.cpu.mem_read(banks, 8 * 4))

    # The hook preserves native setup for stock and remote actors.
    g.cpu.mem_write(equipped + 1241, b"\x05")
    g.cpu.mem_write(equipped + 1236, bytes([weapon_id ^ 1]))
    assert g.run(GAME_STYLE, actor) == 5
    assert g.run(init, actor) == 7
    assert not paths
    g.cpu.mem_write(equipped + 1236, bytes([weapon_id]))
    assert g.run(GAME_STYLE, actor) == 0 and g.run(GAME_STYLE, remote) == 5
    assert g.run(init, remote) == 7
    assert not paths
    assert g.run(init, actor) == 7
    prefix = f"player/mot/frontier_{tag}"
    male, female = f"{prefix}/m_4.lmt", f"{prefix}/f_4.lmt"
    sounds = [f"sound/se/pl/frontier_{tag}/{bank}.srqr" for bank in audio]
    expected = {
        f"{prefix}/m_0.lmt", f"{prefix}/m_2.lmt", male, *sounds,
        f"effect/frontier_{tag}/particles.pel",
        *(f"player/hit/frontier_{tag}/{shape}.hts" for shape in shapes),
    }
    if hud:
        expected.add(f"GUI/frontier_{tag}/{hud}.tex")
    assert set(paths) == expected and not queued, (expected - set(paths), paths, queued)
    particles = f"effect/frontier_{tag}/particles.pel"
    assert set(background) == expected - {
        path for path in expected if path.endswith((".lmt", ".hts"))
    }
    assert paths[-len(background):] == background
    assert all(g.get(resources[path] + 80) == 0 for path in background)
    prepared = list(paths)
    assert g.run(init, actor) == 7 and paths == prepared

    # A failed request gives up its retained reference and can be queued again.
    failed = resources[particles]
    g.put(failed + 80, 0x10)
    assert g.run(init, actor) == 7 and paths == [*prepared, particles]
    assert released == [failed] and resources[particles] != failed
    prepared = list(paths)
    assert bytes(g.cpu.mem_read(banks, 8 * 4)) == before

    assert g.run(g.hooks[ENCODE], actor, 0x10000 | clip) == 0xE00 | (clip % 100)
    assert g.get(banks + 6 * 4) == resources[male]
    strikes = []
    attack = g.get(api_for(g, b"mhgu::StyleRegistryV1", b"SRG1") + 8)
    g.stub(attack, lambda _, data, *__: strikes.append(g.get(data + 16)) or 1)
    g.cpu.mem_write(actor + 5078, struct.pack("<h", 100))

    def frame(value, end=False):
        g.cpu.mem_write(actor + 1280, struct.pack("<f", value))
        g.cpu.mem_write(actor + 1206, bytes([4 if end else 0]))
        g.run(tick, actor, inputs)

    if weapon_id == 9:
        # A player can already use a private motion while presentation is pending.
        # No unfinished sound bank reaches the native queue and nothing reloads it.
        set_action(16, 0x8000 + 73)
        for sample in (0, 2, 6, 14, 24):
            frame(sample)
        assert g.half(actor + 1204) == 1478 and not queued and paths == prepared
        set_action(1, 0)
        frame(25)

    for path in background:
        g.put(resources[path] + 80, 1)

    if weapon_id == 1:
        set_action(16, 0x8002)
        for sample in (0, 4, 4, 14):
            frame(sample)
        assert g.half(actor + 1204) == clip
        assert queued == [(resources[sounds[2]], 4102), (resources[sounds[2]], 4101)], queued
        assert strikes == [resources["player/hit/frontier_sns/blade_6.hts"]]
    elif weapon_id == 8:
        set_action(16, 0x8000 + 10)
        for sample in (0, 4, 6):
            frame(sample)
        assert g.half(actor + 1204) == clip
        assert queued == [(resources[sounds[0]], 4098), (resources[sounds[1]], 4100)]
        frame(37, end=True)
        frame(40)
        assert strikes == [resources["player/hit/frontier_sa/hit_106_0.hts"]]
    elif weapon_id == 9:
        set_action(16, 0x8000 + 73)
        for sample in (0, 2, 6, 14, 24):
            frame(sample)
        assert queued == [
            (resources[sounds[0]], 4097),
            (resources[sounds[2]], 4098),
            (resources[sounds[2]], 4099),
            (resources[sounds[1]], 4170),
        ], queued
    elif weapon_id == 11:
        set_action(16, 0x8000 + 16)
        for sample in (0, 2, 8):
            frame(sample)
        assert g.half(actor + 1204) == clip
        assert queued == [(resources[sounds[2]], 4102), (resources[sounds[2]], 4097)]
        frame(27, end=True)
        frame(28)
        frame(36)
        assert g.half(actor + 1204) == 1404
        assert strikes == [0, 0]  # Both hits use GU's equipped weapon collision.
    assert paths == prepared

    # Reinitializing reuses the same resources. A different model sex loads
    # its own motion only, and first playback binds it to a free slot.
    g.run(init, actor)
    assert paths == prepared
    g.cpu.mem_write(equipped + 1240, b"\x01")
    g.run(init, actor)
    assert set(paths[len(prepared):]) == {f"{prefix}/f_0.lmt", f"{prefix}/f_2.lmt", female}
    prepared = list(paths)
    assert g.run(g.hooks[ENCODE], actor, 0x10000 | clip) == 0xD00 | (clip % 100)
    assert g.get(banks + 5 * 4) == resources[female] and paths == prepared


CASES = (
    ("great-sword", 0, 1, 0x1749A88, 1410, "gs", ("iron", "blade", "common"),
     ("guard_counter", "radiance"), None),
    ("dual-blades", 11, 2, 0x174F2B4, 1413, "db", ("blade", "common", "weapon"), (), "gauge"),
    ("sword-shield", 1, 5, 0x174A354, 1405, "sns", ("blade", "common", "weapon"),
     ("blade_6", "blade_7", "blade_23", "blade_28", "blade_29", "round_force"), None),
    ("long-sword", 7, 3, 0x174CF64, 1415, "ls", ("weapon", "common", "blade"),
     ("blade_0", "blade_32", "blade_33", "discharge", "instant", "release"), "gauge"),
    ("gunlance", 9, 6, 0x174E110, 1405, "gl", ("blade", "common", "weapon"), (), None),
    ("switch-axe", 8, 4, 0x174D838, 1412, "sa", ("weapon", "blade", "common"),
     tuple(f"hit_{family}_{group}" for family, count in ((106, 7), (107, 4)) for group in range(count)), "gauge"),
)


def check_case_coverage(repo):
    plugins = {path.parent.name for path in (repo / "plugins").glob("*-extreme/Cargo.toml")}
    covered = {f"{name}-extreme" for name, *_ in CASES}
    assert len(covered) == len(CASES), "Duplicate weapon acceptance cases"
    assert len({case[2] for case in CASES}) == len(CASES), "Duplicate style IDs"
    assert plugins == covered, (
        f"Missing weapon cases: {', '.join(sorted(plugins - covered)) or 'none'}. "
        f"Cases without a plugin: {', '.join(sorted(covered - plugins)) or 'none'}."
    )


def replay_load_order(repo, elf):
    config = b"[Weapons]\n" + b"".join(f"{weapon}={style}\n".encode() for _, weapon, style, *_ in CASES)
    modules = [PROVIDER]
    for name, *_ in CASES:
        mod = name + "-extreme"
        modules.append(repo / "plugins" / mod / "dist" / mod / "romfs/nativeNX/plugins" / (mod.replace("-", "_") + ".nro"))
    ordered = sorted(modules, key=lambda p: p.name)
    for paths in (ordered, [p for p in ordered if p != PROVIDER] + [PROVIDER]):
        g = boot(elf, config, ())
        for i, path in enumerate(paths):
            exports = g.load(path, 0x9000000 + i * 0x100000)
            for callback in ("forge_onLoad", "forge_onInit"):
                g.run(exports[callback], MANAGER)
        _, selected = service(g)
        found = [name for name, weapon, style, *_ in CASES if g.run(selected, weapon) == style]
        assert len(found) == len(CASES), found
    print("All six weapon styles registered in filename order and with the registry last.")


def replay(repo=REPO):
    check_case_coverage(repo)
    elf = (repo / "mhgu/main.elf").read_bytes()
    replay_load_order(repo, elf)
    from verify_manual import replay as verify_manual
    verify_manual(repo, elf, CASES)
    for case in CASES:
        print(f"Checking {case[0]}-extreme", flush=True)
        replay_style(repo, elf, *case)
    print("GU registration and hunt preload: motions, sound, particles, hit shapes and HUD passed")


if __name__ == "__main__":
    replay()
