//! GU monster instances, behavior, and combat resources.
pub mod action;
pub mod anger;
#[cfg(all(target_arch = "arm", target_endian = "little"))]
pub mod attack;
#[cfg(any(test, all(target_arch = "arm", target_endian = "little")))]
pub mod damage;
#[cfg(all(target_arch = "arm", target_endian = "little"))]
mod instance;
pub mod motion;
pub mod movement;
#[cfg(all(target_arch = "arm", target_endian = "little"))]
pub use instance::{Monster, MonsterClass, MonsterState, MonsterUpdate};
#[cfg(all(target_arch = "arm", target_endian = "little"))]
mod scene;
#[cfg(all(target_arch = "arm", target_endian = "little"))]
pub mod shell;
#[cfg(all(target_arch = "arm", target_endian = "little"))]
mod skeleton;
