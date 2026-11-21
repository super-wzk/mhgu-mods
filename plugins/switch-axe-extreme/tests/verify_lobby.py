"""Replay the shared GU action dispatcher for a lobby hunter equipped with w08."""

from verify_port import ROOT, REPO
from verify_style import (
    DISPATCH,
    ENCODE,
    boot,
    hunters,
    load_registered_style,
    service,
)


def replay():
    g = boot((REPO / "mhgu/main.elf").read_bytes(), b"[Weapons]\n8=4\n", ("registry",))
    nx = ROOT / "dist/switch-axe-extreme/romfs/nativeNX"
    load_registered_style(g, nx / "plugins/switch_axe_extreme.nro")
    quest, equipment, _ = hunters(g)
    g.cpu.mem_write(equipment + 1236, b"\x08")
    g.put(quest, 0x174D838)  # uPlayerQuest08
    manager = g.get(0x1885B58)
    # GU 28ED58 allocates 10048 bytes for this vtable. Pad the fixture so
    # that an erroneous write to the quest-only sword pose is detectable.
    lobby = g.alloc(13100)
    g.put(lobby, 0x1748974)
    g.put(lobby + 5060, equipment)
    g.put(lobby + 13096, 0xA5A5A5A5)
    g.put(0x18866F8 + 16, 0x1886678)  # Lobby subclass and base DTI
    calls = []
    g.stub(g.originals[DISPATCH], lambda actor, *_: calls.append(actor) or 7)

    g.put(manager + 28, quest)
    g.put(quest + 13096, 1)
    assert g.run(g.hooks[DISPATCH], quest) == 7
    assert g.get(quest + 13096) == 0

    # GU 28CFC0 calls this same dispatcher on the lobby object during setup.
    g.put(manager + 28, lobby)
    assert g.run(g.hooks[DISPATCH], lobby) == 7
    assert calls == [quest, lobby]
    assert g.get(lobby + 13096) == 0xA5A5A5A5, "style wrote beyond the lobby hunter"
    assert g.run(g.hooks[ENCODE], lobby, 5001) == 0x501
    _, selected = service(g)
    assert g.run(selected, 8) == 4

    g.put(manager + 28, quest)
    g.put(quest + 13096, 1)
    g.run(g.hooks[DISPATCH], quest)
    assert g.get(quest + 13096) == 0
    print("GU quest -> lobby -> quest dispatch preserved the lobby actor and style selection.")


if __name__ == "__main__":
    replay()
