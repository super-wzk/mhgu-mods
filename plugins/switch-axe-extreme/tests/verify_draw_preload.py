"""Check that native hunt setup caches the selected style's attack resources."""

import struct

from verify_style import ENCODE, INIT_HUNTER, SUBMIT_MODEL, api_for, boot, hunters, load_registered_style, setup_controller


def replay_style(repo, name, weapon_id, style, hunter_vtable, clip, tag, audio, shapes):
    config = f"[Weapons]\n{weapon_id}={style}\n".encode()
    g = boot((repo / "mhgu/main.elf").read_bytes(), config, ("registry",))
    plugin = name + "-extreme"
    nx = repo / "plugins" / plugin / "dist" / plugin / "romfs/nativeNX"
    desc = load_registered_style(g, nx / "plugins" / (plugin.replace("-", "_") + ".nro"))
    assert 0x9200000 <= g.hooks[SUBMIT_MODEL] < 0x9300000
    init = g.hooks[INIT_HUNTER]
    actor, equipped, remote = hunters(g)
    for hunter in (actor, remote):
        g.put(hunter, hunter_vtable)
    set_action = setup_controller(g, actor)
    g.stub(0x887648, lambda *_: 0)
    g.stub(0x887660, lambda *_: 0)

    manager, vtable, load_fn = (g.alloc(n) for n in (16, 52, 4))
    g.put(manager, vtable)
    g.put(vtable + 48, load_fn)
    g.put(0x2126A64, manager)
    paths = []
    resources = {}
    native_calls = []
    extensions = {
        0x2125210: ".lmt", 0x2125BEC: ".srqr", 0x1889FDC: ".pel",
        0x1857810: ".hts", 0x21255D0: ".tex",
    }

    def load(_, kind, path, flags):
        assert flags == 1
        assert native_calls[-1] == actor
        relative = g.string(path).replace("\\", "/") + extensions[kind]
        data = (nx / relative).read_bytes()
        resource = g.alloc(176)
        g.put(resource + 80, 1)
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
            resource = g.alloc(228)
            g.put(resource + 80, 1)
            g.put(resource + 220, 128 if weapon_id == 11 else 3072)
            g.put(resource + 224, 1024)
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
    g.cpu.mem_write(equipped + 1236, b"\x06")
    assert g.run(init, actor) == 7
    assert not paths
    g.cpu.mem_write(equipped + 1236, bytes([weapon_id]))
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
    if weapon_id != 0:
        expected.add(f"GUI/frontier_{tag}/gauge.tex")
    assert set(paths) == expected and not queued, (expected - set(paths), paths, queued)
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

    if weapon_id == 8:
        set_action(16, 0x8000 + 10)
        for sample in (0, 4, 6):
            frame(sample)
        assert g.half(actor + 1204) == clip
        assert queued == [(resources[sounds[0]], 4098), (resources[sounds[1]], 4100)]
        frame(37, end=True)
        frame(40)
        assert strikes == [resources["player/hit/frontier_sa/hit_106_0.hts"]]
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


def replay(repo):
    for case in (
        ("great-sword", 0, 1, 0x1749A88, 1410, "gs", ("iron", "blade", "common"),
         ("guard_counter", "radiance")),
        ("dual-blades", 11, 2, 0x174F2B4, 1413, "db", ("blade", "common", "weapon"), ()),
        ("long-sword", 7, 3, 0x174CF64, 1415, "ls", ("weapon", "common", "blade"),
         ("blade_0", "blade_32", "blade_33", "discharge", "instant", "release")),
        ("switch-axe", 8, 4, 0x174D838, 1412, "sa", ("weapon", "blade", "common"),
         tuple(f"hit_{family}_{group}" for family, count in ((106, 7), (107, 4)) for group in range(count))),
    ):
        replay_style(repo, *case)
    print("GU hunt setup caches the selected style's motions, sound, particles, hit shapes and HUD.")
