//! Shared Frontier behavior for weapon styles running in MHGU.
#![no_std]

pub mod action;
pub mod combat;
pub mod effects;
#[cfg(all(target_arch = "arm", target_endian = "little"))]
pub mod motion;
#[cfg(all(target_arch = "arm", target_endian = "little"))]
pub mod sound;
pub mod sprint;
#[cfg(all(target_arch = "arm", target_endian = "little"))]
pub mod sync;
#[cfg(all(target_arch = "arm", target_endian = "little"))]
pub mod trail;
