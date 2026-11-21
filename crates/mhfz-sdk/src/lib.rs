//! Shared Frontier behavior for weapon styles running in MHGU.
#![no_std]

#[cfg(all(target_arch = "arm", target_endian = "little"))]
extern crate alloc;

pub mod action;
pub mod combat;
pub mod effects;
#[cfg(all(target_arch = "arm", target_endian = "little"))]
pub mod motion;
#[cfg(all(target_arch = "arm", target_endian = "little"))]
pub mod sound;
pub mod sprint;
#[cfg(all(target_arch = "arm", target_endian = "little"))]
pub use mhgu_sdk::sync;
#[cfg(all(target_arch = "arm", target_endian = "little"))]
pub mod trail;
#[cfg(all(target_arch = "arm", target_endian = "little"))]
pub mod camera;
