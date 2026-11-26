#![cfg_attr(all(target_arch = "arm", target_endian = "little"), no_std)]
extern crate alloc;
#[cfg(any(test, all(target_arch = "arm", target_endian = "little")))]
mod config;

#[cfg(any(test, all(target_arch = "arm", target_endian = "little")))]
mod monster;
#[cfg(all(target_arch = "arm", target_endian = "little"))]
mod style;
#[cfg(all(target_arch = "arm", target_endian = "little"))]
mod world_contact;

#[cfg(all(target_arch = "arm", target_endian = "little"))]
// Forge 1.5 generates a safe extern handler that dereferences its ABI argument.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
mod entry {
    use super::{monster, style, world_contact};

    #[forge::entry]
    fn start() {
        let base = forge::mem::text_addr();
        monster::install(base);
        style::install(base);
        world_contact::install(base);
        unsafe {
            mhgu_sdk::monster::species::registry::finish_startup();
            mhgu_sdk::style::registry::finish_startup();
        }
    }
}
