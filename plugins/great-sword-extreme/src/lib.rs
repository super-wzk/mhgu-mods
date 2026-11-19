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
#[cfg(any(test, target_arch = "arm"))]
mod program;
#[cfg(target_arch = "arm")]
mod sound;

// Forge 1.5 generates a safe extern handler that dereferences its ABI argument.
#[cfg(target_arch = "arm")]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
mod entry {
    use super::motion;
    use mhgu_sdk::style::{Style, StyleRegistry};

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
        motion::install();
        super::combat::install();
        super::fx::install();
    }
}
