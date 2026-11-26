"""Exercise the release NRO with GU 1.4 code, synthetic hunters and in-memory saves."""
from pathlib import Path
import struct
import sys

ROOT = Path(__file__).resolve().parents[1]
REPO = ROOT.parents[1]
sys.path.insert(0, str(ROOT.parent / "great-sword-extreme/tests"))
sys.path.insert(0, str(ROOT.parent / "mhgu-sdk-runtime/tests"))
from verify_style import character_save, BLOCK, CHANGE_ACTION, api_for, boot, hunters, load_registered_style, service, setup_controller

def main():
    g = boot((REPO / "mhgu/main.elf").read_bytes(), character_save({8: 13}), ("registry",))
    desc = load_registered_style(g, ROOT / "dist/switch-axe-extreme/romfs/nativeNX/plugins/switch_axe_extreme.nro")
    _, selected = service(g)
    assert (g.get(desc), g.get(desc+4), g.run(selected,8), g.run(selected,7)) == (13,8,13,0)
    actor, eq, remote = hunters(g)
    g.cpu.mem_write(eq+1236, bytes([8]))
    g.put(actor,0x174d838)
    set_action = setup_controller(g,actor)
    g.put(actor+240,g.alloc(128))
    inputs = g.alloc(40)
    tick = g.get(desc+20)
    def frame(n):
        g.cpu.mem_write(actor+1280, struct.pack("<f",n))
        g.run(tick,actor,inputs)
    def state():
        return g.half(actor+9482)-0x8000
    g.stub(0x2b6e5c,lambda *_:set_action(1,0))
    g.stub(0x2a69dc,lambda *_:0)
    g.cpu.hook_del(g.traps.pop(0x283238))
    g.cpu.mem_write(actor+5088,struct.pack('<f',900.))
    g.cpu.mem_write(actor+1284,struct.pack('<f',180.))
    set_action(1,0)
    g.put(inputs+16,1<<14)
    frame(0)
    assert (state(),g.half(actor+1204))==(0,1401)
    g.put(inputs+16,0)
    frame(56)
    g.put(inputs+16,1<<14)
    frame(58)
    g.put(inputs+16,0)
    assert state()==0
    frame(80)
    assert (state(),g.half(actor+1204))==(40,1410)
    set_action(1,0)
    g.put(inputs+36,1)
    frame(0)
    g.put(inputs+36,0)
    assert state()==89
    out=g.alloc(12)
    assert g.run(g.get(desc+60),actor,out)==2
    frame(24)
    assert g.run(g.get(desc+60),actor,out)==1
    reg=api_for(g,b"mhgu::StyleRegistryV3",b"SRG3")
    # GU 2F0670 rechecks the guard, applies its reaction and requests recoil.
    # Substitute only the outer scene's action setup and recoil animation.
    g.put(actor + 5068, 1)
    g.stub(g.originals[CHANGE_ACTION], lambda _, group, slot, __: set_action(group, slot) or 1)
    g.stub(0x2C4EEC, lambda *_: 0)
    damage, absorbed = g.alloc(4), g.alloc(4)
    g.put(damage, 35)
    def impact(owner):
        g.cpu.mem_write(actor + 12826, struct.pack("<H", owner))
        return g.run(0x2F0670, actor, 0, damage, absorbed)

    assert g.run(BLOCK, actor, 0) == 0  # outside the window
    skills = set()
    g.cpu.hook_del(g.traps.pop(0x2864C4))
    g.stub(0x2864C4, lambda _, skill, *__: int(skill in skills))
    for skill, end in ((0, 24), (169, 26), (168, 28), (279, 28), (297, 28)):
        skills.clear()
        skills.add(skill)
        set_action(16, 0x8000 + 89)
        frame(0)
        frame(end - 1)
        assert g.run(BLOCK, actor, 0) == 1
        frame(end)
        assert g.run(BLOCK, actor, 0) == 0
    skills.clear()
    set_action(16, 0x8000 + 89)
    frame(0)
    assert impact(0xffff) == 1
    frame(8)
    assert state() == 58
    for _ in range(2):
        set_action(16,0x8000+89)
        frame(0)
        assert g.run(g.originals[BLOCK], actor, 0) == 0  # stock w08
        assert g.run(BLOCK, actor, 0x4001) == 0
        assert g.run(BLOCK, actor, 0x4000) == 1
        for flags, kind, power, expected in (
            (0, 0x19, 0, 0), (0x100, 0, 0, 0), (0, 0, -1, 0),
            (0x20, 0, 0, 0), (0x40, 0, 0, 1),
        ):
            g.cpu.mem_write(actor + 12820, struct.pack("<HB", flags, kind))
            g.cpu.mem_write(actor + 12828, struct.pack("<h", power))
            assert g.run(BLOCK, actor, 0x8000 if flags == 0x40 else 0) == expected
        g.cpu.mem_write(actor + 12820, struct.pack("<H", 0x20))
        skills.add(171)
        assert g.run(BLOCK, actor, 0) == 1
        skills.clear()
        g.cpu.mem_write(actor + 12820, bytes(2))
        assert impact(123) == 1
        assert (g.half(actor+9478), g.half(actor+9482)) == (32, 3)
        frame(8)
        assert state()==90
    set_action(1,0)
    g.put(inputs,1<<8)
    frame(0)
    g.put(inputs,0)
    assert state()==94
    frame(124)
    assert (state(),g.half(actor+1204))==(110,1476)
    frame(180)
    assert g.half(actor+9478)==1
    g.put(inputs+16,1<<2)
    frame(0)
    assert state()==78
    for owner, follow in ((0xffff, 59), (123, 92)):
        set_action(16, 0x8000 + 91)
        frame(0)
        assert impact(owner) == 1
        frame(8)
        assert state() == follow
    assert g.get(damage) == 35
    assert impact(123) == 0  # success animation itself cannot absorb a second hit
    g.cpu.hook_del(g.traps.pop(0x27E608))
    g.cpu.hook_del(g.traps.pop(0x27F5D0))
    yaw = 0
    g.stub(0x27F5D0, lambda *_: -yaw & 65535)
    for guard, monster, other, slash in ((89, 90, 58, 106), (91, 92, 59, 152)):
        for yaw, offset in ((-0x4000, 1), (0x4000, 2), (0x8000, 0)):
            for owner, recovery in ((123, monster), (0xffff, other)):
                set_action(16, 0x8000 + guard)
                g.put(actor + 4076, 0)
                g.cpu.mem_write(actor + 5136, struct.pack("<h", 100))
                frame(0)
                assert impact(owner) == 1
                g.put(inputs + 16, 1 << 6)  # w08's combined X+A command
                frame(8)
                assert state() == recovery
                g.put(inputs + 16, 0)
                if owner == 123:
                    frame(8)
                    assert state() == recovery
                g.cpu.mem_write(actor + 5136, b"\0\0")
                frame(10)
                assert state() == slash + offset, (owner, recovery, yaw, state())
    g.stub(0x27E608, lambda *_: 0)
    g.cpu.hook_del(g.traps.pop(g.originals[CHANGE_ACTION]))
    print("GU absorption: stock w08 rejection, guard window, facing, axe and sword recoil follow-ups passed.")
    from verify_hud import replay
    replay(g, actor, lambda: g.run(tick,actor,0))
    from verify_branches import replay as branches
    branches(g, REPO, actor, remote, tick, inputs, set_action, reg)
    from verify_routes import replay as routes
    routes(REPO, ROOT)
    from verify_stance import replay as stance
    stance(REPO, ROOT)
    from verify_weapon_styles import replay as weapon_styles
    weapon_styles(REPO)
    print("GU NRO: weapon selection, axe strike, buffered combo, guard window, absorption, light-sword stance and sword attack passed.")

if __name__=="__main__":
    main()
