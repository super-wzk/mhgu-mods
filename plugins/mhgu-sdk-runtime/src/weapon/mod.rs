//! Equipment selection, extended weapons, and registered hunting styles.
mod guild;
mod hud;
mod icon;
mod inventory;
mod menu;
mod model;
mod profile;
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

unsafe fn local_hunter() -> u32 {
    unsafe { mhgu_sdk::weapon::style::hunter::local() }.unwrap_or(0)
}

pub(super) fn install(base: u32) {
    if style::install(base) {
        inventory::install(base);
        guild::install(base);
        hud::install(base);
        icon::install(base);
        menu::install(base);
        model::install(base);
        stats::install(base);
        profile::install(base);
        sets::install(base);
        trade::install(base);
    }
}
