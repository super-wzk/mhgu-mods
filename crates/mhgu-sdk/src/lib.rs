//! Engine bindings for MHGU 1.4 (Switch ARM32). Value types also work in host tools.
#![no_std]

#[cfg(any(test, all(target_arch = "arm", target_endian = "little")))]
extern crate alloc;

pub mod combat;
pub mod geometry;
#[cfg(all(target_arch = "arm", target_endian = "little"))]
mod memory;
pub mod monster;
pub mod motion;
#[cfg(all(target_arch = "arm", target_endian = "little"))]
pub mod quest;
#[cfg(all(target_arch = "arm", target_endian = "little"))]
mod service;
pub mod style;
