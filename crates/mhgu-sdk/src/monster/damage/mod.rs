//! Monster health, part breaks and native flinch programs.
mod program;
#[cfg(all(target_arch = "arm", target_endian = "little"))]
mod state;
pub use program::{DamageMotionEnd, DamageProgram, DamageStep};
