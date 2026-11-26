"""Replay GU weapon-art effects against the installed-format Frontier NROs."""
import struct
import subprocess
import sys
import zlib
from pathlib import Path

from unicorn import UC_HOOK_MEM_WRITE, arm_const as arm

REPO = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(REPO / "plugins/great-sword-extreme/tests"))
from verify_style import character_save, STYLE_IDS, api_for, boot, hunters, load_registered_style, quest_entries, setup_controller


def float_bits(value):
    return struct.unpack("<I", struct.pack("<f", value))[0]


class Scene:
    def __init__(self, weapon, name, vtable):
        g = self.g = boot((REPO / "mhgu/main.elf").read_bytes(),
                          character_save({weapon: STYLE_IDS[name + '-extreme']}), ("registry",))
        g.inline_hooks = g.inline_hooks | {0x2B4E7C, 0x11A4D8C}
        if weapon == 1:
            g.inline_hooks |= {0x116D464}
        if weapon == 11:
            g.inline_hooks |= {0x11BA074, 0x169D44}
        root = REPO / "plugins" / (name + "-extreme")
        module = root.name.replace("-", "_")
        desc = load_registered_style(g, root / "dist" / root.name / "romfs/nativeNX/plugins" / (module + ".nro"))
        # Observe published HUD values in the loaded NRO using its companion ELF.
        elf = root / "target/armv7a-none-eabi/release" / ("lib" + module + ".so")
        self.symbols = subprocess.check_output(["nm", str(elf)], text=True).splitlines()
        self.actor, eq, self.remote = hunters(g)
        a = self.actor
        g.cpu.mem_write(eq + 1236, bytes([weapon]))
        g.put(a, vtable)
        self.action = setup_controller(g, a)
        self.inputs = g.alloc(40)
        self.controller = g.get(desc + 20)
        g.put(a + 240, g.alloc(128))
        g.cpu.mem_write(a + 10057, b"\x01")
        g.put(a + 5088, float_bits(900))
        g.put(a + 1284, float_bits(240))
        g.put(a + 1188, 1)
        g.put(a + 5068, 1)
        self.action(1, 0)
        raw = next(zlib.decompress(data) for _, kind, data in quest_entries(f"w{weapon:02}.arc")
                   if kind == 0x496F8F22)
        magic, version, ints, floats, _ = struct.unpack_from("<4s4I", raw)
        assert magic == b"FUP\0" and version == 2
        params = struct.unpack_from(f"<{ints + floats}I", raw, 20)
        self.stub(0x282850, lambda _, index, *__: params[index])
        self.stub(0x282870, lambda _, index, *__: g.cpu.reg_write(arm.UC_ARM_REG_S0, params[ints + index]))
        self.stub(0x950BCC, lambda *_: 0)
        self.stub(0x27BCD0, lambda *_: 1)
        if weapon == 8:
            self.stub(0x2A4324, lambda *_: 0)  # the scene already supplies equipment
            g.run(0x11A8BBC, a, 0)  # native w08 quest/equip initializer
        self.tick()

    def field(self, name):
        symbol, = (s for s in self.symbols if f"6battle{len(name)}{name}" in s
                   and s.split()[1] == "b" and s.split()[2].split(".")[0].endswith(name))
        return 0x9200000 + int(symbol.split()[0], 16)

    def tick(self):
        self.g.run(self.controller, self.actor, self.inputs)

    def stub(self, pc, fn):
        if pc in self.g.traps:
            self.g.cpu.hook_del(self.g.traps.pop(pc))
        self.g.stub(pc, fn)


def switch_axe():
    s = Scene(8, "switch-axe", 0x174D838)
    g, a, hud = s.g, s.actor, s.field("HUD")
    def meters():
        s.tick()
        packed = g.get(hud)
        return g.get(a + 13100), packed & 65535, packed >> 16

    def riot_effect(_, *__):
        if g.cpu.reg_read(arm.UC_ARM_REG_LR) != 0x11A4E9C:
            return 0
        # Simulate any stock reload during the callback. The extreme style
        # replaces it with a partial recovery from the pre-art gauge value.
        g.put(a + 13100, 100)
        return 1

    assert meters() == (50, 150, 0)
    g.put(a + 13100, 0)
    s.action(16, 85)
    g.cpu.mem_write(a + 5, b"\1")
    s.stub(0x280C1C, riot_effect)
    g.run(0x11A08E0, a)
    assert meters() == (25, 300, 1)
    # The stock no-input morph command uses this virtual to detect an empty sword.
    g.put(a + 13100, 0)
    assert g.run(0x2B4E7C, a) == 0
    assert g.run(0x2B4E7C, s.remote) == 1

    g.put(a + 13120, 0)
    g.run(s.controller, 0, 0)
    s.action(1, 0)
    assert meters() == (0, 150, 0)  # reselecting the style never resets native slash
    for tier, (slash, starting_light) in enumerate(((25, 150), (90, 0), (100, 0))):
        s.action(1, 0)
        g.put(a + 13100, slash)
        assert meters()[:2] == (slash, starting_light)
        s.action(16, 85 + tier)
        g.cpu.mem_write(a + 5, b"\1")
        s.stub(0x280C1C, lambda *_: 0)
        g.run(0x11A08E0, a)
        assert meters()[:2] == (slash, starting_light)  # windup
        s.stub(0x280C1C, riot_effect)
        g.run(0x11A08E0, a)
        light = 300
        assert g.get(a + 13120) == tier + 1 and meters() == (min(100, slash + 25), light, 1)
        assert g.get(a + 13096) == 1
        s.action(1, 0)
        # An independent GU change is immediately visible while Riot is active.
        g.put(a + 13100, 9)
        assert meters() == (9, light, 1)
        g.put(a + 13100, 0)
        assert g.run(0x2B4E7C, a) == 0
        g.put(s.inputs + 16, 1 << 2)  # A alone opens the light-sword double slash.
        s.tick()
        assert g.half(a + 9482) == 0x8000 + 78
        g.put(s.inputs + 16, 0)
        for frame in (46, 90):
            g.put(a + 1280, float_bits(frame))
            assert meters() == (0, light, 1)
        s.action(16, 88 + tier)
        g.cpu.mem_write(a + 5, b"\1")
        g.put(a + 10084, tier)
        s.stub(0x280C1C, lambda *_: 1)
        g.run(0x11A08E0, a)
        filled = (75, 100, 100)[tier]
        assert meters()[:2] == (filled, light)
        assert g.run(0x11A8E74, a) == (0, 10, 30)[tier]
        s.action(16, 85 + tier)
        g.cpu.mem_write(a + 5, b"\1")
        s.stub(0x280C1C, riot_effect)
        g.run(0x11A08E0, a)
        assert meters()[:2] == (filled, light)  # no repeat recovery during the same activation
        g.put(a + 13128, 0)
        writes = []
        watch = g.cpu.hook_add(UC_HOOK_MEM_WRITE, lambda *args: writes.append(args[4]),
                               begin=a + 13100, end=a + 13103)
        try:
            g.put(a + 28, float_bits(2))
            g.put(a + 13132, float_bits(10))
            g.put(a + 13140, float_bits(20))
            g.run(g.hooks[0x11A75F8], a)
            assert g.get(a + 13132) == float_bits(8) and g.get(a + 13140) == float_bits(18)
        finally:
            g.cpu.hook_del(watch)
            g.put(a + 28, 0)
        assert not writes, writes  # draining light must never borrow the GU slash field
        light -= 3
        assert meters()[:2] == (filled, light)
        assert g.get(a + 13128) == float_bits((63, 72, 63)[tier])
        g.run(g.hooks[0x11A75F8], a)
        assert meters()[:2] == (filled, light)
        # Let the last pulse land in drawn idle, during a swing, or while walking.
        finish_swing = tier == 1
        before_morph = (16, 0x8000 + 78) if finish_swing else (2 if tier == 2 else 1, 0)
        s.action(*before_morph)
        g.put(a + 1280, 0)
        assert meters()[:2] == (filled, light)
        while light:
            g.put(a + 13128, 0)
            g.run(g.hooks[0x11A75F8], a)
            light = max(0, light - 3)
            if light == 0:
                assert g.get(a + 13120) == 0
                assert g.get(a + 13096) == 1  # the upkeep hook leaves the sword pose intact
            assert meters()[:2] == (filled, light)
        if finish_swing:
            assert (g.half(a + 9478), g.half(a + 9482)) == before_morph
            assert g.get(a + 13096) == 1
            s.stub(0x2B6E5C, lambda *_: s.action(1, 0))
            g.cpu.mem_write(a + 1206, b"\4")
            s.tick()
            g.cpu.mem_write(a + 1206, b"\0")
            assert (g.half(a + 9478), g.half(a + 9482)) == (1, 0)
            s.tick()
        assert g.get(hud) >> 16 == 0
        assert (g.half(a + 9478), g.half(a + 9482)) == (16, 0x8000 + 31)
        assert g.half(a + 1204) == 1441  # clip requested from the motion fixture
        assert g.get(a + 13096) == 1
        for frame, sword in ((63, 1), (64, 0)):
            g.put(a + 1280, float_bits(frame))
            s.tick()
            assert g.get(a + 13096) == sword
    g.put(a + 13100, 0)
    assert g.run(0x2B4E7C, a) == 1  # the unmodified axe predicate resumes at expiry
    sword_phial(Scene(8, "switch-axe", 0x174D838))
    sword_deflection(Scene(8, "switch-axe", 0x174D838))
    print("Switch Axe: Riot recovery, Energy Charge, timed light drain and animated morph passed")


def sword_deflection(s):
    g, a = s.g, s.actor
    equipment = g.get(a + 5060)
    weapon_vtable = g.get(a)
    hit = a + 12320
    sharpness_grade = g.get(weapon_vtable + 508)
    assert sharpness_grade == 0x26893C  # GU w08 live sharpness getter.
    shape, manager, vt, load, row = (g.alloc(n) for n in (128, 32, 64, 4, 56))
    target, target_vt, part, zone, factor, element, noop = (
        g.alloc(n) for n in (48000, 1400, 32, 16, 4, 4, 4)
    )
    g.put(shape + 80, 1)
    g.put(a + 12700, shape)
    g.put(0x2126A64, manager)
    g.put(manager, vt)
    g.put(vt + 48, load)
    s.stub(load, lambda _, kind, *__: shape if kind == 0x1857810 else 0)
    for address in (0x887648, 0x887660, 0x168960, 0x1696C4, 0x2B76CC,
                    0x2B7ED4, 0xDF814, 0xDD4C4, 0x2AD288):
        s.stub(address, lambda *_: 0)
    for offset in (440, 520, 536, 1400):
        s.stub(g.get(weapon_vtable + offset), lambda *_: 0)
    s.stub(g.originals[0x2A8A88], lambda *_: row)
    # Supply a stationary target and let GU compute the contact factor.
    g.put(target, target_vt)
    g.put(target_vt + 796, noop)
    s.stub(0xBAB60, lambda *_: zone)
    for address in (noop, 0xBAD1C, 0x2864D4):
        s.stub(address, lambda *_: 0)
    s.stub(0x3A8D68, lambda *_: 5)
    g.put(a + 11120, 100)
    for state, frame, grade, affinity, hitzone, expected in (
        (5, 38, 0, 30, 10, 35),
        (5, 38, 1, 30, 10, 35),
        (5, 38, 0, -20, 10, 35),
        (5, 38, 2, 0, 10, 5),
        (5, 38, 0, 0, 80, 5),
        (70, 70, 1, 0, 10, 70),
    ):
        for action_id in (30, state):
            s.action(16, 0x8000 + action_id)
            g.put(a + 1280, 0)
            s.tick()
        # Affinity is stored on equipment. Sharpness changes independently on the hunter.
        g.cpu.mem_write(equipment + 802, struct.pack("<h", affinity))
        g.put(a + 5104, grade)
        g.cpu.mem_write(zone, bytes((hitzone, hitzone)))
        g.put(factor, float_bits(1.0))
        assert g.run(sharpness_grade, a) == grade
        assert g.run(0x2AD190, a, 0) == (affinity & 0xFFFFFFFF)
        g.put(hit + 48, 0)
        g.put(a + 1280, float_bits(frame))
        s.tick()
        assert g.get(hit + 48) and g.get(a + 13096) == 1
        g.run(g.hooks[0x178030], 0, hit, target, part, part, a, factor, element)
        ratio, = struct.unpack("<f", g.cpu.mem_read(factor, 4))
        assert (ratio < 0.23) == (hitzone == 10), (grade, hitzone, ratio)
        g.put(a + 1280, float_bits(frame + 2))
        s.tick()
        assert g.half(a + 9482) == 0x8000 + expected, (state, grade, affinity, hitzone, expected)
    print("Switch Axe: GU hitzone, live sharpness, affinity independence and FZ discharge exemption passed")


def sword_phial(s):
    g, a = s.g, s.actor
    shape, manager, vt, load = (g.alloc(size) for size in (128, 32, 64, 4))
    g.put(shape + 80, 1)
    g.put(manager, vt)
    g.put(vt + 48, load)
    g.put(0x2126A64, manager)
    s.stub(0x887660, lambda *_: 0)  # fixture shape belongs to the scene
    s.stub(load, lambda _, kind, *__: shape if kind == 0x1857810 else 0)
    flags = []
    registry = api_for(g, b"mhgu::StyleRegistryV3", b"SRG3")
    s.stub(g.get(registry + 8), lambda _, attack, *__: flags.append(g.get(attack + 12)) or 1)
    slash_skills(s)
    for state, frame, sword in ((0, 56, False), (70, 70, True)):
        flags.clear()
        s.action(16, 0x8000 + state)
        g.put(a + 1280, 0)
        s.tick()
        g.put(a + 1280, float_bits(frame))
        s.tick()
        assert flags and all(bool(f & 8) == sword for f in flags), (state, flags)

    # Isolate GU's phial modifier in the hit constructor. Use the real
    # phial multipliers and the retail status bit generated by Demon Riot.
    hit, row = a + 12320, g.alloc(56)
    s.stub(g.get(g.get(a) + 348), lambda _, index, *__: phial if index == 27 else 0)
    s.stub(0x282F58, lambda *_: 0)
    g.cpu.mem_write(a + 13124, b"\1")
    for phial, offset, normal in ((0, 56, 120), (1, 80, 125)):
        for level, multiplier in enumerate((1, 1.05, 1.1, 1.2)):
            g.put(a + 13120, level)
            g.put(a + 10336, 0)
            s.action(16, 0x8000 + 70)
            g.run(0x11A9034, a)
            g.cpu.mem_write(hit + offset, struct.pack("<H", 100))
            g.run(0x2B76CC, a, hit, row, flags[0] & 8)
            assert abs(g.half(hit + offset) - int(normal * multiplier)) <= 1


def slash_skills(s):
    g, a = s.g, s.actor
    part, target, factor = (g.alloc(32) for _ in range(3))
    g.put(factor, float_bits(1))
    s.stub(g.originals[0x178030], lambda *_: 0)
    s.stub(0x280C1C, lambda *_: 1)
    for skill, two_hits, reload in ((0, 12, 75), (152, 14, 90), (153, 8, 60)):
        s.stub(0x2864C4, lambda _, candidate, *__: int(skill == candidate))
        g.put(a + 13100, 10)
        s.action(16, 0x8000)
        g.put(a + 1280, 0)
        s.tick()
        g.put(a + 1280, float_bits(56))
        s.tick()
        for _ in range(2):
            g.run(g.hooks[0x178030], 0, a + 12320, target, part, 0, a, factor, 0)
        assert g.get(a + 13100) == 10 + two_hits
        s.tick()
        assert g.get(a + 13100) == 10 + two_hits
        s.action(16, 0x8000 + 106)  # axe sidestep costs seven with every skill
        g.put(a + 1280, 0)
        s.tick()
        assert g.get(a + 13100) == 3 + two_hits, (skill, g.get(a + 13100))
        s.action(16, 88)  # Energy Charge I
        g.put(a + 10084, 0)
        g.cpu.mem_write(a + 5, b"\1")
        g.put(a + 13100, 0)
        g.run(0x11A08E0, a)
        assert g.get(a + 13100) == reload
        s.tick()
        assert g.get(a + 13100) == reload
    s.stub(0x2864C4, lambda *_: 0)
    # Ordinary axe upkeep goes to GU once. A style tick neither regenerates
    # the gauge independently nor reinitializes the native recovery delay.
    s.action(1, 0)
    g.cpu.mem_write(a + 5078, struct.pack("<h", 100))
    g.put(a + 13100, 10)
    g.put(a + 13104, float_bits(2))
    g.put(a + 28, float_bits(2))
    s.tick()
    assert g.get(a + 13100) == 10 and g.get(a + 13104) == float_bits(2)
    g.run(g.hooks[0x11A75F8], a)
    regenerated = g.get(a + 13100)
    assert regenerated > 10 and g.get(a + 13104) != 0
    s.tick()
    assert g.get(a + 13100) == regenerated
    # Light-sword idle and walking use the same native slash clock and skill scaling.
    g.put(a + 13096, 1)
    for skill, state, slash, expected in (
        (0, (1, 0), 10, 15), (152, (2, 30), 10, 16), (153, (1, 0), 10, 14),
        (0, (1, 0), 98, 100),
    ):
        s.stub(0x2864C4, lambda _, candidate, *__: int(skill == candidate))
        s.action(*state)
        g.put(a + 13100, slash)
        g.put(a + 13104, float_bits(4))
        g.run(g.hooks[0x11A75F8], a)
        assert g.get(a + 13100) == slash and g.get(a + 13104) == float_bits(2)
        g.run(g.hooks[0x11A75F8], a)
        assert g.get(a + 13100) == expected and g.get(a + 13104) == float_bits(300)
        assert g.get(a + 13096) == 1
    s.stub(0x2864C4, lambda *_: 0)
    g.put(a + 28, 0)
    print("GU slash meter: axe and light-sword passive recovery, two axe contacts, sidestep cost and Energy Charge under Focus/Distraction passed")


def long_sword():
    s = Scene(7, "long-sword", 0x174CF64)
    g, a, cut = s.g, s.actor, s.field("CUT")
    hit = a + 12320
    # Bind GU's color virtual to the built detour, preserving its native
    # one-contact latch in 119F574. Other native moves can also request +1.
    g.put(g.get(a) + 2064, g.hooks[0x119ED98])
    for slot, expected in ((30, 0), (54, 425), (62, 850), (54, 850)):
        s.action(16, slot)
        g.cpu.mem_write(a + 13116, b"\1\1")
        for flags, kind in ((0, 1), (0x4000, 0), (0x4000, 1), (0x4000, 1)):
            g.put(hit + 48, flags)
            previous = g.get(cut)
            g.run(0x119F574, a, hit, 1, kind)
            assert g.get(cut) == (expected if flags and kind else previous)
            assert g.get(a + 13104) == 0
    g.cpu.hook_del(g.traps.pop(0x2A309C))
    for duration, remaining in ((0, 60), (120, 85)):
        g.put(a + 10112, 85)
        g.put(a + 13112, float_bits(duration))
        g.run(0x2A309C, a, (-25) & 0xFFFFFFFF, 0)
        assert g.get(a + 10112) == remaining
    print("Long Sword: Sakura contact, repeat hit, 425/850 cap, no color and Unhinged Spirit cost passed")


def dual_blades():
    s = Scene(11, "dual-blades", 0x174F2B4)
    g, a = s.g, s.actor
    s.action(16, 0x8000)
    s.stub(g.get(g.get(a) + 900), lambda *_: 0)  # contact flash
    s.stub(0x3A5ED0, lambda *_: 0)
    g.put(a + 28, float_bits(2))
    row = g.alloc(56)
    target, other = g.alloc(16), g.alloc(16)
    hp = a + 5078
    cases = ((False, 60, 60), (True, 60, 61), (True, 100, 100), (True, 0, 0), (False, 60, 60))
    for hand, offset in enumerate((12320, 12504)):
        hit, armed = a + offset, a + 13156 + 4 * hand
        for active, initial_hp, expected_hp in cases:
            g.put(a + 13144, float_bits(600 if active else 0))
            g.cpu.mem_write(hp, struct.pack("<3h", initial_hp, 100, initial_hp))
            g.cpu.mem_write(hit + 56, struct.pack("<h", 100))
            g.run(g.hooks[0x11BAD38], a, hit, row, 1)
            assert g.get(armed) == int(active)
            g.run(0x169D44, hit, target, 1)
            assert g.half(hp) == initial_hp
            g.run(0x11BA9C8, a, hit, 1, 1)
            assert g.get(armed) == (2 if active else 0)
            if not active:
                continue

            g.put(a + 13164, 3)
            for _ in range(60):
                g.run(0x11BA074, a)
                if not g.get(armed):
                    break
            assert g.get(armed) == 0 and 0 < g.half(hit + 56) < 100
            assert g.cpu.mem_read(hit + 5, 1) == b"\1"
            for contact, victim, kind in ((hit, target, 0), (hit, 0, 1), (s.remote + offset, target, 1)):
                g.run(0x169D44, contact, victim, kind)
                assert g.half(hp) == initial_hp
            g.run(0x169D44, hit, target, 1)
            assert g.half(hp) == expected_hp, (hand, initial_hp, g.half(hp))
            g.run(0x169D44, hit, other, 1)
            assert g.half(hp) == expected_hp
    print("Dual Blades: native Wolf's Maw follow-ups heal once per hand; base hits, expiry, cap and fainting passed")


if __name__ == "__main__":
    switch_axe()
    long_sword()
    dual_blades()
