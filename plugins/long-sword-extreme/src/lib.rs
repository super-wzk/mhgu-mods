//! Frontier Extreme Long Sword on GU 1.4.
#![cfg_attr(target_arch = "arm", no_std)]

#[cfg(target_arch = "arm")]
extern crate alloc;

#[cfg(target_arch = "arm")]
mod battle;
#[cfg(target_arch = "arm")]
mod collision;
#[cfg(target_arch = "arm")]
mod discharge;
#[cfg(target_arch = "arm")]
mod fx;
#[cfg(target_arch = "arm")]
mod hud;
#[cfg(target_arch = "arm")]
mod instant;
#[cfg(target_arch = "arm")]
mod motion;
#[cfg(target_arch = "arm")]
mod peer;
#[cfg(any(target_arch = "arm", test))]
mod program;
#[cfg(target_arch = "arm")]
mod release;
#[cfg(target_arch = "arm")]
mod sound;
#[cfg(target_arch = "arm")]
mod trail;

#[cfg(target_arch = "arm")]
const WEAPON: u8 = 7;
#[cfg(target_arch = "arm")]
const STYLE_ID: u32 = 3;

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
        (read::<u32>(actor, 0) == forge::mem::text_addr() + 0x174cf64).then_some(actor)
    }
}

#[cfg(target_arch = "arm")]
use core::sync::atomic::{AtomicBool, Ordering::Relaxed};
#[cfg(target_arch = "arm")]
static WORLD_CONTACT: AtomicBool = AtomicBool::new(false);

#[cfg(target_arch = "arm")]
fn world_contact_ready() -> bool {
    WORLD_CONTACT.load(Relaxed)
}

#[cfg(target_arch = "arm")]
unsafe extern "C" fn expand_world_contact(owner: u32, hit: u32) -> u32 {
    unsafe {
        let slot = peer::Slot::of(owner);
        let shapes = instant::expand(slot, owner, hit);
        if shapes == 0 {
            let waves = discharge::expand(slot, owner, hit);
            if waves == 0 {
                release::position_for_expansion(slot, owner, hit);
            }
            waves
        } else {
            shapes
        }
    }
}

// Forge 1.5's entry macro supplies the ABI bridge.
#[cfg(target_arch = "arm")]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
mod entry {
    use super::{STYLE_ID, WEAPON, battle, motion};
    use mhgu_sdk::style::{
        Style, StyleRegistry, remote,
        rules::{self, Rules},
    };

    static RULES: Rules = Rules {
        // FZ weapon row 7 uses 90°. GU 2F2BA0 rejects Guard Up hits for w07.
        guard_half_angle: 0x4000,
        deflection: Some(mhfz_sdk::combat::deflection),
        camera_offset: Some(mhfz_sdk::camera::offset),
        ..Rules::EMPTY
    };

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
        guard: Some(battle::guard_policy),
        commands: Some(battle::bind_commands),
    };

    #[forge::entry]
    fn main() {
        mhgu_sdk::style::registry::when_ready(install_style);
    }

    extern "C" fn install_style() {
        let Some(registry) = StyleRegistry::current() else {
            forge::log::prelude::warn!("Extreme LS disabled: style registry unavailable at init");
            return;
        };
        if let Err(err) = registry.register(
            &EXTREME,
            c"极之型太刀",
            c"樱花气刃斩首次命中增加425点斩槽。\r\n\
              练气解放圆月斩期间，极型招式\r\n\
              不消耗练气。集中影响蓄力与积槽。\r\n\
              化解受防御性能与防御强化影响。\r\n\
              详见猎人笔记的武器操作指南。",
        ) {
            forge::log::prelude::warn!("Extreme LS registration failed: {err:?}");
            return;
        }
        mhgu_sdk::style::manual::bind(STYLE_ID, include_str!("manual.txt"));
        if !rules::bind(STYLE_ID, &RULES) {
            forge::log::prelude::warn!("Style {}: GU rule service unavailable", STYLE_ID);
        }
        mhfz_sdk::effects::model::install();
        let bound =
            unsafe { mhgu_sdk::style::world_contact::bind(STYLE_ID, super::expand_world_contact) };
        super::WORLD_CONTACT.store(bound, super::Relaxed);
        if !bound {
            forge::log::prelude::warn!("Extreme LS: world contact service unavailable");
        }
        if !unsafe { remote::bind_playback(STYLE_ID, &super::peer::PLAYBACK) } {
            forge::log::prelude::warn!("Style {}: remote action playback unavailable", STYLE_ID);
        }
        unsafe { mhgu_sdk::style::preload::bind(STYLE_ID, motion::preload) };
        battle::install();
        super::hud::install();
    }
}
