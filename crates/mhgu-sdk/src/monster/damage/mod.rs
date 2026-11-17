//! Monster health, part breaks and native flinch programs.
mod program;
#[cfg(all(target_arch = "arm", target_endian = "little"))]
mod state;
pub use program::{DamageMotionEnd, DamageProgram, DamageStep};

/// Row to bind to a monster's hitzone slot in its loaded rEnemyDtTune.
#[derive(Clone, Copy)]
pub enum HitzoneRow {
    /// The default row with the same index as the slot.
    Default,
    /// A row in the resource's optional second table.
    Alternate(u8),
}
