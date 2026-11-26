//! Frontier Extreme Gunlance on GU 1.4.
#![cfg_attr(target_arch = "arm", no_std)]

#[cfg(target_arch = "arm")]
mod battle;
#[cfg(target_arch = "arm")]
mod fx;
#[cfg(target_arch = "arm")]
mod heat;
#[cfg(target_arch = "arm")]
mod motion;
#[cfg(any(target_arch = "arm", test))]
mod program;
#[cfg(target_arch = "arm")]
mod shell;
#[cfg(target_arch = "arm")]
mod sound;

#[cfg(target_arch = "arm")]
const WEAPON: u8 = 9;
#[cfg(target_arch = "arm")]
const STYLE_ID: u32 = 15;

#[cfg(target_arch = "arm")]
macro_rules! native {
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
unsafe fn read<T: Copy>(actor: u32, offset: u32) -> T {
    unsafe { ((actor + offset) as *const T).read_unaligned() }
}

#[cfg(target_arch = "arm")]
unsafe fn write<T: Copy>(actor: u32, offset: u32, value: T) {
    unsafe { ((actor + offset) as *mut T).write_unaligned(value) }
}

#[cfg(target_arch = "arm")]
unsafe fn active(actor: u32) -> bool {
    use mhgu_sdk::style::{hunter, selected};
    unsafe {
        actor != 0
            && hunter::local() == Some(actor)
            && hunter::is_quest(actor)
            && hunter::equipped_weapon(actor) == Some(WEAPON)
            && selected(WEAPON) == Some(STYLE_ID)
    }
}

#[cfg(target_arch = "arm")]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
mod entry {
    use super::*;
    use mhgu_sdk::style::{Style, StyleRegistry, preload, remote, rules};

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

    static RULES: rules::Rules = rules::Rules {
        deflection: Some(mhfz_sdk::combat::deflection),
        ground_motion: Some(motion::finish_ground),
        camera_offset: Some(mhfz_sdk::camera::offset),
        ..rules::Rules::EMPTY
    };

    #[forge::entry]
    fn main() {
        mhgu_sdk::style::registry::when_ready(install_style);
    }

    extern "C" fn install_style() {
        let Some(registry) = StyleRegistry::current() else {
            return;
        };
        if let Err(error) = registry.register(
            &STYLE,
            c"极之型铳枪",
            c"点火后可蓄炮和连射，近战不耗斩味。\r\n\
              热度槽改为显示点火时间和冷却，\r\n\
              蓝槽全满时可再次点火。\r\n\
              龙击炮准备中按A可追加装填。\r\n\
              详见猎人笔记的武器操作指南。",
        ) {
            forge::log::prelude::warn!("Extreme Gunlance registration failed: {error:?}");
            return;
        }
        mhgu_sdk::style::manual::bind(STYLE_ID, include_str!("manual.txt"));
        unsafe { remote::bind_sheathe(STYLE_ID, &motion::SHEATHE_MOTIONS) };
        rules::bind(STYLE_ID, &RULES);
        if !unsafe { remote::bind_playback(STYLE_ID, &battle::PLAYBACK) } {
            forge::log::prelude::warn!("Style {}: remote action playback unavailable", STYLE_ID);
        }
        heat::install();
        fx::install();
        motion::install();
        shell::install();
        unsafe {
            preload::bind(STYLE_ID, motion::preload);
        }
    }
}
