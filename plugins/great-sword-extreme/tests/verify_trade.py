"""Exercise the packaged trade hooks, native economy, and character save recovery."""

import json
import struct
from unicorn import UC_HOOK_CODE
from unicorn.arm_const import UC_ARM_REG_R0, UC_ARM_REG_R2, UC_ARM_REG_R3, UC_ARM_REG_SP

from verify_menu import register_weapon
from verify_style import MANAGER, REPO, SAVE, api_for, boot, save_character, select_character

SHOP, SMITH = 0x6E8558, 0x6F1AE8
PANEL_VTABLE, PAGE_VTABLE, PANEL_UPDATE = 0x176F384, 0x176F45C, 0x4C
A, B, UP, DOWN, LEFT, RIGHT = 0x2000, 0x4000, 0x10, 0x40, 0x80, 0x20


class Facility:
    def __init__(self, g, smith, stock_total=14):
        self.g = g
        self.update = SMITH if smith else SHOP
        self.smith = smith
        if 0x13EFD7C not in g.traps:
            g.stub(0x13EFD7C, lambda dst, size, *_: g.cpu.mem_write(dst, bytes(size)))
        if 0x13EFF20 not in g.traps:
            g.stub(0x13EFF20, lambda dst, size, value, *_: g.cpu.mem_write(dst, bytes([value & 255]) * size))
        self.owner = g.alloc(13200)
        self.panel = g.alloc(96)
        self.page = g.alloc(84)
        self.page_numbers = {}
        self.money_panel = g.alloc(96)
        self.cursor = self.owner + (8036 if smith else 92)
        self.stock = 7
        self.rows, self.visible, self.highlights = {}, {}, {}
        self.native = []
        self.icons = {}
        self.funds = []
        self.sounds = []
        phase = 2 if smith else 3
        g.put(self.owner + 4, phase)
        g.put(self.owner + 52, g.alloc(128))
        for index in (2,10,11):
            g.put(g.get(self.owner+52)+4*index,g.alloc(144))
        self.header = self.owner + (11600 if smith else 1808)
        g.put(g.get(self.owner + 52), self.panel)
        g.put(g.get(self.owner + 52) + 12, self.money_panel)
        g.put(g.get(self.owner + 52) + 4, self.page)
        g.put(self.page, PAGE_VTABLE)
        g.put(self.page + 76, self.cursor)
        g.put(self.panel, PANEL_VTABLE)
        g.put(self.panel + 76, self.cursor)
        desc = self.header - 60
        g.put(self.panel + 84, desc)
        for row in range(self.stock):
            native_label = g.alloc(16)
            g.cpu.mem_write(native_label, f"Native {row}\0".encode())
            g.put(desc + 4 + 8 * row, 1)
            g.put(desc + 8 + 8 * row, native_label)
        for offset in ((8036, 8072, 8108, 8144, 8180, 8216) if smith else (92, 128, 164, 200, 236)):
            g.run(0x54ED74, self.owner + offset)
        if smith:
            g.run(0x553030, self.owner + 8360)
        g.run(0x54EDEC, self.cursor, self.stock, 1, 0)
        g.run(0x54EE34, self.cursor, stock_total, 3)
        def observed_parent(cpu, *_):
            if cpu.reg_read(UC_ARM_REG_R0) == self.owner:
                self.native.append(self.owner)
        g.cpu.hook_add(UC_HOOK_CODE, observed_parent, begin=self.update+4, end=self.update+4)
        frame = g.alloc(108)
        g.put(0x2126764, frame)
        g.put(frame + 104, 0x41000000)

    def connect(self):
        g = self.g
        for addr in (0x65CB3C, 0x65D9E8, 0x65F830, 0x4EEB78, 0x4EEC94, 0x56E530, 0x56E6FC,
                     0x56E528, 0x56E538, 0x56E808, 0x56E6A8, 0x5D0D44, 0x56E5A8, 0x56FDE0, 0x65E794, 0x162E0C, 0x13C208, 0x55C4D4, 0x55BC7C, 0x55BC4C):
            if addr in g.traps:
                g.cpu.hook_del(g.traps.pop(addr))
        label = g.alloc(16)
        g.cpu.mem_write(label, b"Native category\0")
        g.stub(0x55BC7C, lambda *_: label)
        g.stub(0x55BC4C, lambda *_: label)
        g.stub(0x65CB3C, lambda owner, key, *_: g.get(g.get(owner + 52) + 4 * key))
        for addr in (0x65D9E8, 0x65F830, 0x56E530, 0x56E6FC,0x65E5BC,0x65CBA0,0x65CBC4,0x65E794,0x162E0C,0x13C208,0x55C4D4):
            g.stub(addr, lambda *_: 0)
        g.stub(0x4EEB78, lambda _, sound, *rest: self.sounds.append((0, sound)))
        g.stub(0x4EEC94, lambda _, sound, *rest: self.sounds.append((1, sound)))
        g.stub(0x56E528, lambda panel, row, value, *_: self.visible.update({row: value}))
        g.stub(0x56E538, lambda panel, row, value, *_: self.highlights.update({row: value}))
        g.stub(0x56E808, lambda panel, row, part, text: self.rows.update({row: g.string(text)}))
        g.stub(0x56E6A8, lambda panel, row, part, _: self.icons.update({(row, part): g.get(g.cpu.reg_read(UC_ARM_REG_SP))}))
        g.stub(0x5D0D44, lambda panel, value, *_: self.funds.append((panel, value)))

    def answer_equip(self, equip):
        if equip:
            self.press(LEFT)
        self.press(A)
        assert self.g.cpu.mem_read(self.owner + 5, 1)[0] == 0

    def page_counter(self):
        g = self.g
        for addr in (0x56E5A8, 0x56FDE0):
            if addr in g.traps:
                g.cpu.hook_del(g.traps.pop(addr))
        g.stub(0x56E5A8, lambda *_: 0)
        g.stub(0x56FDE0, lambda _, group, part, value: self.page_numbers.update({part: value}))
        g.run(g.get(g.get(self.page) + PANEL_UPDATE), self.page)
        assert g.get(self.page + 76) == self.cursor
        return self.page_numbers[1], self.page_numbers[2]

    def press(self, key=0, *, repeat=False):
        g = self.g
        pad = g.get(0x188C670)
        g.put(pad + 27644, 0 if repeat else key)
        g.put(pad + 27700, key if repeat else 0)
        if self.smith and key and g.get(self.owner + 168):
            self.press()
            g.put(pad + 27644, 0 if repeat else key)
            g.put(pad + 27700, key if repeat else 0)
        g.run(self.update, self.owner)
        if self.smith and g.cpu.mem_read(self.owner + 4, 2) == b"\x03\x09":
            self.press()
        self.rows.clear()
        for row in range(8):
            record = self.header + 16 + 24*row
            text = g.get(record+16)
            if text:
                self.rows[row] = g.string(text)


    def render_category(self):
        self.rows.clear()
        update = self.g.get(self.g.get(self.panel) + PANEL_UPDATE)
        self.g.run(update, self.panel)
        return dict(self.rows)

    def open(self, name="Custom weapon", icon=13):
        g = self.g
        # Native pages retain all seven stock rows, then input enters the appended page.
        g.run(0x557D90, self.cursor, 0)
        self.press(RIGHT)
        assert g.run(0x557D78, self.cursor) == 7
        for _ in range(6):
            self.press(DOWN)
        assert g.run(0x557D78, self.cursor) == 13
        assert self.render_category() and len(self.rows) == 7
        assert self.highlights[6] == 4
        self.press(DOWN)
        assert self.render_category()[0] == name and self.highlights[0] == 4
        assert self.icons[0, 11] == icon
        assert g.run(0x557D78, self.cursor) == 13
        self.press(UP)
        assert self.render_category()[6] != name and self.highlights[6] == 4
        self.press(RIGHT)
        assert self.render_category()[0] == name
        self.press(A)


def setup_items(g):
    item_box, pouch = g.alloc(30000), g.alloc(256)
    table, entries, record = g.alloc(32), g.alloc(112), g.alloc(56 * 300)
    g.put(0x1858438, item_box)
    g.put(0x18584B8, pouch)
    g.put(0x18583A8, table)
    g.put(table + 28, entries)
    g.put(entries + 100, 300)
    g.put(entries + 108, record)
    for item in (100, 101):
        g.cpu.mem_write(record + 56 * item + 14, b"\x63")
    g.put(pouch + 26, (2 << 16) | 100)
    g.put(pouch + 30, (1 << 16) | 101)
    g.put(item_box + 26, (3 << 16) | 100)
    g.put(item_box + 30, (1 << 16) | 101)
    locked = set()
    g.stub(0x1943A4, lambda _, item, *rest: item in locked)
    for addr in (0x1943D4, 0x3F8634, 0x3F8678):
        g.stub(addr, lambda *_: 0)
    return pouch, item_box, locked


def verify():
    g = boot((REPO / "mhgu/main.elf").read_bytes(), None, order=("registry",))
    grant, _, instance = register_weapon(g)[1:]
    result = g.alloc(8)
    assert not g.run(instance, 1, result)
    g.put(0x188C670, g.alloc(27704))
    wallet = g.get(0x1897F78)
    g.put(wallet + 36, 1000)
    pouch, item_box, locked = setup_items(g)
    g.stub(0xB84114,lambda *_:0)
    native_calls = []
    for address in (0x6EA28C, 0x6F31D8, 0x6FBCFC, 0x143160):
        g.cpu.hook_add(UC_HOOK_CODE, lambda _cpu, pc, *_: native_calls.append(pc),
                       begin=address, end=address)

    retail_grants = []
    g.cpu.hook_add(UC_HOOK_CODE, lambda cpu, *_: retail_grants.append(
        (cpu.reg_read(UC_ARM_REG_R2), cpu.reg_read(UC_ARM_REG_R3))),
        begin=0x1421B4, end=0x1421B4)

    api = api_for(g, b"mhgu::WeaponTradeV2", b"WTR2")
    purchase, forge, upgrade = (g.get(api + i * 4) for i in range(3))
    item = lambda ident, qty: (qty << 16) | ident
    assert g.run(purchase, 17, 200) == 1
    assert g.run(purchase, 16, 10_000_000) == 1
    assert g.run(purchase, 16, 200) == 0
    assert g.run(purchase, 16, 100) == 2
    assert g.run(forge, 16, 300, item(100, 2), item(100, 1), 0, 0) == 1
    assert g.run(forge, 16, 300, item(100, 3), item(101, 2), 0, 0) == 0
    assert g.run(upgrade, 16, 3, 400, item(100, 1), 0, 0, 0) == 1
    assert g.run(upgrade, 16, 0, 400, item(100, 1), 0, 0, 0) == 0

    shop = Facility(g, False)
    shop.connect()
    # GU may paint the panel before it runs the facility input callback.
    g.run(0x557D90, shop.cursor, 13)
    assert shop.render_category()[6].startswith("Native ") and not shop.native
    shop.open()
    assert g.get(shop.header+24)==200 and 1 not in shop.rows
    shop.press(A)
    assert g.get(wallet+36)==1000
    shop.press(B)
    shop.press(B)  # Native Yes/No first moves to No, then dismisses.
    assert g.get(wallet + 36) == 1000 and not g.run(instance, 1, result)
    shop.press(A)
    shop.press(A)
    assert g.run(instance, 1, result) and g.half(result + 6) == 0
    assert g.get(wallet + 36) == 800 and shop.funds[-1] == (shop.money_panel, 800)
    assert (0, 10) in shop.sounds
    shop.answer_equip(False)
    g.put(wallet + 36, 100)
    shop.press(A)
    shop.press(A)
    assert not g.run(instance, 2, result)
    assert g.get(wallet + 36) == 100
    shop.press(B)
    assert shop.render_category()[0] == "Custom weapon" and shop.highlights[0] == 4
    assert shop.page_counter() == (3, 3)
    shop.press(UP)
    assert shop.render_category()[6].startswith("Native ") and shop.highlights[6] == 4
    assert shop.page_counter() == (2, 3)
    assert shop.native
    # No retail equipment catalogue is loaded in this fixture.
    opened = []
    g.stub(0x6E94B4, lambda owner, *_: opened.append(owner))
    shop.press(A)
    assert opened == [shop.owner] and shop.native[-1] == shop.owner
    g.cpu.hook_del(g.traps.pop(0x6E94B4))
    g.put(wallet + 36, 800)

    smith = Facility(g, True)
    smith.connect()
    smith.press()
    assert smith.page_counter() == (1, 3)
    smith.press(LEFT)
    assert smith.page_counter() == (3, 3)
    assert smith.render_category() == {0: "Custom weapon"}
    smith.press(LEFT)
    assert smith.page_counter() == (2, 3)
    assert smith.render_category()[0].startswith("Native ")
    smith.press(LEFT)
    assert smith.page_counter() == (1, 3)
    smith.open()
    assert smith.rows == {0: "Custom weapon"}
    smith.press(A)
    assert g.half(smith.owner + 11820) == 3 and g.half(smith.owner + 11822) == 5, (
        g.cpu.mem_read(smith.owner + 4, 2), g.get(smith.owner + 168), g.half(smith.owner + 11820),
        g.half(smith.owner + 11822), smith.rows,
    )
    locked.add(101)
    smith.press(A)
    assert g.get(wallet+36)==800
    assert g.get(pouch + 26) == item(100, 2) and g.get(item_box + 26) == item(100, 3)
    assert g.get(pouch + 30) == item(101, 1) and g.get(item_box + 30) == item(101, 1)
    assert g.get(wallet + 36) == 800 and not g.run(instance, 2, result)
    assert (1, 117) not in smith.sounds
    locked.clear()
    smith.press(A)
    smith.press(A)
    assert g.get(wallet + 36) == 500 and g.run(instance, 2, result)
    assert g.get(pouch + 26) == 0 and g.get(item_box + 26) == item(100, 2)
    assert g.get(pouch + 30) == 0 and g.get(item_box + 30) == 0
    assert (1, 117) in smith.sounds
    smith.answer_equip(False)
    smith.press(A)
    smith.press(A)
    assert g.get(wallet+36)==500
    smith.press(B)
    smith.press(B)
    smith.render_category()
    g.run(0x557D90, smith.cursor, 3)
    smith.press(A)
    smith.press(A)
    smith.press(A)
    smith.press(A)
    smith.press()
    assert g.get(wallet + 36) == 100
    assert g.get(item_box + 26) == item(100, 1)
    assert g.run(instance, 1, result) and g.half(result + 6) == 1
    assert g.run(instance, 2, result) and g.half(result + 6) == 0

    # A full SDK box still permits upgrades, but crafting and buying cannot spend.
    for ident in range(3, 129):
        assert g.run(grant, 16) == ident
    smith.press(B)
    shop = Facility(g, False)
    shop.connect()
    shop.open()
    shop.press(A)
    shop.press(A)
    assert not g.run(instance,129,result) and g.get(wallet+36)==100
    smith = Facility(g, True)
    smith.connect()
    smith.open()
    assert smith.rows == {0: "Custom weapon"}
    smith.press(B)
    smith.press(B)
    smith.render_category()
    g.run(0x557D90, smith.cursor, 3)
    smith.press(A)
    assert g.half(smith.owner + 8388) == 128 and g.half(smith.owner + 8386) == 2

    shop = Facility(g, False)
    shop.connect()
    register_weapon(g, ident=17, style=44, weapon_name="Second type", icon=27)
    assert g.run(purchase, 17, 321) == 0
    g.run(0x557D90, shop.cursor, 13)
    shop.press(DOWN)
    shop.press(DOWN)
    assert shop.render_category()[1] == "Second type" and shop.icons[1, 11] == 27
    shop.press(A)
    assert g.get(shop.header + 24) == 321 and 1 not in shop.rows
    shop.press(B)
    shop.press(UP)
    assert shop.render_category()[0] == "Custom weapon" and shop.icons[0, 11] == 13
    for ident in range(18, 24):
        register_weapon(g, ident=ident, style=ident + 40, weapon_name=f"Type {ident}")
        assert g.run(purchase, ident, 321) == 0
    assert shop.page_counter() == (3, 4)
    assert shop.render_category()[6] == "Type 22"
    shop.press(RIGHT)
    assert shop.page_counter() == (4, 4)
    assert shop.render_category() == {0: "Type 23"}
    shop.press(LEFT)
    assert shop.page_counter() == (3, 4)
    shop.press(LEFT)
    assert shop.page_counter() == (2, 4)

    partial = Facility(g, False, stock_total=13)
    partial.connect()
    g.run(0x557D90, partial.cursor, 12)
    assert partial.render_category()[6] == "Custom weapon"
    partial.press(DOWN)
    assert partial.render_category()[6] == "Custom weapon" and partial.highlights[6] == 4
    partial.press(DOWN)
    assert partial.render_category()[0] == "Second type"

    assert {0x6EA28C, 0x6F31D8, 0x6FBCFC} <= set(native_calls)
    assert 0x143160 not in native_calls
    assert retail_grants and set(retail_grants) == {(0, 0)}
    verify_save(g, instance, result)
    print("GU purchase and forging controllers, native debit, SDK rewards and single-file save passed")


def verify_save(g, instance, result, *, expected_upgrade=1, last_instance=128):
    before = g.files.copy()
    save_character(g, success=False)
    assert g.files == before
    g.mount_available = False
    save_character(g)
    assert g.files == before
    g.mount_available = True
    save_character(g)
    assert set(g.files) == set(before) | {SAVE}
    data = json.loads(g.files[SAVE])["data"]["inventory"]
    assert data["instances"]["1"]["upgrade"] == expected_upgrade
    select_character(g, 1)
    assert not g.run(instance, 1, result)
    select_character(g, 0)
    assert g.run(instance, 1, result) and g.half(result + 6) == expected_upgrade
    assert g.run(instance, last_instance, result)


if __name__ == "__main__":
    verify()
