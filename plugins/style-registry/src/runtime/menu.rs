//! GU 1.4 equipment-box Change Style controller. Its list has eight real row widgets
//! (6376BC); 5490FC and 548E24, however, only have six native style records.
use super::{Entry, choose, entries, selected};

const STOCK: usize = 6;
const ROWS: [u32; 8] = [7, 8, 9, 10, 0x46, 0x47, 0x4e, 0x4f];
const ACCEPT: u32 = 0x2000;
const BACK: u32 = 0x4000;

unsafe fn read<T: Copy>(p: u32, offset: u32) -> T {
    unsafe { ((p + offset) as *const T).read_unaligned() }
}
unsafe fn set<T>(p: u32, offset: u32, value: T) {
    unsafe { ((p + offset) as *mut T).write_unaligned(value) }
}
// The GU calls below take ARM words and use the normal game calling convention.
macro_rules! gu {
    ($off:expr, $($arg:expr),+) => {{
        let f: unsafe extern "C" fn($(gu!(@word $arg)),+) -> u32 =
            core::mem::transmute(forge::mem::text_addr() + $off);
        f($($arg),+)
    }};
    (@word $arg:expr) => {u32};
}

// All methods borrow the controller only for the duration of its GU callback.
struct BoxMenu(u32);
impl BoxMenu {
    fn panel(&self) -> u32 {
        unsafe { read(self.0, 4) }
    }
    fn weapon(&self) -> u32 {
        unsafe { read(self.panel(), 100) }
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
        entries(self.weapon()).nth(self.row().checked_sub(STOCK)?)
    }
    fn equipped(&self) -> Option<&'static Entry> {
        let weapon = self.weapon();
        let id = selected(weapon);
        entries(weapon).find(|e| e.style.id == id)
    }
    fn configure(&self) {
        if self.panel() == 0 || self.phase() != 1 {
            return;
        }
        let weapon = self.weapon();
        let count = entries(weapon).count();
        if count == 0 {
            return;
        }
        let total = (STOCK + count).min(u16::MAX as usize) as u32;
        let active_id = selected(weapon);
        let initial = entries(weapon)
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
        let w = self.weapon();
        let id = selected(w);
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
            let mut extra = entries(w).skip(first.saturating_sub(STOCK));
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
    fn details(&self) {
        unsafe {
            if let Some(e) = self.custom() {
                let info = read::<u32>(self.0, 8);
                if info != 0 {
                    // Set up the native description pane using a valid record before replacing its text.
                    gu!(0x6394b8, info, 0, 0, 0);
                    gu!(0x56ea08, info, 2, 2, e.name.as_ptr() as u32);
                    gu!(0x56ea08, info, 3, 2, e.details.as_ptr() as u32);
                }
            } else if self.row() < STOCK {
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
                    choose(Some(e));
                    // Continue to the native Arts screen. It must never see a custom row as an art slot.
                    gu!(0x557d90, self.cursor(), 0);
                    gu!(0x549e24, self.0);
                    gu!(0x548f94, self.0, 5);
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
        if let Some(e) = self.equipped() {
            unsafe {
                gu!(0x56ea08, self.panel(), 0x55, 1, e.name.as_ptr() as u32);
            }
        }
    }
}

#[forge::hook(offset = 0x548b7c)]
unsafe fn open(ui: u32, panel: u32, help: u32, mode: u32) -> u32 {
    let out = crate::runtime::native_menu(|| original!(ui, panel, help, mode));
    if panel != 0 && help != 0 {
        BoxMenu(ui).configure();
    }
    out
}

#[forge::hook(offset = 0x5490fc)]
unsafe fn update(ui: u32) -> u32 {
    let m = BoxMenu(ui);
    if m.panel() == 0 {
        return crate::runtime::native_menu(|| original!(ui));
    }
    let phase = m.phase();
    let weapon = m.weapon();
    if entries(weapon).next().is_none() {
        return crate::runtime::native_menu(|| original!(ui));
    }
    let pad: u32 = unsafe { read(forge::mem::text_addr(), 0x188c670) };
    if pad == 0 {
        return 1;
    }
    let pressed: u32 = unsafe { read(pad, 27644) };
    let repeat: u32 = unsafe { read(pad, 27700) };
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
    let out = crate::runtime::native_menu(|| original!(ui));
    let next = m.phase();
    if ((phase == 1 && next == 5) || (phase == 3 && next == 4)) && selected(weapon) != 0 {
        choose(None); // Only after GU accepted the native style, including reselecting the equipped one.
    }
    if phase != 1 && next == 1 {
        m.configure();
    }
    if next == 5 && (phase != 5 || pressed | repeat != 0) {
        m.equipped_label();
    }
    out
}

// The status controller uses 275A30 for the local equipment record. Other
// hunters and equipment previews have their own record, even for the same weapon.
unsafe fn status_entry(equipment: u32) -> Option<&'static Entry> {
    let entry = super::active()?;
    unsafe {
        let manager: u32 = read(forge::mem::text_addr(), 0x1885b58);
        (manager != 0
            && equipment != 0
            && equipment == gu!(0x275a30, manager)
            && u32::from(read::<u8>(equipment, 1236)) == entry.style.weapon)
            .then_some(entry)
    }
}

#[forge::hook(offset = 0x62fb78)]
unsafe fn status_style(ui: u32, equipment: u32, details: u32) -> u32 {
    let out = original!(ui, equipment, details);
    unsafe {
        if let Some(entry) = status_entry(equipment) {
            gu!(0x56e808, ui, 0x5c, 1, entry.name.as_ptr() as u32);
            let cursor: u32 = read(ui, 80);
            if details != 0 && cursor != 0 && read::<u8>(cursor, 13) == 0 {
                gu!(0x56e808, ui, 0x14, 1, entry.name.as_ptr() as u32);
                gu!(0x56e808, ui, 0x14, 2, entry.details.as_ptr() as u32);
            }
        }
    }
    out
}

#[forge::hook(offset = 0x633450)]
unsafe fn status_style_small(ui: u32) -> u32 {
    let out = original!(ui);
    unsafe {
        if let Some(entry) = status_entry(read(ui, 76)) {
            gu!(0x56e808, ui, 0x69, 2, entry.name.as_ptr() as u32);
        }
    }
    out
}

pub(super) fn install(base: u32) {
    forge::install_hook!(base, open);
    forge::install_hook!(base, update);
    forge::install_hook!(base, status_style);
    forge::install_hook!(base, status_style_small);
}
