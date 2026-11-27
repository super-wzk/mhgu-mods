//! GU hunter locomotion, terrain transitions and stamina.
use crate::{
    combat::collision::hurt::terrain_between,
    geometry::Angle,
    memory::{Field, Object, entry, singleton},
    weapon::style::action,
};

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
    ".global mhgu_style_movement_rate",
    ".hidden mhgu_style_movement_rate",
    ".type mhgu_style_movement_rate,%function",
    "mhgu_style_movement_rate:",
    "ldr ip,[r0]",
    "ldr ip,[ip,#144]",
    "push {{r4,lr}}",
    "blx ip",
    "vmov r0,s0",
    "pop {{r4,pc}}",
);
unsafe extern "C" {
    fn mhgu_style_stamina(actor: u32, amount: u32, entry: u32);
    fn mhgu_style_movement_rate(actor: u32) -> u32;
}

/// The hunter root in world coordinates.
/// # Safety
/// The actor must be a live hunter on the game thread.
pub unsafe fn position(actor: u32) -> [f32; 3] {
    unsafe { ((actor + 64) as *const [f32; 3]).read() }
}

/// Effective delta used by GU 539F4C, including the hunter's movement rate.
/// # Safety
/// The actor must be a live hunter on the game thread.
pub unsafe fn motion_frames(actor: u32) -> f32 {
    unsafe {
        let frames = ((actor + 28) as *const f32).read();
        frames * f32::from_bits(mhgu_style_movement_rate(actor))
    }
}

/// Add a forward displacement in world units before GU settles the hunter's
/// root motion, ground and wall contact.
///
/// # Safety
/// The actor must be a live local quest hunter in its ground action update.
pub unsafe fn displace_forward(actor: u32, distance: f32) {
    unsafe { displace_local(actor, distance, 0.) };
}

/// Add a vertical displacement in world units during a private action's windup.
///
/// # Safety
/// The actor must be a live local quest hunter in its ground action update.
pub unsafe fn displace_vertical(actor: u32, distance: f32) {
    unsafe { displace_local(actor, 0., distance) };
}

unsafe fn displace_local(actor: u32, forward: f32, vertical: f32) {
    const VELOCITY: Field<[f32; 3]> = Field::at(4112);
    unsafe {
        let hunter = Object::at(actor).unwrap();
        let frames = hunter.get::<f32>(Field::at(28));
        if (forward == 0. && vertical == 0.)
            || !forward.is_finite()
            || !vertical.is_finite()
            || !frames.is_finite()
            || frames <= 0.
        {
            return;
        }
        let yaw = Angle::from_units(hunter.get(Field::at(4076)));
        let (sin, cos) = yaw.sin_cos();
        let speed = forward / frames;
        let velocity = hunter.get(VELOCITY);
        // GU 539EFC adds velocity × raw frame delta without consuming velocity.
        hunter.set(VELOCITY, [sin * speed, vertical / frames, cos * speed]);
        let integrate: unsafe extern "C" fn(u32) = core::mem::transmute(entry(0x539efc));
        integrate(actor);
        hunter.set(VELOCITY, velocity);
    }
}

pub struct AirborneStep {
    pub velocity: [f32; 3],
    pub blocked: bool,
    pub landed: bool,
}

/// Advance GU's velocity integrator, probe for a wall and use GU's floor landing.
/// Velocity and acceleration are in world units per 60 Hz frame and frame squared.
/// The extra forward and vertical distances apply after integration. A landing
/// tolerance enables floor checks while descending and sets their reach.
/// # Safety
/// The actor must be a live airborne quest hunter on the game thread.
pub unsafe fn advance_airborne(
    actor: u32,
    velocity: [f32; 3],
    acceleration: [f32; 3],
    [forward, vertical]: [f32; 2],
    landing_tolerance: Option<f32>,
) -> AirborneStep {
    const POSITION: Field<[f32; 3]> = Field::at(64);
    const VELOCITY: Field<[f32; 3]> = Field::at(4112);
    let hunter = unsafe { Object::at(actor) }.unwrap();
    let start = hunter.get(POSITION);
    hunter.set(VELOCITY, velocity);
    hunter.set(Field::at(4128), acceleration);
    let integrate: unsafe extern "C" fn(u32) = unsafe { core::mem::transmute(entry(0x539f4c)) };
    unsafe { integrate(actor) };
    let mut end = hunter.get(POSITION);
    if forward != 0. || vertical != 0. {
        let yaw = Angle::from_units(hunter.get(Field::at(4076)));
        let (sin, cos) = yaw.sin_cos();
        end[0] += sin * forward;
        end[1] += vertical;
        end[2] += cos * forward;
        hunter.set(POSITION, end);
    }
    let [mut vx, vy, mut vz] = hunter.get(VELOCITY);
    let blocked = (start[0] != end[0] || start[2] != end[2])
        && terrain_between(
            hunter,
            [start[0], start[1] + 90., start[2]],
            [end[0], end[1] + 90., end[2]],
        )
        .is_some();
    if blocked {
        hunter.set(POSITION, [start[0], end[1], start[2]]);
        vx = 0.;
        vz = 0.;
        hunter.set(VELOCITY, [vx, vy, vz]);
    }
    let descending = if vertical == 0. {
        vy < 0.
    } else {
        end[1] < start[1]
    };
    let landed = descending
        && landing_tolerance
            .is_some_and(|tolerance| unsafe { action::land_with_tolerance(actor, tolerance) });
    if landed {
        hunter.set(VELOCITY, [0.; 3]);
        hunter.set(Field::at(4128), [0f32; 3]);
    }
    AirborneStep {
        velocity: if landed { [0.; 3] } else { [vx, vy, vz] },
        blocked,
        landed,
    }
}

/// Adjust stamina through GU's continuous drain or discrete evade cost.
///
/// # Safety
/// The actor must be a live hunter on the game thread.
pub unsafe fn stamina(actor: u32, amount: f32, continuous: bool) {
    let offset = if continuous { 0x2a5ce4 } else { 0x2a5ebc };
    unsafe { mhgu_style_stamina(actor, amount.to_bits(), entry(offset) as u32) }
}

/// Charge a discrete stamina cost, then shorten Dash Juice and the stamina melody.
/// Both arguments are positive, and the duration is in 60 Hz frames.
///
/// # Safety
/// The actor must be a live local quest hunter on the game thread.
pub unsafe fn spend_stamina(actor: u32, cost: f32, waiver_frames: f32) {
    const DASH_JUICE: Field<f32> = Field::at(10500);
    const MELODY: Field<f32> = Field::at(10768);

    unsafe { stamina(actor, -cost, false) };
    if waiver_frames <= 0. {
        return;
    }
    let Some(hunter) = (unsafe { Object::at(actor) }) else {
        return;
    };
    let juice = hunter.get(DASH_JUICE);
    let melody = hunter.get(MELODY);
    if juice <= 0. && melody <= 0. {
        return;
    }
    let juice_left = (juice - waiver_frames).max(0.);
    let melody_left = (melody - waiver_frames).max(0.);
    if juice > 0. {
        hunter.set(DASH_JUICE, juice_left);
    }
    if melody > 0. {
        hunter.set(MELODY, melody_left);
        if melody_left == 0. {
            // GU clears the elapsed melody's level, extension timer and source.
            hunter.set(Field::at(10721), 0u8);
            hunter.set(Field::at(10884), 0f32);
            hunter.set(Field::at(10985), 0u8);
        }
    }
    if juice_left > 0. || melody_left > 0. {
        return;
    }

    unsafe {
        // Match the normal expiry cue in GU's hunter timer update (29DE64).
        let Some(quest) = singleton(0x188ad90) else {
            return;
        };
        let suppress: unsafe extern "C" fn(u32, u32, u32) -> u32 =
            core::mem::transmute(entry(0x3a5ed0));
        if suppress(quest.addr(), 5, 1) != 0 {
            return;
        }
        let vtable = (actor as *const *const usize).read();
        let alternate: unsafe extern "C" fn(u32) -> u32 =
            core::mem::transmute(vtable.add(984 / 4).read());
        let player_kind: unsafe extern "C" fn(u32) -> u32 = core::mem::transmute(entry(0x27b918));
        if player_kind(actor) != 0 || alternate(actor) == 0 {
            let cue: unsafe extern "C" fn(u32, u32, u32) =
                core::mem::transmute(vtable.add(940 / 4).read());
            cue(actor, 1, 36);
        }
    }
}

/// Check the discrete evade cost after GU's Constitution and food modifiers.
///
/// # Safety
/// The actor must be a live quest hunter on the game thread.
pub unsafe fn has_evade_stamina(actor: u32, cost: f32) -> bool {
    unsafe {
        // GU 2A3E50 waives stamina drain while the stamina effect is active.
        if ((actor + 10500) as *const f32).read() > 0. {
            return true;
        }
        let has_skill = |id| crate::weapon::style::hunter::has_skill(actor, id);
        let constitution = if [156, 275, 293].into_iter().any(has_skill) {
            2
        } else if [157, 286, 313].into_iter().any(has_skill) {
            1
        } else if has_skill(158) {
            -1
        } else if has_skill(159) {
            -2
        } else {
            0
        };
        let food_skill: unsafe extern "C" fn(u32, u32) -> u32 =
            core::mem::transmute(entry(0x2864d4));
        let multiplier = match constitution + i32::from(food_skill(actor, 4) != 0) {
            2.. => 0.5,
            1 => 0.75,
            -1 => 1.2,
            -2 => 1.35,
            _ => 1.,
        };
        ((actor + 5088) as *const f32).read() >= cost * multiplier
    }
}

/// # Safety
/// The actor must be a live local quest hunter on the game thread.
pub unsafe fn has_stamina(actor: u32) -> bool {
    unsafe { ((actor + 5088) as *const f32).read() > 0. }
}

/// Extend the invulnerability clock used by GU's evade action and damage checks.
/// # Safety
/// The actor must be a live local quest hunter on the game thread.
pub unsafe fn grant_evade_invulnerability(actor: u32, frames: f32) {
    let timer = (actor + 11072) as *mut f32;
    unsafe { timer.write(timer.read().max(frames)) };
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

fn camera_view() -> Option<[f32; 3]> {
    use crate::memory::{Field, Object, singleton};
    unsafe {
        let camera = Object::at(singleton(0x2126504)?.get(Field::at(84)))?;
        let eye = camera.get::<[f32; 3]>(Field::at(64));
        let target = camera.get::<[f32; 3]>(Field::at(96));
        Some(core::array::from_fn(|i| target[i] - eye[i]))
    }
}

/// The active world camera's normalized look direction.
///
/// # Safety
/// Call on GU's game thread while controlling the local hunter.
pub unsafe fn camera_direction() -> Option<[f32; 3]> {
    let view = camera_view()?;
    let length = libm::sqrtf(view.into_iter().map(|x| x * x).sum());
    (length.is_finite() && length > 0.).then(|| view.map(|x| x / length))
}

/// Project a world point to an offset from the center of GU's 1280×720 HUD.
///
/// # Safety
/// The active quest camera must be accessible on the game thread.
pub unsafe fn hud_projection(point: [f32; 3]) -> Option<[f32; 2]> {
    let camera = unsafe { Object::at(singleton(0x2126504)?.get(Field::at(84))) }?;
    let eye = camera.get::<[f32; 3]>(Field::at(64));
    let fov = camera.get::<f32>(Field::at(60));
    let forward = unsafe { camera_direction() }?;
    let horizontal = libm::hypotf(forward[0], forward[2]);
    if !(0. ..180.).contains(&fov) || fov == 0. || horizontal < 0.0001 {
        return None;
    }
    let right = [-forward[2] / horizontal, 0., forward[0] / horizontal];
    let up = [-right[2] * forward[1], horizontal, right[0] * forward[1]];
    let dot = |axis: [f32; 3]| (0..3).map(|i| (point[i] - eye[i]) * axis[i]).sum::<f32>();
    let depth = dot(forward);
    if depth <= 0. {
        return None;
    }
    let focal = 360. / libm::tanf(0.5 * fov.to_radians());
    let offset = [focal * dot(right) / depth, -focal * dot(up) / depth];
    offset.into_iter().all(f32::is_finite).then_some(offset)
}

/// Relative stick direction using the current camera azimuth during a held input.
///
/// # Safety
/// The actor must be a live local hunter on the game thread.
pub unsafe fn live_relative_stick_yaw(actor: u32) -> Option<u16> {
    unsafe {
        let relative = relative_stick_yaw(actor)?;
        if ((actor + 11990) as *const u8).read() != 1 {
            return Some(relative);
        }
        // 2A79CC saves the camera azimuth at +12016 until the held-input
        // lock clears. Replace that term of 27F5D0 without clearing GU's lock.
        let locked_yaw = ((actor + 12016) as *const u16).read();
        let live_yaw = camera_view().map_or(0, |[x, _, z]| {
            // Same argument order, scale and truncation as GU 27F5D0.
            (libm::atan2f(x, z) * f32::from_bits(0x4622f983) + 0.5) as i32 as u16
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
