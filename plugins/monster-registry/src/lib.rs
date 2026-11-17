#![cfg_attr(all(target_arch = "arm", target_endian = "little"), no_std)]
extern crate alloc;

#[cfg(any(test, all(target_arch = "arm", target_endian = "little")))]
mod records;
#[cfg(all(target_arch = "arm", target_endian = "little"))]
mod runtime;
