//! Equipment selection, extended weapons, and registered hunting styles.
mod decorations;
mod guild;
mod hud;
mod icon;
mod inventory;
mod menu;
mod model;
mod profile;
mod records;
mod resource;
mod sets;
mod stats;
pub(super) mod style;
mod trade;

pub(super) unsafe fn read<T: Copy>(p: u32, offset: u32) -> T {
    unsafe { ((p + offset) as *const T).read_unaligned() }
}
pub(super) unsafe fn set<T>(p: u32, offset: u32, value: T) {
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

pub(super) use gu;

// GU's resident information dialog (slot 8, mode 0, text group 7).
static INFO_PANEL: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
use core::sync::atomic::Ordering::Relaxed;
struct Information;
impl Information {
    fn panel() -> u32 {
        let get: unsafe extern "C" fn() -> u32 =
            unsafe { core::mem::transmute(forge::mem::text_addr() + 0x65efd0) };
        unsafe { get() }
    }
    fn open(text: &core::ffi::CStr) -> bool {
        unsafe {
            let panel = Self::panel();
            if panel == 0
                || read::<u8>(panel, 24) != 0
                || gu!(0x65f190, 0, text.as_ptr() as u32, 0, 0, 0) == 0
            {
                return false;
            }
            INFO_PANEL.store(panel, Relaxed);
            true
        }
    }
    fn update(text: &core::ffi::CStr) {
        let panel = INFO_PANEL.load(Relaxed);
        if panel != 0 && Self::panel() == panel {
            unsafe {
                gu!(0x56e808, panel, 7, 1, text.as_ptr() as u32);
            }
        }
    }
    fn close() {
        let panel = INFO_PANEL.swap(0, Relaxed);
        if panel != 0 && Self::panel() == panel {
            unsafe {
                gu!(0x64c600, panel);
            }
        }
    }
}

// Stock mode 0 treats A as dismissal. In our readers the owning menu consumes it.
#[forge::hook(offset = 0x64bc40)]
unsafe fn information_input(panel: u32) -> u32 {
    if INFO_PANEL.load(Relaxed) == panel && unsafe { read::<u32>(panel, 76) } == 2 {
        return 2;
    }
    original!(panel)
}

unsafe fn local_hunter() -> u32 {
    unsafe { mhgu_sdk::weapon::style::hunter::local() }.unwrap_or(0)
}

pub(super) fn install(base: u32) {
    if style::install(base) {
        forge::install_hook!(base, information_input);
        inventory::install(base);
        decorations::install(base);
        guild::install(base);
        hud::install(base);
        icon::install(base);
        menu::install(base);
        model::install(base);
        stats::install(base);
        records::install(base);
        profile::install(base);
        sets::install(base);
        trade::install(base);
    }
}
