//! Frontier Extreme Dual Blades on GU 1.4.
#![cfg_attr(target_arch = "arm", no_std)]

#[cfg(target_arch = "arm")]
extern crate alloc;

#[cfg(target_arch = "arm")]
mod battle;
#[cfg(target_arch = "arm")]
mod fx;
#[cfg(target_arch = "arm")]
mod hud;
#[cfg(target_arch = "arm")]
mod motion;
#[cfg(target_arch = "arm")]
mod program;
#[cfg(target_arch = "arm")]
mod sound;

#[cfg(target_arch = "arm")]
const WEAPON: u8 = 11;
#[cfg(target_arch = "arm")]
const STYLE_ID: u32 = 2;

#[cfg(target_arch = "arm")]
macro_rules! native {
    ($offset:expr) => {{
        let f: unsafe extern "C" fn() -> u32 =
            core::mem::transmute((forge::mem::text_addr() + $offset) as usize);
        f()
    }};
    ($offset:expr, $($arg:expr),+ $(,)?) => {{
        let f: unsafe extern "C" fn($(native!(@type $arg)),+) -> u32 =
            core::mem::transmute((forge::mem::text_addr() + $offset) as usize);
        f($($arg),+)
    }};
    (@type $arg:expr) => {u32};
}
#[cfg(target_arch = "arm")]
use native;

#[cfg(target_arch = "arm")]
unsafe fn read<T: Copy>(address: u32, offset: u32) -> T {
    unsafe { ((address + offset) as *const T).read_unaligned() }
}
#[cfg(target_arch = "arm")]
unsafe fn write<T>(address: u32, offset: u32, value: T) {
    unsafe { ((address + offset) as *mut T).write_unaligned(value) }
}
#[cfg(target_arch = "arm")]
fn local_actor() -> Option<u32> {
    if mhgu_sdk::style::selected(WEAPON) != STYLE_ID {
        return None;
    }
    unsafe {
        let actor = mhgu_sdk::style::hunter::local()?;
        (read::<u32>(actor, 0) == forge::mem::text_addr() + 0x174f2b4).then_some(actor)
    }
}

// Forge 1.5's entry macro supplies the ABI bridge.
#[cfg(target_arch = "arm")]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
mod entry {
    use super::{STYLE_ID, WEAPON, battle, motion};
    use mhgu_sdk::style::{Style, StyleRegistry};

    static EXTREME: Style = Style {
        id: STYLE_ID,
        weapon: WEAPON as u32,
        encode: Some(motion::encode),
        decode: Some(motion::decode),
        independent: true,
        art_slots: 3,
        controller: Some(battle::dispatch),
        request: Some(battle::request),
        actions: super::program::ACTIONS,
        guard: None,
        commands: Some(battle::bind_commands),
    };

    #[forge::entry]
    fn main() {
        let Some(registry) = StyleRegistry::current() else {
            forge::log::prelude::warn!("Extreme DB disabled: style registry unavailable at init");
            return;
        };
        if let Err(err) = registry.register(&EXTREME, c"极之型双剑", c"怪物猎人边境极之型双剑。")
        {
            forge::log::prelude::warn!("Extreme DB registration failed: {err:?}");
            return;
        }
        battle::install();
        super::fx::install(EXTREME.id);
        super::hud::install();
    }
}
