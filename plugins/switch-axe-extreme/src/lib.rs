//! Frontier Extreme Switch Axe F for GU 1.4.
#![cfg_attr(target_arch = "arm", no_std)]
#[cfg(target_arch = "arm")]
extern crate alloc;
#[cfg(target_arch = "arm")]
const STYLE_ID: u32 = 4;
#[cfg(target_arch = "arm")]
const WEAPON_ID: u8 = 8;
#[cfg(target_arch = "arm")]
mod battle;
#[cfg(target_arch = "arm")]
mod fx;
#[cfg(target_arch = "arm")]
mod hud;
#[cfg(target_arch = "arm")]
mod motion;
#[cfg(any(test, target_arch = "arm"))]
mod program;
#[cfg(target_arch = "arm")]
mod sound;
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
#[allow(clippy::not_unsafe_ptr_arg_deref)]
mod entry {
    use super::{STYLE_ID, WEAPON_ID, battle, motion, program};
    use mhgu_sdk::style::{
        Style, StyleRegistry,
        rules::{self, Rules},
    };

    static RULES: Rules = Rules {
        deflection: Some(mhfz_sdk::combat::deflection),
        // FZ 8181353A reads 90° from mhfdat +0x65C, weapon row 12.
        guard_half_angle: 0x4000,
        camera_offset: Some(mhfz_sdk::camera::offset),
        ..Rules::EMPTY
    };
    static EXTREME: Style = Style {
        id: STYLE_ID,
        weapon: WEAPON_ID as u32,
        independent: true,
        art_slots: 3,
        encode: Some(motion::encode),
        decode: Some(motion::decode),
        controller: Some(battle::dispatch),
        request: Some(battle::request),
        guard: Some(battle::guard_policy),
        actions: program::ACTIONS,
        commands: Some(battle::commands),
    };
    #[forge::entry]
    fn main() {
        mhgu_sdk::style::registry::when_ready(install_style);
    }

    extern "C" fn install_style() {
        let Some(registry) = StyleRegistry::current() else {
            forge::log::prelude::warn!("Extreme Switch Axe: missing style registry");
            return;
        };
        if let Err(error) =
            registry.register(&EXTREME, c"极之型斩斧 F", c"怪物猎人边境极之型斩斧 F。")
        {
            forge::log::prelude::warn!("Extreme Switch Axe registration failed: {error:?}");
            return;
        }
        mhgu_sdk::style::manual::bind(STYLE_ID, include_str!("manual.txt"));
        if !rules::bind(STYLE_ID, &RULES) {
            forge::log::prelude::warn!("Style {}: GU rule service unavailable", STYLE_ID);
        }
        mhfz_sdk::effects::model::install();
        battle::install();
        unsafe { mhgu_sdk::style::preload::bind(STYLE_ID, motion::preload) };
    }
}
