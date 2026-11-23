"""Replay Extreme draw hits through GU's real affinity and KO calculation."""

import itertools
import struct
import zlib

from unicorn import UC_HOOK_CODE, arm_const as arm

from verify_weapon_arts import Scene, float_bits
from verify_style import CHANGE_ACTION, DB_DRAW_ROUTE, SA_DRAW_ROUTE, SNS_DRAW_ROUTE, quest_entries


class DrawScene(Scene):
    def __init__(self, weapon, style, name, vtable):
        super().__init__(weapon, style, name, vtable)
        g, actor = self.g, self.actor
        self.skills = set()
        self.hits = []
        self.walking = False

        def change(_, group, slot, flags):
            self.action(group, slot)
            # The registry's lifecycle test runs 2AB670 itself to verify this flag.
            pose = g.cpu.mem_read(actor + 10057, 1)[0] & ~14
            g.cpu.mem_write(actor + 10057, bytes([pose | (8 if flags & 0x80 else 0)]))

        def accept_command(*_):
            table = g.get(actor + 9384)
            command = g.get(g.get(g.get(table + 20)) + 8) + 100
            group, slot, flags = struct.unpack("<3H", g.cpu.mem_read(command, 6))
            if group == 0 or group & 0x8000:
                return 0
            change(actor, group, slot, flags)
            return 1

        self.stub(g.originals[CHANGE_ACTION], change)
        self.stub(0x2E9D54, accept_command)
        for addr in (0x2A2444, 0x2A2560):
            g.cpu.hook_del(g.traps.pop(addr))
        self.stub(0x27E608, lambda *_: int(self.walking))
        self.stub(0x27B918, lambda *_: 1)
        self.stub(0x2864C4, lambda _, skill, *rest: int(skill in self.skills))

        hdp = next(zlib.decompress(data) for name, kind, data in quest_entries(f"w{weapon:02}.arc")
                   if kind == 0x4E2EF008 and name.endswith(f"pl_we{weapon:02}_hitdata".encode()))
        raw, bank, vtable, shape = (g.alloc(size) for size in (len(hdp), 112, 72, 112))
        g.cpu.mem_write(raw, hdp)
        g.put(bank, vtable)
        g.put(vtable + 64, 0x270D8C)
        g.put(vtable + 68, 0x270D98)
        g.put(bank + 100, raw)
        g.put(bank + 104, raw + 16)
        g.put(actor + 12696, bank)
        g.put(actor + 12700, shape)
        g.put(actor + 5078, 100)
        g.put(actor + 5104, 4)
        g.cpu.mem_write(actor + 1206, b"\0")
        self.stub(0x13EFDC4, lambda dest, size, *_: g.cpu.mem_write(dest, bytes(size)))
        for addr in (0x887648, 0x887660, 0x2AD288):
            self.stub(addr, lambda *_: 0)
        for offset in (12320, 12504):
            g.run(0x1687A8, actor + offset)
        serials = itertools.count(1)
        self.stub(0x16B920, lambda *_: next(serials))
        # Provide an inert shape group for the live hit builder.
        manager, manager_vt, load, shape_vt, getter, group = (
            g.alloc(size) for size in (32, 64, 4, 72, 4, 16)
        )
        g.put(0x2126A64, manager)
        g.put(manager, manager_vt)
        g.put(manager_vt + 48, load)
        g.put(shape, shape_vt)
        g.put(shape_vt + 68, getter)
        g.put(group + 4, 3)
        g.put(group + 8, g.alloc(12))
        g.put(shape + 80, 1)
        self.stub(getter, lambda *_: group)
        self.stub(load, lambda _, kind, *__: shape if kind == 0x1857810 else 0)
        g.cpu.hook_add(UC_HOOK_CODE, self.capture, begin=0x2B76CC, end=0x2B76CC)

    def capture(self, cpu, *_):
        self.hits.append(cpu.reg_read(arm.UC_ARM_REG_R1))

    def change(self, group, slot, flags=0):
        self.g.run(self.g.hooks[CHANGE_ACTION], self.actor, group, slot, flags)

    def active(self):
        return self.g.half(self.actor + 9482)

    def marked_draw(self):
        return self.g.run(0x2A263C, self.actor) == 1

    def idle(self, drawn):
        self.change(1, 0)
        self.g.cpu.mem_write(self.actor + 10057, bytes([int(drawn)]))
        self.g.cpu.mem_write(self.actor + 1206, b"\0")

    def draw(self, route, kind):
        g = self.g
        ptr = g.run(g.hooks[route], self.actor, kind)
        self.change(*struct.unpack("<3H", g.cpu.mem_read(ptr, 6)))

    def frame(self, frame):
        g, actor = self.g, self.actor
        g.put(actor + 1280, float_bits(frame))
        g.put(actor + 1276, float_bits(frame + 2))
        self.hits.clear()
        self.tick()
        return [(g.half(hit + 144), g.cpu.mem_read(hit + 86, 1)[0]) for hit in self.hits]


def ground_draws():
    cases = (
        # Weapon, style, name, hunter vtable, draw selector and kind, state, frames, KO and hits.
        (1, 5, "sword-shield", 0x174A354, SNS_DRAW_ROUTE, 0, 9, (0, 80, 8), 30, 1, [(0, 5)]),
        (8, 4, "switch-axe", 0x174D838, SA_DRAW_ROUTE, 0, 10, (0, 80, 38, 40), 30, 1, []),
        # The light-sword draw is selected by the controller after unlock.
        (8, 4, "switch-axe", 0x174D838, None, 0x8004, 4, (0, 46, 38), 30, 1, []),
        (9, 6, "gunlance", 0x174E110, 0x11A9D08, 0, 20, (0, 85, 42), 30, 1, []),
        (11, 2, "dual-blades", 0x174F2B4, DB_DRAW_ROUTE, 0, 16, (0, 86, 28, 36), 8, 2, []),
        # GU's sheathed-run slot 73 becomes the Frontier dash slash.
        (11, 2, "dual-blades", 0x174F2B4, None, 73, 82, (4,), 8, 1, []),
    )
    # Converted SDT hitstop by state, including the paired shield/blade opener.
    factors = {(1, 9): (0, 1), (8, 10): (2,), (8, 4): (1,), (9, 20): (1,),
               (11, 16): (1, 1), (11, 82): (1,)}
    for weapon, style, name, vt, route, kind, state, frames, ko, hits, other in cases:
        s = DrawScene(weapon, style, name, vt)
        for drawn, skills in ((False, {91}), (False, {92}), (False, {91, 92}), (True, {91, 92})):
            s.skills = skills
            s.idle(drawn)
            if drawn:
                s.change(16, 0x8000 + state)
            elif route is None:
                s.change(16, kind)
            else:
                s.draw(route, kind)
            assert s.active() == 0x8000 + state, (name, hex(s.active()))
            contacts = []
            hitstop = []
            for frame in frames:
                contacts.extend(s.frame(frame))
                hitstop.extend(s.g.cpu.mem_read(hit + 108, 1)[0] for hit in s.hits)
            affinity = 100 if 91 in skills and not drawn else 0
            stun = ko if 92 in skills and not drawn else 0
            expected = other + [(affinity, stun)] * hits
            assert contacts == expected, (name, drawn, skills, contacts, expected)
            assert tuple(hitstop) == factors[weapon, state], (name, state, hitstop)


def aerial_hitstop():
    s = DrawScene(11, 2, "dual-blades", 0x174F2B4)
    s.idle(True)
    s.change(16, 0x8000 + 78)
    s.frame(0)
    for frame, hand in ((24, 12320), (30, 12504), (38, 12320), (46, 12504), (54, 12320)):
        s.frame(frame)
        assert s.hits == [s.actor + hand], (frame, s.hits)
        hit = s.hits[0]
        # SDT rows 242/243 both carry two ticks. Native HDP 61/62 differ.
        assert s.g.cpu.mem_read(hit + 108, 1) == b"\x01"
        s.g.cpu.mem_write(hit + 6, b"\x04")
        s.g.run(0x16DB68, 0, hit)
        assert struct.unpack("<2f", s.g.cpu.mem_read(s.actor + 10156, 8)) == (3., 0.)
        s.g.cpu.mem_write(s.actor + 10156, bytes(8))


def axe_charge():
    s = DrawScene(8, 4, "switch-axe", 0x174D838)
    g = s.g
    s.skills = {91, 92}
    g.put(s.actor + 28, float_bits(2))
    for drawn, walking, alternate, release, impact in (
        (False, False, False, 121, 56),
        (False, False, False, 116, 24),
        (False, True, False, 122, 24),
        (False, False, True, 125, 14),
        (True, False, False, 116, 24),
    ):
        s.walking = False
        s.idle(drawn)
        s.change(16, 0x8000 + 111)
        g.put(s.inputs, (1 << 27) if release != 121 else 0)
        s.frame(0)
        assert s.marked_draw() != drawn
        if release == 121:
            for frame in (20, 40):
                s.frame(frame)
        else:
            s.walking = walking
            s.frame(40 if walking else 56)
            assert s.active() == 0x8000 + (113 if walking else 112)
            assert s.marked_draw() != drawn
            if walking:
                for frame, moving in ((42, False), (44, True)):
                    s.walking = moving
                    s.frame(frame)
                    assert s.active() == 0x8000 + (113 if moving else 112)
                    assert s.marked_draw() != drawn
            for frame in range(62, 128, 2):
                s.frame(frame)
            g.put(s.inputs, (1 << 27) if alternate else 0)
            g.put(s.inputs + 16, (1 << 2) if alternate else 0)
            s.frame(128)
            g.put(s.inputs, 0)
            g.put(s.inputs + 16, 0)
        assert s.active() == 0x8000 + release, (release, hex(s.active()))
        assert s.marked_draw() != drawn
        assert s.frame(impact) == ([(0, 0)] if drawn else [(100, 30)]), release
        s.change(16, 0x8000 + 10)
        assert not s.marked_draw()
        contacts = [hit for frame in (0, 80, 38, 40) for hit in s.frame(frame)]
        assert contacts == [(0, 0)], (release, contacts)


if __name__ == "__main__":
    ground_draws()
    aerial_hitstop()
    axe_charge()
    print("GU draw skills: blade affinity, conditional KO and Switch Axe charge releases passed")
