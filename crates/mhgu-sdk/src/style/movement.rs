//! GU hunter locomotion, terrain transitions and stamina.
use crate::{memory::entry, style::action};

core::arch::global_asm!(
    ".syntax unified",
    ".arm",
    ".fpu vfpv3",
    ".text",
    ".align 2",
    ".global mhgu_style_stamina",
    ".hidden mhgu_style_stamina",
    ".type mhgu_style_stamina,%function",
    "mhgu_style_stamina:",
    "vmov s0,r1",
    "mov r1,#0",
    "bx r2",
);
unsafe extern "C" {
    fn mhgu_style_stamina(actor: u32, amount: u32, entry: u32);
}

/// Adjust stamina through GU's continuous drain or discrete evade cost.
///
/// # Safety
/// The actor must be a live hunter on the game thread.
pub unsafe fn stamina(actor: u32, amount: f32, continuous: bool) {
    let offset = if continuous { 0x2a5ce4 } else { 0x2a5ebc };
    unsafe { mhgu_style_stamina(actor, amount.to_bits(), entry(offset) as u32) }
}

/// Use GU's running stamina latch and continuous drain together.
///
/// # Safety
/// The actor must be a live local hunter in a running action on the game thread.
pub unsafe fn drain_running_stamina(actor: u32) {
    unsafe {
        let running: unsafe extern "C" fn(u32, u32) = core::mem::transmute(entry(0x2a25a8));
        running(actor, 1);
        stamina(actor, -1., true);
    }
}

/// World stick direction relative to body facing, when GU has directional input.
///
/// # Safety
/// The actor must be a live hunter on the game thread.
pub unsafe fn relative_stick_yaw(actor: u32) -> Option<u16> {
    unsafe {
        let direction: unsafe extern "C" fn(u32, u32) -> u32 =
            core::mem::transmute(entry(0x27e608));
        if direction(actor, 0) == 0 {
            return None;
        }
        let aim: unsafe extern "C" fn(u32) -> u32 = core::mem::transmute(entry(0x27f5d0));
        Some(
            ((actor + 4076) as *const u16)
                .read()
                .wrapping_sub(aim(actor) as u16),
        )
    }
}

pub const CLIMBS: &[u16] = &[137, 138, 139];
const ROLL_OFF_LEDGE: (u16, u16) = (4, 14);

pub fn ground_idle(current: (u16, u16)) -> bool {
    matches!(current, (1, 0 | 46) | (2, 0 | 30))
}

/// GU 2E5810's ordinary weapon-down walk, sprint and stop phases.
pub fn sheathed_movement(current: (u16, u16)) -> bool {
    matches!(current, (2, 0..=3 | 5))
}

/// Let GU resolve the running direction, including its camera override.
///
/// # Safety
/// The actor must be a live hunter on the game thread.
pub unsafe fn steer(actor: u32) -> bool {
    unsafe {
        let direction: unsafe extern "C" fn(u32, u32) -> u32 =
            core::mem::transmute(entry(0x27e608));
        if direction(actor, 0) == 0 {
            return false;
        }
        let aim: unsafe extern "C" fn(u32) -> u32 = core::mem::transmute(entry(0x27f678));
        let yaw = aim(actor) & 0xffff;
        ((actor + 4076) as *mut u32).write(yaw);
        ((actor + 5280) as *mut u32).write(yaw);
        true
    }
}

/// Hand a ledge drop to GU and confirm the native falling slot was accepted.
///
/// # Safety
/// The actor must be a live hunter on the game thread.
pub unsafe fn roll_off_ledge(actor: u32) -> bool {
    unsafe {
        let change: unsafe extern "C" fn(u32, u32, u32, u32) =
            core::mem::transmute(entry(0x2831e0));
        let (group, slot) = ROLL_OFF_LEDGE;
        change(actor, group.into(), slot.into(), 0xc);
        action::current(actor) == ROLL_OFF_LEDGE
    }
}
