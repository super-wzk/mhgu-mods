//! Independent style actions on GU's shared hunter state lifecycle.
//!
//! The weapon group remains native (16); the upper half of its slot namespace
//! belongs to the selected custom style, rather than to a stock weapon handler.

pub const GROUP: u16 = 16;
pub const FIRST: u16 = 0x8000;

pub const fn slot(action: u8) -> u16 {
    FIRST + action as u16
}

pub const fn custom(group: u16, slot: u16) -> bool {
    group == GROUP && slot >= FIRST && slot < FIRST + 256
}

/// Request a style action through GU's common change-state implementation.
/// It resets the native action phase, previous-action history, contact state,
/// interruption flags and motion bookkeeping before the style starts its clip.
///
/// # Safety
/// `actor` must be the selected local hunter during its controller callback.
#[cfg(all(target_arch = "arm", target_endian = "little"))]
pub unsafe fn enter(actor: u32, action: u8) -> bool {
    unsafe {
        let change: unsafe extern "C" fn(u32, u32, u32, u32) -> u32 =
            core::mem::transmute(crate::memory::entry(0x2ab670));
        change(actor, GROUP.into(), slot(action).into(), 0);
        ((actor + 9478) as *const u16).read() == GROUP
            && ((actor + 9482) as *const u16).read() == slot(action)
    }
}
