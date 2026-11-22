"""Mash w08 attacks through GU's mapped input and the shipped LMT loop clocks."""
import math
import struct
import subprocess
from unicorn import arm_const as arm
from verify_style import BLOCK, COMMAND_UPDATE, DISPATCH, WEAPON_DISPATCH, api_for
from verify_branches import Frontier


def replay(g, root, actor, form, weapon, set_action):
    elf = root / "target/armv7a-none-eabi/release/libswitch_axe_extreme.so"
    symbols = subprocess.check_output(["nm", str(elf)], text=True).splitlines()
    hud_symbol, = (line for line in symbols if line.split()[-1].split(".")[0].endswith("6battle3HUD"))
    hud = 0x9200000 + int(hud_symbol.split()[0], 16)

    def number(offset, value):
        g.cpu.mem_write(actor + offset, struct.pack("<f", value))

    def stamina():
        return struct.unpack("<f", g.cpu.mem_read(actor + 5088, 4))[0]

    def recover_stamina():
        # Run the passive recovery block inside GU's common hunter update.
        g.cpu.reg_write(arm.UC_ARM_REG_R4, actor)
        g.cpu.reg_write(arm.UC_ARM_REG_R7, actor + 9478)
        g.cpu.reg_write(arm.UC_ARM_REG_R8, actor + 10057)
        g.run(0x29DBDC, until=0x29DE64)

    def state():
        return g.half(actor + 9482) - 0x8000

    previous = 0
    def pad(frame, held=0, *, stick=0, wrap=False):
        nonlocal previous
        g.cpu.mem_write(actor + 5108, struct.pack("<3H", held, previous & ~held, held & ~previous))
        previous = held
        g.cpu.mem_write(actor + 5140, bytes(32))
        g.cpu.mem_write(actor + 5132, struct.pack("<H", (aim + 0x4000) & 0xffff))
        g.cpu.mem_write(actor + 5136, struct.pack("<h", stick))
        number(1280, frame)
        g.cpu.mem_write(actor + 1206, bytes([4 if wrap else 0]))
        g.run(0x27CA28, actor)
        g.run(0x11A03A8, actor)
        g.run(g.hooks[WEAPON_DISPATCH], actor)
        return state()

    number(5092, 1500)  # GU 2A3E50 clamps stamina to this hunter's maximum.
    number(1268, 0)
    shape, manager, vt, load = [g.alloc(n) for n in (128, 32, 64, 4)]
    g.put(shape + 80, 1)
    g.stub(0x887660, lambda *_: 0)  # fixture owns the shared HTS
    old_manager = g.get(0x2126A64)
    g.put(manager, vt)
    g.put(vt + 48, load)
    g.put(0x2126A64, manager)
    g.stub(load, lambda _, kind, *__: shape if kind == 0x1857810 else 0)
    api = api_for(g, b"mhgu::StyleRegistryV1", b"SRG1")
    hits = []
    submit = g.get(api + 8)
    g.stub(submit, lambda *_: hits.append((state(), g.half(actor + 1204))) or 1)

    reference = Frontier(root.parents[1] / "mhfz/eboot.bin.elf")
    facing = 0x1234
    aim = facing
    if 0x27F5D0 in g.traps:
        g.cpu.hook_del(g.traps.pop(0x27F5D0))
    sectors = (
        (0, 29), (0x1FFF, 29), (0x2000, 18), (0x4000, 18), (0x6000, 18),
        (0x60B6, 18), (0x6100, 19), (0x7FFF, 19), (-0x8000, 19),
        (-0x6100, 19), (-0x60B6, 17), (-0x6000, 17), (-0x4000, 17),
        (-0x2000, 17), (-0x1FFF, 29),
    )

    def enter(action, *, sword=False, energy=100):
        nonlocal previous
        form(sword)
        g.put(actor + 13100, energy)
        g.put(actor + 4076, facing)
        g.put(actor + 5280, facing)
        g.put(actor + 5188, 0)
        set_action(16, 0x8000 + action)
        previous = 0
        # Separate joystick trials by more than the sprint double-push window.
        for _ in range(16):
            pad(0)

    def tap(button, frame, *, stick=100):
        # GU can resolve the chord immediately and stagger a single button.
        for n in range(frame, frame + 6, 2):
            prior = state()
            active = pad(n, button, stick=stick)
            if active != prior:
                return active
        return state()

    def refill_light():
        for _ in range(4):
            set_action(16, 0x8000 + 90)
            pad(0)
            pad(8)
        assert g.get(hud) & 0xffff == 300

    for sword, opener in ((False, 0), (True, 5)):
        if sword:
            refill_light()
        for yaw, strength, axe_action in (
            (0, 0, 2), (0, 100, 2), (-0x4000, 49, 2), (-0x4000, 50, 107),
            (0x4000, 100, 108), (-0x8000, 100, 106),
        ):
            aim = (facing - yaw) & 0xffff
            enter(opener, sword=sword)
            set_action(1, 0)
            pad(0, 0x120, stick=strength)
            g.run(g.hooks[DISPATCH], actor)
            assert state() == (70 if sword else axe_action), (sword, yaw, strength, state())

    # Both light-sword branches are open at frame 56 of the standing X slash.
    aim = (facing + 0x4000) & 0xffff
    for strength, target in ((49, 70), (50, 153)):
        for first in (0, 0x20, 0x100):
            refill_light()
            enter(5, sword=True)
            if first:
                assert pad(55, first, stick=strength) == 5
            assert pad(56, 0x120, stick=strength) == target, (strength, first, state())

    # Start a thrust with the native chord, then restore its directional chord
    # from GU's pad state when the weapon mapper omits X+A.
    for presses, strength, target in (
        (((4, 0x20), (72, 0x120)), 100, 153),
        (((4, 0x100), (72, 0x120)), 100, 153),
        (((68, 0x40), (70, 0x160)), 100, 153),
        (((4, 0), (70, 0x160)), 100, 153),
        (((4, 0), (90, 0x20), (92, 0x20), (94, 0x20), (96, 0x120)), 100, 153),
        (((4, 0x20), (72, 0x120)), 49, 70),
        (((4, 0x20), (68, 0x120), (70, 0x120)), 100, 70),
    ):
        refill_light()
        aim = facing
        enter(5, sword=True)
        set_action(1, 0)
        pad(0, 0x120)
        g.run(g.hooks[DISPATCH], actor)
        assert state() == 70
        aim = (facing + 0x4000) & 0xffff
        for frame, buttons in presses:
            pad(frame, buttons, stick=strength)
        assert state() == target, (presses, strength, state())

    refill_light()
    enter(70, sword=True)
    assert pad(114) == 74
    assert pad(4, 0x20) == 74
    assert pad(30, 0x120, stick=100) == 153

    for backward in (False, True):
        refill_light()
        aim = facing
        enter(70, sword=True)
        assert tap(0x100, 76, stick=0) == 70
        assert pad(82) == 169
        assert pad(2) == 169
        if backward:
            aim = (facing + 0x8000) & 0xffff
            assert tap(0x100, 22) == 63
        else:
            assert tap(0x100, 22, stick=0) == 169
            assert pad(58) == 60

    for sword, opener, window, base in ((False, 0, 70, 106), (True, 5, 48, 152)):
        for yaw, expected in sectors:
            assert reference.command((17, 18, 19), yaw) == expected, (yaw, expected)
            aim = (facing - yaw) & 0xffff
            enter(opener, sword=sword)
            target = {17: base + 1, 18: base + 2, 19: base}.get(expected, opener)
            active = tap(0x120, window)
            assert active == target, (sword, yaw, target, active)
        # No slash gauge means the axe opener cannot charge the dodge attack.
        if not sword:
            aim = (facing + 0x8000) & 0xffff
            enter(opener, energy=0)
            assert tap(0x120, window) == opener

    for strength, expected in ((0, 2), (48, 2), (49, 2), (50, 3), (100, 3)):
        moving = strength >= 50
        assert reference.command((2, 7), 0, stick=moving, button=209) == (7 if moving else 2)
        aim = facing
        enter(1)
        assert tap(0x20, 60, stick=strength) == expected, (strength, state())

    for yaw, target in ((0x1fff, (1, 6)), (0x2000, (16, 0x8024)),
                        (-0x2000, (16, 0x8025)), (-0x8000, (16, 0x8025))):
        aim = (facing - yaw) & 0xffff
        for fz_stamina, gu_stamina, expected in ((74, 149, (16, 0x8000)), (75, 150, target)):
            assert reference.command((12,), yaw, stamina=fz_stamina) == (12 if fz_stamina == 75 else 29)
            enter(0)
            number(5088, gu_stamina)
            tap(0x80, 80)
            assert (g.half(actor + 9478), g.half(actor + 9482)) == expected, (yaw, gu_stamina)
    g.cpu.hook_del(g.traps.pop(0x2864C4))
    g.stub(0x2864C4, lambda _, skill, *__: int(skill == 156))
    for reserve, expected in ((74, (16, 0x8005)), (75, (16, 0x8024))):
        aim = (facing - 0x4000) & 0xffff
        enter(5, sword=True)
        number(5088, reserve)
        tap(0x80, 56)
        assert (g.half(actor + 9478), g.half(actor + 9482)) == expected, reserve
    g.cpu.hook_del(g.traps.pop(0x2864C4))
    g.stub(0x2864C4, lambda *_: 0)
    aim = facing

    def cycle(clip, button, entry, repeat, *, exhaust=False):
        bank = root / "dist/switch-axe-extreme/romfs/nativeNX/player/mot/frontier_sa"
        data = (bank / f"m_{clip // 100 - 10}.lmt").read_bytes()
        offset, = struct.unpack_from("<I", data, 8 + 4*(clip % 100))
        length, restart = struct.unpack_from("<Ii", data, offset + 8)
        assert 0 <= restart < length - 1
        number(1284, length - 1)
        number(1288, restart)
        before = len(hits)
        n = 0
        wraps = 0
        for tick in range(length * (16 if exhaust else 2)):
            n += 2
            wrap = n >= length - 1
            if wrap:
                n = restart + n - (length - 1)
                wraps += 1
            active = pad(n, button if tick % 5 == 0 else 0, wrap=wrap)
            if exhaust and active not in (entry, repeat):
                return active
            assert active in (entry, repeat) and g.half(actor + 1204) == clip, (entry, tick, active, n)
            if entry in (8, 155):
                reserve = stamina()
                recover_stamina()
                assert stamina() == reserve, (active, tick, reserve, stamina())
            if not exhaust and active == repeat and wraps >= 3 and len(hits) >= before + 7:
                break
        else:
            raise AssertionError((entry, wraps, hits[before:]))
        assert all(c == clip for _, c in hits[before:])
        # Stop tapping; the default exit must still win at the next open window.
        for _ in range(length*2):
            n += 2
            wrap = n >= length - 1
            if wrap:
                n = restart + n - (length - 1)
            if pad(n, wrap=wrap) not in (entry, repeat):
                return state()
        raise AssertionError("continuation remained active after releasing the attack button")

    try:
        form(False)
        number(5088, 1500)
        weapon(0x120, "003a", 2)
        for frame in range(2, 60, 2):
            pad(frame, 0x100 if frame == 42 else 0)
        assert state() == 8
        assert cycle(1405, 0x100, 8, 9) in (46, 47)
        enter(45)
        number(5088, 400)
        assert pad(28) == 45
        assert stamina() == 300
        recover_stamina()
        assert stamina() == 300
        for elapsed in (1, 2):
            number(28, elapsed)
            enter(8)
            number(5088, 250)
            assert cycle(1405, 0x100, 8, 9, exhaust=True) in (46, 47)
            assert stamina() == 0
            set_action(1, 0)
            for _ in range(3):
                recover_stamina()
            assert stamina() > 0
        number(28, 2)
        for sword in (False, True):
            if sword:
                refill_light()
            for walking in (False, True):
                for release in ("R", "X", "A"):
                    form(sword)
                    number(5088, 1500)
                    g.put(actor + 13100, 100)
                    number(1284, 220)
                    weapon(0x40, "000b" if sword else "001a", 162 if sword else 111)
                    previous = 0x40
                    g.put(actor + 4076, facing)
                    g.put(actor + 5280, facing)
                    aim = (facing + 0x3000) & 0xffff if walking else facing
                    gate = 40 if walking else 56
                    for n in range(10, gate, 2):
                        assert pad(n, 0x40, stick=100 if walking else 0) == (162 if sword else 111)
                    assert g.half(actor + 4076) == facing
                    holding = (163 if sword else 112) + int(walking)
                    assert pad(gate, 0x40, stick=100 if walking else 0) == holding
                    assert (g.half(actor + 4076), g.half(actor + 5280)) == (aim, aim), (sword, walking)
                    for tick in range(32):
                        n = (tick*2) % 52 if walking else 62 + tick*2
                        assert pad(n, 0x40, stick=100 if walking else 0) == holding
                    light = g.get(hud) & 0xffff
                    if release == "A":
                        for k in range(4):
                            next_state = pad(n + 2*k, 0x140, stick=100 if walking else 0)
                            if next_state != holding:
                                break
                        assert next_state == (166 if sword else 125), next_state
                    else:
                        expected = (157 if walking else 155) if sword else (122 if walking else 116)
                        for k in range(4):
                            next_state = pad(n + 2*k + 2, 0x60 if release == "X" else 0, stick=100 if walking else 0)
                            if next_state != holding:
                                break
                        assert next_state == expected, (release, expected, next_state)
                        if not walking:
                            assert g.get(actor + 13100) == (100 if sword else 94)
                            assert cycle(1065 if sword else 1490, 0x20, expected, expected+1) in (
                                (170, 171) if sword else (38, 39))
                            if sword:
                                assert g.get(actor + 13100) == 100 and (g.get(hud) & 0xffff) <= light - 7*6
                            else:
                                assert g.get(actor + 13100) <= 100 - 7*6 and (g.get(hud) & 0xffff) == light
        refill_light()
        # Slash evades accept held X from frame 18 and switch at frame 38.
        for sword, slide, hold, weak, charged, rush, follow in (
            (False, 106, 123, 141, 127, 130, 142), (False, 107, 124, 140, 133, 136, 138),
            (False, 108, 123, 141, 127, 130, 142), (True, 152, 128, 131, 134, 160, 29),
            (True, 153, 129, 132, 135, 161, 29), (True, 154, 128, 131, 134, 160, 29),
        ):
            if sword:
                refill_light()
            for at, stick, expected in ((38, 0, weak), (40, 0, charged), (40, 100, rush)):
                enter(slide, sword=sword)
                assert pad(18, 0x20) == slide
                assert pad(38, 0x20) == hold
                assert pad(at, 0, stick=stick) == expected, (slide, at, stick, state())

            # Compare both turns using GU's camera-relative aim and actual X mapping.
            for yaw, strength, follow_stick in (
                (-0x4000, 100, 100), (0x4000, 50, 50), (0x1fff, 100, 100),
                (-0x8000, 100, 100), (0x4000, 100, 49), (-0x4000, 100, 0),
            ):
                enter(slide, sword=sword)
                assert pad(18, 0x20) == slide
                assert pad(38, 0x20) == hold, (slide, state(), g.get(hud) & 0xffff)
                aim = (facing + yaw) & 0xffff
                assert pad(20, 0x20, stick=strength) == hold
                assert g.half(actor + 4076) == facing
                aim = (facing - yaw) & 0xffff
                assert pad(40, 0, stick=strength) == rush
                heading = reference.turn(slide, rush, yaw, facing)
                assert (g.half(actor + 4076), g.half(actor + 5280)) == (heading, heading)
                assert pad(12, stick=strength) == rush
                assert tap(0x20, 22, stick=strength) == rush  # X is buffered from frame 22.
                g.run(COMMAND_UPDATE, actor)
                assert pad(33, stick=strength) == rush
                assert g.half(actor + 4076) == heading
                aim = (heading + yaw) & 0xffff
                # FZ checks the live stick when the buffered X executes.
                relative = -yaw if follow_stick >= 50 else 0
                expected = reference.turn(hold, follow, relative, heading)
                assert pad(34, stick=follow_stick) == follow
                observed = g.half(actor + 4076), g.half(actor + 5280)
                assert observed == (expected, expected), (slide, follow_stick, observed, expected)

        camera_manager = g.get(0x2126504)
        original_camera = g.get(camera_manager + 84)
        camera = g.alloc(112)

        def camera_atan2(*_):
            dy, dx = (
                struct.unpack("<f", struct.pack("<I", g.cpu.reg_read(register)))[0]
                for register in (arm.UC_ARM_REG_S0, arm.UC_ARM_REG_S1)
            )
            result, = struct.unpack("<I", struct.pack("<f", math.atan2(dy, dx)))
            g.cpu.reg_write(arm.UC_ARM_REG_S0, result)

        g.stub(0x13EFD70, camera_atan2)
        try:
            for sword, rush, follow, old_yaw, new_yaw in (
                (False, 130, 142, 0, 0x4000),
                (True, 160, 29, 0x4000, 0),
            ):
                if sword:
                    refill_light()
                for keep_held in (True, False):
                    aim = (facing - old_yaw) & 0xffff
                    enter(rush, sword=sword)
                    # GU 2A79CC latches the camera azimuth while forward is held.
                    g.cpu.mem_write(actor + 11990, b"\x01")
                    g.put(actor + 12016, old_yaw)
                    assert pad(20, stick=100) == rush
                    assert tap(0x20, 22) == rush
                    # Rotate the live camera 90 degrees while the attack is queued.
                    g.cpu.mem_write(camera + 96, struct.pack("<3f", new_yaw != 0, 0, new_yaw == 0))
                    g.put(camera_manager + 84, camera)
                    if not keep_held:
                        g.cpu.mem_write(actor + 11990, b"\x00")
                        assert pad(32, stick=0) == rush
                    assert pad(34, stick=100) == follow
                    expected = reference.turn(0, follow, old_yaw - new_yaw, facing)
                    heading = g.half(actor + 4076), g.half(actor + 5280)
                    assert heading == (expected, expected), (sword, keep_held, heading, expected)
                    assert g.cpu.mem_read(actor + 11990, 1) == bytes([keep_held])
                    g.put(camera_manager + 84, original_camera)
                    g.cpu.mem_write(actor + 11990, b"\x00")
        finally:
            g.cpu.hook_del(g.traps.pop(0x13EFD70))
            g.put(camera_manager + 84, original_camera)
            g.cpu.mem_write(actor + 11990, b"\x00")

        for sword, attack, accept, execute, follow in (
            (False, 0, 58, 80, 40), (True, 72, 170, 208, 75),
            (False, 104, 70, 90, 114), (False, 126, 22, 34, 142),
            (False, 125, 22, 34, 142), (True, 166, 22, 34, 29),
            (True, 149, 70, 90, 17), (True, 150, 22, 34, 29),
        ):
            if sword:
                refill_light()
            for yaw in (0x4000, -0x4000):
                for follow_stick in (49, 50):
                    if attack == 72:
                        enter(70, sword=True)  # Reset the discharge tier before testing the release.
                        set_action(16, 0x8000 + attack)
                        assert pad(156) == attack
                    else:
                        enter(attack, sword=sword)
                    aim = (facing - yaw) & 0xffff
                    assert tap(0x20, accept) == attack
                    assert pad(execute - 1, stick=100) == attack
                    aim = (facing + yaw) & 0xffff
                    relative = yaw if follow_stick < 50 else -yaw
                    if attack == 72:
                        _, expected = reference.release(210, yaw=relative, facing=facing)
                    else:
                        expected = reference.turn(0, follow, relative, facing)
                    assert pad(execute, stick=follow_stick) == follow
                    assert (g.half(actor + 4076), g.half(actor + 5280)) == (expected, expected), (
                        attack, follow, yaw, follow_stick,
                    )

        g.cpu.hook_del(g.traps.pop(0x283238))
        for sword, slide, charge in (
            (False, 106, 123), (False, 107, 124), (False, 108, 123),
            (True, 152, 128), (True, 153, 129), (True, 154, 128),
        ):
            if sword:
                refill_light()
            guard = 91 if sword else 89
            aim = facing
            enter(slide, sword=sword)
            assert pad(36, 0x800) == slide
            assert pad(38, 0x800) == slide
            assert pad(40) == slide
            assert pad(42, 0x800) == guard

            enter(slide, sword=sword)
            assert pad(18, 0x20) == slide
            assert pad(36) == slide
            assert pad(38, 0x800) == (charge if sword else guard)
            enter(slide, sword=sword)
            assert pad(38, 0x820) == guard

            for yaw, strength in ((0, 0), (0x4000, 49), (0x4000, 50),
                                  (0x1fff, 100), (0x2000, 100), (0x6000, 100),
                                  (0x6001, 100), (-0x4000, 100)):
                enter(slide, sword=sword)
                aim = (facing - yaw) & 0xffff
                g.put(actor + 5280, (facing + 0x8000) & 0xffff)
                assert pad(38, 0x800, stick=strength) == guard
                expected = reference.turn(slide, guard, yaw if strength >= 50 else 0, facing)
                assert (g.half(actor + 4076), g.half(actor + 5280)) == (expected, expected), (
                    slide, yaw, strength, expected,
                )
                assert g.run(BLOCK, actor, (expected + 0x4000) & 0xffff) == 1
                assert g.run(BLOCK, actor, (expected + 0x4001) & 0xffff) == 0

            enter(slide, sword=sword)
            assert pad(82, 0x800) == guard
            enter(slide, sword=sword)
            # FZ 81815D60 leaves the completed slash before 81815F30 reads GP.
            pad(82, 0x800, wrap=True)
            assert (g.half(actor + 9478), g.half(actor + 9482)) == (1, 0)

        for sword, sprint in ((False, 103), (True, 148)):
            refill_light()
            enter(sprint, sword=sword)
            pad(70, stick=100)
            pad(120, stick=100)
            pad(90, stick=100, wrap=True)
            pad(92, stick=100)  # publish the wrapped tick's gauge change
            assert state() == sprint
            assert g.get(actor + 13100) == (100 if sword else 90)
            light = g.get(hud) & 0xffff
            assert light == (290 if sword else 300), (sprint, light)

        refill_light()
        form(True)
        # A stock empty slash gauge cannot knock a charged light sword out of idle.
        g.put(actor + 13100, 0)
        set_action(1, 0)
        pad(0)
        g.run(g.hooks[DISPATCH], actor)
        assert (g.half(actor + 9478), g.half(actor + 9482)) == (1, 0)
        g.put(actor + 13100, 80)
        # Spend all 300 light on the ten-point opening slash. No hit is credited.
        for _ in range(30):
            set_action(16, 0x8000 + 5)
            pad(0)
            assert pad(20) == 5
        assert pad(22) == 5 and g.get(hud) & 0xffff == 0
        set_action(1, 0)
        pad(0, 0x20)
        g.run(g.hooks[DISPATCH], actor)
        assert state() == 31
        assert g.get(actor + 13100) == 80
        number(1284, 180)
        pad(180)
        assert g.get(actor + 13096) == 0

        refill_light()
        enter(155, sword=True)
        number(5088, 200)
        assert cycle(1065, 0x20, 155, 156, exhaust=True) in (170, 171)
        pad(0)
        assert g.get(hud) & 0xffff == 0 and stamina() == 200
    finally:
        g.cpu.hook_del(g.traps.pop(submit))
        g.put(0x2126A64, old_manager)
        number(1284, 180)
        number(1288, 0)
        g.cpu.mem_write(actor + 5136, bytes(2))
        g.cpu.mem_write(actor + 1206, b"\0")
    print("GU mapped directions, X deadzone, B evade stamina, buffered slash-evade and sprint follow-up turns, and empty-light morph passed.")
    print("GU buffered Switch Axe turns use the live camera with either continuous stick input or a neutral/re-push.")
    print("GU mapped A/X continuations: seven or more strikes, release exits, standing/moving charge and charged uppercuts passed.")
    print("GU passive stamina recovery: suppressed during axe wild swings and light flurry, resumes in idle; both loops end on depletion.")
