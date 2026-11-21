"""Check that native hunt setup caches first-draw resources for all four styles."""

import struct

from verify_style import ENCODE, INIT_HUNTER, SUBMIT_MODEL, boot, hunters, load_registered_style, setup_controller


def replay_style(repo, name, weapon_id, style, hunter_vtable, clip, tag, audio, particles):
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
    extensions = {0x2125210: ".lmt", 0x2125BEC: ".srqr", 0x1889FDC: ".pel"}

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
    expected = [f"{prefix}/m_0.lmt", male, *sounds]
    if particles:
        expected.append(f"effect/frontier_{tag}/particles.pel")
    assert paths == expected and not queued, (paths, queued)
    assert bytes(g.cpu.mem_read(banks, 8 * 4)) == before

    assert g.run(g.hooks[ENCODE], actor, 0x10000 | clip) == 0xE00 | (clip % 100)
    assert g.get(banks + 6 * 4) == resources[male]
    if weapon_id == 8:
        set_action(16, 0x8000 + 10)
        for frame in (0, 4, 6):
            g.cpu.mem_write(actor + 1280, struct.pack("<f", frame))
            g.run(tick, actor, inputs)
        assert g.half(actor + 1204) == clip
        assert queued == [(resources[sounds[0]], 4098), (resources[sounds[1]], 4100)]
    assert paths == expected

    # Reinitializing reuses the same resources. A different model sex loads
    # its own motion only, and first playback binds it to a free slot.
    g.run(init, actor)
    assert paths == expected
    g.cpu.mem_write(equipped + 1240, b"\x01")
    g.run(init, actor)
    expected += [f"{prefix}/f_0.lmt", female]
    assert paths == expected
    assert g.run(g.hooks[ENCODE], actor, 0x10000 | clip) == 0xD00 | (clip % 100)
    assert g.get(banks + 5 * 4) == resources[female] and paths == expected


def replay(repo):
    for case in (
        ("great-sword", 0, 1, 0x1749A88, 1410, "gs", ("iron",), False),
        ("dual-blades", 11, 2, 0x174F2B4, 1413, "db", ("blade", "common", "weapon"), False),
        ("long-sword", 7, 3, 0x174CF64, 1415, "ls", ("weapon", "common"), True),
        ("switch-axe", 8, 4, 0x174D838, 1412, "sa", ("weapon", "blade"), True),
    ):
        replay_style(repo, *case)
    print("GU hunt setup caches both draw banks and startup audio/effects for all four selected styles.")
