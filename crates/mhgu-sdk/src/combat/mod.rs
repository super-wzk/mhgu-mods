//! Combat objects shared by different kinds of owners.
#[cfg(all(target_arch = "arm", target_endian = "little"))]
pub mod collision;
pub mod shell;
