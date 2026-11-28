//! GU 1.4 Change Style and Hunter Arts for registered styles and weapons.
//! The list has eight row widgets (6376BC) and six native style records.
use super::{Entry, active, choose_style, entries, native_menu};
use crate::weapon::{gu, inventory, local_hunter, read, set};
use core::ffi::CStr;

static GUIDE_PAGE: mhgu_sdk::sync::Shared<Option<usize>> = mhgu_sdk::sync::Shared::new(None);
const STOCK: usize = 6;
const ROWS: [u32; 8] = [7, 8, 9, 10, 0x46, 0x47, 0x4e, 0x4f];
const ACCEPT: u32 = 0x2000;
const BACK: u32 = 0x4000;

// All methods borrow the controller only for the duration of its GU callback.
struct BoxMenu(u32);
impl BoxMenu {
    fn panel(&self) -> u32 {
        unsafe { read(self.0, 4) }
    }
    fn weapon(&self) -> u32 {
        unsafe { read(self.panel(), 100) }
    }
    fn entries(&self) -> impl Iterator<Item = &'static Entry> {
        entries(self.weapon()).filter(|entry| entry.kind.is_none())
    }
    fn selected(&self) -> u32 {
        active(self.weapon()).map_or(0, |entry| entry.style.id)
    }
    fn phase(&self) -> u32 {
        unsafe { read(self.0, 128) }
    }
    fn cursor(&self) -> u32 {
        self.0 + 12
    }
    fn row(&self) -> usize {
        unsafe { gu!(0x557d78, self.cursor()) as usize }
    }
    fn page(&self) -> usize {
        unsafe { read::<u16>(self.cursor(), 16) as usize }
    }
    fn custom(&self) -> Option<&'static Entry> {
        self.entries().nth(self.row().checked_sub(STOCK)?)
    }
    fn equipped(&self) -> Option<&'static Entry> {
        active(self.weapon())
    }
    fn configure(&self) {
        if self.panel() == 0 || self.phase() != 1 {
            return;
        }
        if let Some((_, entry)) = inventory::selected() {
            self.draw_registered_style(entry);
            return;
        }
        let count = self.entries().count();
        let total = (STOCK + count).min(u16::MAX as usize) as u32;
        let active_id = self.selected();
        let initial = self
            .entries()
            .position(|e| e.style.id == active_id)
            .map_or_else(|| self.row().min(STOCK - 1), |i| STOCK + i);
        unsafe {
            gu!(0x54edec, self.cursor(), total.min(8), 1, 0);
            gu!(0x54ee34, self.cursor(), total, 3);
            gu!(0x557d90, self.cursor(), (initial as u32).min(total - 1));
        }
        self.draw_page();
        self.details();
    }
    fn draw_page(&self) {
        let p = self.panel();
        let id = self.selected();
        let first = self.page() * ROWS.len();
        unsafe {
            let list = read::<u32>(p, 88);
            if list == 0 {
                return;
            }
            let visible = usize::from(read::<u16>(self.cursor(), 20))
                .saturating_sub(first)
                .min(8);
            gu!(0x5c5500, list, visible as u32);
            // Restore localized stock labels after paging back.
            if first == 0 {
                gu!(0x637958, p);
            }
            let native = gu!(0x561270, read::<u32>(p, 104)) as usize;
            let mut extra = self.entries().skip(first.saturating_sub(STOCK));
            for (slot, widget) in ROWS.iter().copied().take(visible).enumerate() {
                let index = first + slot;
                let equipped = if index < STOCK {
                    id == 0 && index == native
                } else if let Some(e) = extra.next() {
                    gu!(0x5c55b0, list, slot as u32, e.name.as_ptr() as u32);
                    e.style.id == id
                } else {
                    false
                };
                gu!(0x56e530, p, widget, 1, equipped as u32);
            }
        }
    }
    fn draw_registered_style(&self, entry: &'static Entry) {
        unsafe {
            let panel = self.panel();
            let list = read::<u32>(panel, 88);
            if list == 0 {
                return;
            }
            gu!(0x54edec, self.cursor(), 1, 1, 0);
            gu!(0x54ee34, self.cursor(), 1, 3);
            gu!(0x557d90, self.cursor(), 0);
            gu!(0x637958, panel);
            gu!(0x5c5500, list, 1);
            gu!(0x5c55b0, list, 0, entry.name.as_ptr() as u32);
            gu!(0x56e530, panel, ROWS[0], 1, 1);
        }
        if crate::weapon::style::manual::has_book(entry) {
            let details = alloc::ffi::CString::new(alloc::format!(
                "<ICON PAD_X> 操作指南\r\n{}",
                entry.details.to_string_lossy()
            ))
            .unwrap();
            self.show_details(&entry.name, &details);
        } else {
            self.show_details(&entry.name, &entry.details);
        }
    }
    fn show_details(&self, name: &CStr, details: &CStr) {
        unsafe {
            let info = read::<u32>(self.0, 8);
            if info != 0 {
                gu!(0x6394b8, info, 0, 0, 0);
                gu!(0x56ea08, info, 2, 2, name.as_ptr() as u32);
                gu!(0x56ea08, info, 3, 2, details.as_ptr() as u32);
            }
        }
    }
    fn details(&self) {
        if let Some(e) = self.custom() {
            self.show_details(&e.name, &e.details);
        } else if self.row() < STOCK {
            unsafe {
                gu!(0x548e24, self.0);
            }
        }
    }
    fn navigate(&self, pressed: u32, repeat: u32) {
        let before = self.row();
        let page = self.page();
        unsafe {
            let help = self.0 + 84;
            let old_help = read::<u16>(help, 16);
            gu!(0x54ee98, self.cursor(), pressed | repeat);
            if read::<u8>(self.0, 121) != 0 {
                gu!(0x54ee98, help, pressed);
            }
            if self.page() != page {
                self.draw_page();
            }
            if self.row() != before || read::<u16>(help, 16) != old_help {
                self.details();
            }
        }
    }
    fn confirm(&self, e: &Entry, buttons: u32) {
        unsafe {
            match gu!(0x54fd94, self.0 + 120, buttons, 128, 32, 0) {
                1 => {
                    choose_style(self.weapon(), Some(e));
                    // Continue to the native Arts screen. It must never see a custom row as an art slot.
                    gu!(0x557d90, self.cursor(), 0);
                    gu!(0x549e24, self.0);
                    native_menu(true, || gu!(0x548f94, self.0, 5));
                    self.equipped_label();
                    gu!(0x54f260, 1, 201);
                }
                2 => {
                    set(self.0, 128, 1u32);
                    gu!(0x54ecc0, self.cursor(), 0);
                    gu!(0x54ecc0, self.0 + 84, 0);
                    self.details();
                    gu!(0x54f260, 0, 2);
                }
                _ => {}
            }
        }
    }
    fn equipped_label(&self) {
        if let Some(e) = inventory::selected()
            .map(|(_, entry)| entry)
            .or_else(|| self.equipped())
        {
            unsafe {
                gu!(0x56ea08, self.panel(), 0x55, 1, e.name.as_ptr() as u32);
            }
        }
    }
}

// 286318 clears incompatible Arts through the native slot setters.
pub(in crate::weapon) fn validate_arts(actor: u32, equipment: u32) {
    let Some((_, entry)) = inventory::selected().filter(|_| actor != 0 && equipment != 0) else {
        return;
    };
    unsafe {
        native_menu(true, || gu!(0x286318, actor));
        let mut equipped = 0;
        for slot in 0..3 {
            if gu!(0xe79c8, equipment + 4, slot) != 0 {
                equipped += 1;
                if equipped > entry.style.art_slots {
                    gu!(0xe79b4, equipment + 4, slot, 0);
                    gu!(0xe7a20, equipment + 4, slot, 0);
                }
            }
        }
    }
}

#[forge::hook(offset = 0x548b7c)]
unsafe fn open(ui: u32, panel: u32, help: u32, mode: u32) -> u32 {
    if panel != 0 && help != 0 {
        unsafe {
            let actor = local_hunter();
            validate_arts(actor, inventory::actor_equipment(actor));
        }
    }
    if GUIDE_PAGE.with(Option::take).is_some() {
        crate::weapon::Information::close();
    }
    let out = native_menu(true, || original!(ui, panel, help, mode));
    if panel != 0 && help != 0 {
        BoxMenu(ui).configure();
    }
    out
}

#[forge::hook(offset = 0x5490fc)]
unsafe fn update(ui: u32) -> u32 {
    let m = BoxMenu(ui);
    if m.panel() == 0 {
        return native_menu(false, || original!(ui));
    }
    let phase = m.phase();
    let weapon = m.weapon();
    let pad: u32 = unsafe { read(forge::mem::text_addr(), 0x188c670) };
    if pad == 0 {
        return 1;
    }
    let pressed: u32 = unsafe { read(pad, 27644) };
    let repeat: u32 = unsafe { read(pad, 27700) };
    if let Some((_, entry)) = inventory::selected() {
        if phase == 1
            && GUIDE_PAGE.with(|p| crate::weapon::style::manual::style_guide(entry, pressed, p))
        {
            return 1;
        }
        // The displayed row is the registered style. Project the fallback
        // style row for GU's "already equipped" check on the way to Hunter Arts.
        if phase == 1 && pressed & ACCEPT != 0 {
            unsafe {
                let row = gu!(0x561270, read::<u32>(m.panel(), 104));
                set(m.cursor(), 13, row as u8);
            }
        }
        let out = native_menu(true, || original!(ui));
        if phase != 1 && m.phase() == 1 {
            m.configure();
        }
        if m.phase() == 5 {
            if phase == 1 {
                // The single visible style row must not carry into the art slots.
                unsafe { gu!(0x557d90, m.cursor(), 0) };
            }
            m.equipped_label();
        }
        return out;
    }
    if m.entries().next().is_none() {
        return native_menu(false, || original!(ui));
    }
    if matches!(phase, 1 | 2) {
        unsafe {
            if phase == 2 {
                if let Some(e) = m.custom() {
                    m.confirm(e, pressed | repeat);
                    return 1;
                }
            } else if pressed & ACCEPT != 0 {
                if m.custom().is_some() {
                    set(ui, 120, 0u8);
                    set(ui, 128, 2u32);
                    gu!(0x54ecc0, m.cursor(), 1);
                    gu!(0x54ecc0, ui + 84, 1);
                    gu!(0x54f260, 0, 1);
                    return 1;
                }
            } else if pressed & BACK == 0 {
                m.navigate(pressed, repeat);
                return 1;
            }
        }
    }
    let out = native_menu((5..=9).contains(&phase), || original!(ui));
    let next = m.phase();
    if (phase == 1 && next == 5) || (phase == 3 && next == 4) {
        let had_custom_slots = m.equipped().is_some_and(|entry| entry.style.art_slots != 0);
        choose_style(weapon, None); // Only after GU accepts the native style.
        if had_custom_slots && phase == 1 {
            // GU skipped the normal slot migration because its saved style byte
            // already matched the selected row.
            unsafe {
                let style = read::<u32>(m.panel(), 104);
                gu!(0x549fb4, ui, 0, style);
                gu!(0x548f94, ui, 5);
            }
        }
    }
    if phase != 1 && next == 1 {
        m.configure();
    }
    if next == 5 && (phase != 5 || pressed | repeat != 0) {
        m.equipped_label();
    }
    out
}

pub(super) fn install(base: u32) {
    forge::install_hook!(base, open);
    forge::install_hook!(base, update);
}
