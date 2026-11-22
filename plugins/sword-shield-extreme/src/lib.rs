//! Frontier Extreme Sword and Shield on GU 1.4.
#![cfg_attr(target_arch = "arm", no_std)]

#[cfg(target_arch = "arm")]
extern crate alloc;

#[cfg(target_arch = "arm")]
mod battle;
#[cfg(target_arch = "arm")]
mod collision;
#[cfg(target_arch = "arm")]
mod fx;
#[cfg(target_arch = "arm")]
mod motion;
#[cfg(any(target_arch = "arm", test))]
mod program;
#[cfg(target_arch = "arm")]
mod sound;
#[cfg(target_arch = "arm")]
mod trail;
#[cfg(target_arch = "arm")]
const WEAPON: u8 = 1;
#[cfg(target_arch = "arm")]
const STYLE_ID: u32 = 5;

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
    use super::*;
    use mhgu_sdk::style::{Style, StyleRegistry, preload, remote, rules, world_contact};
    static RULES: rules::Rules = rules::Rules {
        deflection: Some(mhfz_sdk::combat::deflection),
        ground_motion: Some(finish_ground),
        camera_offset: Some(mhfz_sdk::camera::offset),
        ..rules::Rules::EMPTY
    };
    extern "C" fn finish_ground(slot: u16, phase: u8, motion: u32) -> bool {
        matches!(
            (slot, phase, motion),
            (1, 1, 5002) | (2, 1, 5003) | (6, 2, 28 | 5012 | 5013)
        )
    }
    static STYLE: Style = Style {
        id: STYLE_ID,
        weapon: WEAPON as u32,
        encode: Some(motion::encode),
        decode: Some(motion::decode),
        independent: true,
        art_slots: 3,
        controller: Some(battle::dispatch),
        request: Some(battle::request),
        actions: program::ACTIONS,
        guard: None,
        commands: Some(battle::commands),
    };
    #[forge::entry]
    fn main() {
        let Some(registry) = StyleRegistry::current() else {
            return;
        };
        if let Err(err) = registry.register(
            &STYLE,
            c"极之型单手剑",
            c"持刀冲刺、回避攻击与盾击连段。混沌刃药强化回旋斩。",
        ) {
            forge::log::prelude::warn!("Extreme SnS registration failed: {err:?}");
            return;
        }
        rules::bind(STYLE_ID, &RULES);
        motion::install();
        mhfz_sdk::effects::model::install();
        trail::install();
        unsafe {
            preload::bind(STYLE_ID, motion::preload);
            remote::bind(STYLE_ID, battle::peer);
            remote::observe(STYLE_ID, battle::observe);
            if !world_contact::bind(STYLE_ID, collision::expand)
                || !world_contact::bind_pass(STYLE_ID, collision::submit_waves)
            {
                forge::log::prelude::warn!("Extreme SnS wave service unavailable");
            }
        }
    }
}
