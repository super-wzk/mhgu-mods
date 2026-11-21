//! Frontier Extreme Great Sword on GU 1.4. Register its actions with the style registry.
#![cfg_attr(target_arch = "arm", no_std)]

#[cfg(target_arch = "arm")]
mod motion;
#[cfg(target_arch = "arm")]
extern crate alloc;
#[cfg(target_arch = "arm")]
mod battle;
#[cfg(target_arch = "arm")]
mod combat;
#[cfg(target_arch = "arm")]
mod fx;
#[cfg(target_arch = "arm")]
mod program;
#[cfg(target_arch = "arm")]
mod sound;

// Forge 1.5 generates a safe extern handler that dereferences its ABI argument.
#[cfg(target_arch = "arm")]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
mod entry {
    use super::motion;
    use mhgu_sdk::style::{
        Style, StyleRegistry,
        rules::{self, Rules},
    };

    static RULES: Rules = Rules {
        // FZ 8174277A exempts w00 state 65 from the stamina gate, with a 90° cone.
        guard_half_angle: 0x4000,
        deflection: Some(mhfz_sdk::combat::deflection),
        ground_motion: Some(finish_ground_motion),
        camera_offset: None,
    };

    extern "C" fn finish_ground_motion(slot: u16, phase: u8, motion: u32) -> bool {
        matches!(
            (slot, phase, motion),
            (1, 1, 5002) | (2, 1, 5003) | (6, 2, 5012 | 5013)
        )
    }

    static EXTREME: Style = Style {
        id: 1,
        weapon: 0,
        encode: Some(motion::encode),
        decode: Some(motion::decode),
        independent: true,
        art_slots: 3,
        controller: Some(super::battle::dispatch),
        request: Some(super::battle::request),
        actions: super::battle::ACTIONS,
        guard: Some(super::battle::guard_policy),
        commands: Some(super::battle::commands),
    };

    #[forge::entry]
    fn main() {
        let Some(registry) = StyleRegistry::current() else {
            forge::log::prelude::warn!("Extreme GS disabled: style registry unavailable at init");
            return;
        };
        if let Err(err) = registry.register(&EXTREME, c"极之型大剑", c"怪物猎人边境极之型大剑。")
        {
            forge::log::prelude::warn!(
                "Extreme GS: could not register style {}: {err:?}",
                EXTREME.id
            );
            return;
        }
        if !rules::bind(EXTREME.id, &RULES) {
            forge::log::prelude::warn!("Style {}: GU rule service unavailable", EXTREME.id);
        }
        mhfz_sdk::effects::model::install();
        unsafe { mhgu_sdk::style::preload::bind(EXTREME.id, motion::preload) };
        motion::install();
        super::battle::install_peers();
        if !unsafe { mhgu_sdk::style::world_contact::bind(EXTREME.id, super::combat::expand) } {
            forge::log::prelude::warn!("Extreme GS: world contact service unavailable");
        }
        super::fx::install(EXTREME.id);
    }
}
