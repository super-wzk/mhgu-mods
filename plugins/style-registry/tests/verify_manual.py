"""Exercise both manual readers with the registered NROs and GU menu objects."""
from verify_style import MANAGER, boot, style_descriptor, service


def expand(text):
    for key in ("A", "B", "X", "Y", "R", "摇杆"):
        icon = "ANALOG" if key == "摇杆" else "PAD_" + key
        text = text.replace("{" + key + "}", f"<SIZE 28><COL E6E6E6FF><ICON {icon}></COL></SIZE>")
    assert "{" not in text and "斩味" not in text
    # The simplified Chinese GU guide joins button icons with ASCII + and /.
    assert not any(char in text for char in "＋／"), text
    return text


def replay(repo, elf, cases):
    g = boot(elf, b"[Weapons]\n", ("registry",))
    close_native_manual = 0x65F7A4
    # Forge 1.5.1 copies both entry instructions without relocating the PC-relative LDR.
    assert g.originals[close_native_manual] == 0
    manager, container, dialogs, native_dialog, dialog_vtable = (
        g.alloc(size) for size in (332, 56, 56, 4, 8)
    )
    g.put(0x1899458, manager)
    assert g.run(close_native_manual) == 0
    g.put(manager + 328, container)
    g.put(container + 52, dialogs)
    g.cpu.hook_del(g.traps.pop(0x65CB3C))
    assert g.run(close_native_manual) == 0

    slot = dialogs + 13 * 4
    native_close = []

    def close_dialog(owner, *_):
        assert owner == native_dialog
        native_close.append("close")

    def release_dialog(owner, *_):
        assert owner == native_dialog
        native_close.append("remove")

    release = g.alloc(4)
    g.put(native_dialog, dialog_vtable)
    g.put(dialog_vtable + 4, release)
    g.stub(release, release_dialog)
    g.stub(0x56E04C, close_dialog)
    g.put(slot, native_dialog)
    assert g.run(close_native_manual) == dialogs and g.get(slot) == 0
    assert native_close == ["close", "remove"]
    list_panel, selection, cursor, vt, panel, arrows = (
        g.alloc(n) for n in (100, 3, 100, 80, 100, 140))
    g.put(list_panel + 92, selection)
    g.put(list_panel + 76, cursor)
    g.put(cursor, vt)
    g.put(panel + 84, arrows)
    g.cpu.mem_write(list_panel + 88, b"\2")
    g.cpu.mem_write(panel + 83, b"\xff")
    indices = [0, 7, 1, 11, 2, 12, 3, 9, 8, 14, 13, 6, 4, 10]
    strings, counts = [], []

    def capture(_panel, widget, field, text):
        strings.append((widget, field, g.string(text)))
        return 0

    def stub(addr, fn):
        if addr in g.traps:
            g.cpu.hook_del(g.traps.pop(addr))
        g.stub(addr, fn)

    for addr in (0x54533C, 0x56E538, 0x56E6A8, 0x65EB68, 0x54EDEC, 0x600DE4,
                 0x56E528, 0x56E530, 0x550354, 0x56E6FC, 0x65D424, 0x65D464):
        stub(addr, lambda *_: 0)
    empty = g.alloc(4)
    stub(0x55BC4C, lambda *_: empty)
    stub(0x56E808, capture)
    stub(0x54EE34, lambda _, size, *__: counts.append(size) or 0)
    page_fn = g.alloc(4)
    g.put(vt + 36, page_fn)
    stub(page_fn, lambda *_: page)
    selected_row = g.alloc(4)
    g.put(vt + 56, selected_row)
    stub(selected_row, lambda *_: 6)
    def count(index):
        g.cpu.mem_write(selection + 1, bytes([index]))
        g.run(g.hooks[0x5B1294], list_panel)
        return g.cpu.mem_read(list_panel + 80, 1)[0]

    assert count(0) == 6 and count(14) == 2
    text = g.alloc(16)
    register, _ = service(g)
    assert g.run(register, style_descriptor(g, 42, 0), text, text) == 0 and count(0) == 6
    all_pages = []
    for i, (name, weapon, *_) in enumerate(sorted(cases, key=lambda case: indices.index(case[1]))):
        root = repo / "plugins" / (name + "-extreme")
        nro = root / "dist" / root.name / "romfs/nativeNX/plugins" / (root.name.replace("-", "_") + ".nro")
        ex = g.load(nro, 0x9200000 + 0x100000 * i)
        for callback in ("forge_onLoad", "forge_onInit"):
            g.run(ex[callback], MANAGER)
        idx = indices.index(weapon)
        assert count(idx) == 7
        page = 0
        g.run(g.hooks[0x5B0F70], list_panel)
        assert g.cpu.mem_read(selection + 2, 1) == b"\x06"
        assert strings[-1][0:2] == (15, 1)
        title = strings[-1][2]
        assert "极之型" in title
        g.cpu.mem_write(panel + 81, bytes([idx, 6]))
        pages = (root / "src/manual.txt").read_text().strip().split("\n\n")
        g.run(g.hooks[0x646A3C], panel, cursor)
        assert counts[-1] == len(pages)
        for page, expected in enumerate(pages):
            g.run(g.hooks[0x646AD0], panel)
            expected = expand(expected)
            body = expected.replace("\t", "　").replace("\n", "\r\n")
            if "\t" in expected:
                heading, *rows = expected.splitlines()
                assert 1 <= len(rows) <= 9
                assert strings[-19] == (3, 2, heading)
                widgets = g.cpu.mem_read(0x163B0D4, 9)
                for j, widget in enumerate(widgets):
                    keys, move = rows[j].split("\t") if j < len(rows) else ("", "")
                    assert strings[-18 + 2*j] == (widget, 2, keys)
                    assert strings[-17 + 2*j] == (widget, 3, move)
            else:
                assert strings[-2:] == [(3, 2, title), (17, 1, body)]
            all_pages.append((title, body))

    pad, dialog = g.alloc(28000), g.alloc(128)
    g.put(0x188C670, pad)
    hint = g.alloc(184)
    hints = []
    stub(0x65E580, lambda *_: hint)
    stub(0x642C44, lambda obj, *_: hints.append(("save", obj)) or 0)
    stub(0x642A78, lambda *_: 0)
    stub(0x642B6C, lambda obj, text, *_: hints.append((g.string(text), obj)) or 0)
    stub(0x642D34, lambda obj, *_: hints.append(("restore", obj)) or 0)
    stub(0x65EFD0, lambda *_: dialog)
    stub(0x65F190, lambda _, text, *__: capture(dialog, 7, 1, text) or 1)
    closed = []
    stub(0x64C600, lambda owner, *_: closed.append(owner) or 0)
    def press(mask):
        g.put(pad + 27644, mask)
        g.run(g.hooks[0x5CB780], panel)

    g.put(panel + 76, 0)
    for i, (title, body) in enumerate(all_pages):
        press(0x2000 if i == 0 else 0x20)
        assert strings[-1][2].startswith(title) and strings[-1][2].endswith(body)
    press(0x20)
    assert strings[-1][2].endswith(all_pages[0][1])
    press(0x80)
    assert strings[-1][2].endswith(all_pages[-1][1])
    press(0x40)
    assert strings[-1][2].endswith(all_pages[0][1])
    press(0x10)
    assert strings[-1][2].startswith(all_pages[-1][0])
    press(0x4000)
    assert closed == [dialog]
    g.put(slot, native_dialog)
    assert g.run(close_native_manual) == dialogs and g.get(slot) == 0
    assert native_close == ["close", "remove", "close", "remove"]
    assert hints == [("save", hint), ("<ICON PAD_A> 武器操作指南", hint), ("restore", hint)]
    print(f"Both manual readers displayed {len(all_pages)} pages across six styles; optional registration and navigation passed.")
