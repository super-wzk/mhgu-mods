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

/// Defer natural stamina recovery through GU's countdown at 29DBDC.
/// GU lets the timer expire on its own when the action stops refreshing it.
///
/// # Safety
/// The actor must be a live local hunter on the game thread.
pub unsafe fn defer_stamina_recovery(actor: u32) {
    unsafe {
        let elapsed = ((actor + 28) as *const f32).read();
        let timer = (actor + 10092) as *mut f32;
        // Leave the timer positive after the next update at 30 or 60 Hz.
        timer.write(timer.read().max(elapsed.max(2.) + 1.));
    }
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

/// Relative stick direction using the current camera azimuth during a held input.
///
/// # Safety
/// The actor must be a live local hunter on the game thread.
pub unsafe fn live_relative_stick_yaw(actor: u32) -> Option<u16> {
    use crate::memory::{Field, Object, singleton};
    unsafe {
        let relative = relative_stick_yaw(actor)?;
        if ((actor + 11990) as *const u8).read() != 1 {
            return Some(relative);
        }
        // 2A79CC saves the camera azimuth at +12016 until the held-input
        // lock clears. Replace that term of 27F5D0 without clearing GU's lock.
        let locked_yaw = ((actor + 12016) as *const u16).read();
        let camera =
            singleton(0x2126504).and_then(|manager| Object::at(manager.get(Field::at(84))));
        let live_yaw = camera.map_or(0, |camera| {
            let [eye_x, _, eye_z] = camera.get::<[f32; 3]>(Field::at(64));
            let [target_x, _, target_z] = camera.get::<[f32; 3]>(Field::at(96));
            // Same argument order, scale and truncation as GU 27F5D0.
            (libm::atan2f(target_x - eye_x, target_z - eye_z) * f32::from_bits(0x4622f983) + 0.5)
                as i32 as u16
        });
        Some(relative.wrapping_add(locked_yaw).wrapping_sub(live_yaw))
    }
}

pub const CLIMBS: &[u16] = &[137, 138, 139];
const ROLL_OFF_LEDGE: (u16, u16) = (4, 14);

pub fn ground_idle(current: (u16, u16)) -> bool {
    matches!(current, (1, 0 | 46) | (2, 0 | 30))
}

/// GU 2E5810's weapon-down walk, sprint, stop and panic-run phases.
pub fn sheathed_movement(current: (u16, u16)) -> bool {
    matches!(current, (2, 0..=3 | 5 | 13..=15))
}

/// Set both the hunter's current and target facing.
///
/// # Safety
/// The actor must be a live hunter on the game thread.
pub unsafe fn set_facing(actor: u32, yaw: u16) {
    unsafe {
        ((actor + 4076) as *mut u32).write(u32::from(yaw));
        ((actor + 5280) as *mut u32).write(u32::from(yaw));
    }
}

/// Turn relative to the hunter's current facing and update its target facing.
///
/// # Safety
/// The actor must be a live hunter on the game thread.
pub unsafe fn turn(actor: u32, offset: u16) {
    unsafe {
        let facing = ((actor + 4076) as *const u16).read_unaligned();
        set_facing(actor, facing.wrapping_add(offset));
    }
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
        set_facing(actor, aim(actor) as u16);
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

/// Probe a slide's movement direction before entering GU's native falling action.
///
/// # Safety
/// The actor must be a live hunter on the game thread.
pub unsafe fn step_off_ledge(actor: u32, offset: u16) -> bool {
    unsafe {
        use crate::memory::{Field, Object};
        let Some(actor) = Object::at(actor) else {
            return false;
        };
        let probe: unsafe extern "C" fn(u32) -> u32 = core::mem::transmute(entry(0x2a8458));
        let facing = actor.get::<u32>(Field::at(4076));
        let yaw = (facing as u16).wrapping_add(offset);
        let airborne = actor.get::<u8>(Field::at(9477)) == 2;
        if !airborne {
            // GU's ledge probe caches a result for the current facing.
            if offset != 0 {
                actor.set(Field::at(4076), u32::from(yaw));
                actor.set(Field::at(11956), 0u32);
            }
            let ledge = probe(actor.addr()) == 1;
            if offset != 0 {
                actor.set(Field::at(4076), facing);
                actor.set(Field::at(11956), 0u32);
            }
            if !ledge {
                return false;
            }
        }
        let target_yaw = actor.get::<u32>(Field::at(5280));
        actor.set(Field::at(4076), u32::from(yaw));
        actor.set(Field::at(5280), u32::from(yaw));
        if roll_off_ledge(actor.addr()) {
            return true;
        }
        actor.set(Field::at(4076), facing);
        actor.set(Field::at(5280), target_yaw);
        false
    }
}
