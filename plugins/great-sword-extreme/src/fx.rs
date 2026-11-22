//! Event requests for converted Frontier EFL resources. GU owns emission,
//! particle curves, attachment, rendering and destruction.
use mhfz_sdk::effects::{self, MotionClock, MotionEvent};
use mhgu_sdk::style::{
    action::MotionFrame,
    particle::{self, Anchor, Bank, TrackedUnits},
};
mod charge;
mod depth;
mod ground;
pub(super) use charge::ChargeEffect;
pub(super) use ground::ground_strike;

const ROUTES: &[u8] = include_bytes!("../target/assets/effects/routes.bin");
const SHOCKWAVE_ROUTE: usize = 7; // Frontier bank 4022, selector 5.
const CHARGE_ROUTE: usize = 9;
const _: () = {
    let mut route = CHARGE_ROUTE;
    while route <= CHARGE_ROUTE + 4 {
        let at = route * 4 + 2;
        assert!(u16::from_le_bytes([ROUTES[at], ROUTES[at + 1]]) <= 32);
        route += 1;
    }
};
const MOTION_ROWS: &[[u8; 18]] = include_bytes!("../target/assets/effects/motion_fx.bin")
    .as_chunks::<18>()
    .0;
static PARTICLES: Bank = Bank::new(c"effect\\frontier_gs\\particles", ROUTES);

pub(super) unsafe fn preload() {
    unsafe { PARTICLES.preload() };
}

static CHARGE_UNITS: TrackedUnits<20> = TrackedUnits::new();

unsafe fn field<T: Copy>(p: u32, off: u32) -> T {
    unsafe { ((p + off) as *const T).read_unaligned() }
}
unsafe fn anchor(p: u32, joint: Option<u8>) -> [f32; 3] {
    unsafe {
        if let Some(matrix) = joint.and_then(|j| particle::joint_matrix(p, u32::from(j))) {
            return field(matrix, 48);
        }
        field(p, 64)
    }
}
enum Attachment {
    Burst(Option<u8>),
    World([f32; 3], u16),
    Follow(u8),
}
unsafe fn attachment(p: u32, mode: Attachment) -> Option<Anchor> {
    unsafe {
        Some(match mode {
            Attachment::Burst(joint) => Anchor::World(anchor(p, joint), field(p, 4076)),
            Attachment::World(position, yaw) => Anchor::World(position, yaw),
            Attachment::Follow(joint) => Anchor::Joint {
                parent: particle::parent(p)?,
                joint: i16::from(joint),
                offset: None,
            },
        })
    }
}

unsafe fn route(p: u32, route: usize, mode: Attachment) {
    unsafe {
        if let Some(anchor) = attachment(p, mode) {
            PARTICLES.emit(route, anchor);
        }
    }
}

unsafe fn route_layers(slot: usize, p: u32, route: usize, mode: Attachment, pending: u32) -> u32 {
    unsafe {
        let Some(anchor) = attachment(p, mode) else {
            return pending;
        };
        if route == CHARGE_ROUTE {
            CHARGE_UNITS.emit_layers(&PARTICLES, route, anchor, pending, 4 * slot)
        } else {
            PARTICLES.emit_layers(route, anchor, pending, |_, _| {})
        }
    }
}
pub(super) unsafe fn emit(p: u32, id: u16, joint: Option<u8>) {
    // Bank 4006 occupies routes 0 through 6, in selector order.
    if id <= 6 {
        unsafe { route(p, usize::from(id), Attachment::Burst(joint)) }
    }
}
pub(super) unsafe fn charged_slash(p: u32) {
    unsafe {
        route(p, 14, Attachment::Follow(2));
        let mut position = anchor(p, Some(2));
        position[1] = field(p, 4336);
        route(p, 15, Attachment::World(position, field(p, 4076)));
    }
}
// Frontier scene node 0 corresponds to the GU actor origin.
pub(super) unsafe fn shockwave_origin(p: u32, yaw: u16) -> [f32; 3] {
    let [x, y, z]: [f32; 3] = unsafe { field(p, 64) };
    let (sin, cos) = mhgu_sdk::geometry::Angle::from_units(yaw).sin_cos();
    [x + 200. * sin, y, z + 200. * cos]
}

pub(super) unsafe fn radiance(p: u32, position: [f32; 3], yaw: u16) {
    unsafe { route(p, SHOCKWAVE_ROUTE, Attachment::World(position, yaw)) }
}

pub(super) unsafe fn motion_events(
    clock: &mut MotionClock,
    p: u32,
    clip: u16,
    previous: f32,
    sample: MotionFrame,
) {
    let frame = sample.frame;
    unsafe {
        match clip {
            1449 if previous < 50. && frame >= 50. => {
                emit(p, 3, Some(0)); // FZ scene node 2 is the pelvis.
                ground::uppercut_burst(p, 5);
            }
            1450 if previous < 70. && frame >= 70. => ground::uppercut_burst(p, 6),
            1476 if sample.blend <= 0. => guard_counter(p, previous, frame),
            _ => {}
        }
        for event in clock.advance(MOTION_ROWS, clip, sample) {
            motion_effect(p, event);
        }
    }
}

unsafe fn motion_effect(p: u32, e: MotionEvent) {
    unsafe {
        if e.kind == 5 {
            if let Some((anchor, wet)) = effects::motion_ground(p, &e) {
                let route = match (e.selector, wet) {
                    (4, false) => 28,
                    (14, false) => 29,
                    (4, true) => 30,
                    (14, true) => 31,
                    (15, false) => 34,
                    (15, true) => 35,
                    _ => return,
                };
                PARTICLES.emit(route, anchor);
            }
            return;
        }
        let route = match (e.bank, e.selector) {
            (4022, 4) => 22,
            (4022, 9) => 23,
            (4022, 11) => 24,
            (4022, 12) => 25,
            (4022, 13) => 26,
            (4035, 3) => 27,
            (4022, 6) => 21,
            (4022, 10) => 19,
            (4022, 15) => 32,
            (4022, 17) => 33,
            _ => return,
        };
        let joint = match e.node {
            0 => None,
            10 => Some(2),
            14 => Some(8),
            _ => return,
        };
        let anchor = if let Some(joint) = joint {
            if e.kind == 7 && e.mode == 0 {
                let Some(parent) = particle::parent(p) else {
                    return;
                };
                Anchor::Joint {
                    parent,
                    joint: joint as i16,
                    offset: Some(e.offset),
                }
            } else {
                let Some(m) = particle::joint_matrix(p, joint) else {
                    return;
                };
                let m = field(m, 0);
                let angles = if e.mode == 0 {
                    effects::world_pose_angles(&m)
                } else {
                    [0; 3]
                };
                Anchor::RotatedWorld(particle::transform_offset(&m, e.offset), angles)
            }
        } else {
            let (position, yaw) = particle::world_offset(p, e.offset);
            Anchor::World(position, if e.mode == 5 { 0 } else { yaw })
        };
        PARTICLES.emit(route, anchor);
    }
}

unsafe fn guard_counter(p: u32, previous: f32, frame: f32) {
    // FZ 817C08AA uses the current motion interval and shares one emission
    // for the last two thresholds when both are crossed in the same update.
    let crossed = |at| previous <= at && at < frame;
    for (emit, route_id) in [
        (crossed(90.), 19),
        (crossed(90.), 20),
        (crossed(92.), 21),
        (crossed(100.) || crossed(108.), SHOCKWAVE_ROUTE),
    ] {
        if emit {
            unsafe {
                let yaw = field(p, 4076);
                let origin = shockwave_origin(p, yaw);
                route(p, route_id, Attachment::World(origin, yaw));
            }
        }
    }
}
pub(super) unsafe fn radiance_ready(p: u32) {
    // Frontier binds 4022:21 to hunter node 18, retargeted to GU joint 12.
    unsafe { route(p, 8, Attachment::Follow(12)) }
}
fn stop_charge(slot: usize) {
    unsafe { CHARGE_UNITS.stop_slots(4 * slot..4 * slot + 4) };
}

extern "C" fn release_charge(unit: u32) {
    CHARGE_UNITS.forget(unit);
}

pub(super) fn install(style: u32) {
    if !CHARGE_UNITS.bind(style, release_charge) {
        forge::log::prelude::warn!("Extreme GS: particle release service unavailable");
    }
    depth::install();
}
