//! Native monster behavior, distinct from the animation that plays it.
#[cfg(all(target_arch = "arm", target_endian = "little"))]
mod native;

/// Group and slot in GU's monster action interpreter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActionId {
    pub group: u8,
    pub slot: u8,
}
