//! Great sword precise guard against GU's live monster body shapes.
use core::sync::atomic::{AtomicU32, Ordering::Relaxed};

use super::{
    STYLE_ID,
    battle::{
        edit_local_guard_hold, edit_peer_guard_hold, guard_body_active, guarding_peer_slot, read,
        write,
    },
    fx,
};
use mhgu_sdk::{
    geometry::Angle,
    style::{
        self, hunter,
        remote::{PLAYER_COUNT, Roster},
    },
};

const POSITION: u32 = 64;
const STATE: u32 = 5160;
const AREA: u32 = 4180;
const BODY_RADIUS: f32 = 50.; // GU 17CF40 hunter sphere, centered 50 units above the feet.
const EXTRA_HALF_WIDTH: f32 = 30.;
const HELD_LATERAL_RATE: f32 = 0.8;
const MAX_STEP: f32 = 300.; // Leave large position corrections to GU.
static IMPACT_OWNER: AtomicU32 = AtomicU32::new(0);

type Point = [f32; 3];

struct Barrier {
    hunter: u32,
    peer: Option<usize>,
    position: Point,
    center: Point,
    forward: Point,
    holding: bool,
}

pub(super) struct Hold {
    monster: u32,
    state: u32,
    relative_root: Point,
    forward: Point,
}

fn edit_guard_hold(hunter: u32, peer: Option<usize>, edit: impl FnOnce(&mut Option<Hold>)) {
    if let Some(index) = peer {
        edit_peer_guard_hold(index, hunter, edit);
    } else {
        edit_local_guard_hold(hunter, edit);
    }
}

impl Barrier {
    fn edit_hold(&self, edit: impl FnOnce(&mut Option<Hold>)) {
        edit_guard_hold(self.hunter, self.peer, edit);
    }

    fn hold_step(&mut self, monster: u32, state: u32, start: Point, delta: Point, allowed: &mut Point) {
        let relative_root = sub(start, self.position);
        let mut holding = false;
        self.edit_hold(|slot| {
            let Some(hold) = slot.as_ref().filter(|hold| hold.monster == monster) else {
                return;
            };
            let forward = hold.forward;
            let across = [forward[2], 0., -forward[0]];
            let from_contact = sub(relative_root, hold.relative_root);
            let depth = dot(from_contact, forward);
            let side = dot(advance(from_contact, delta, 1.), across);
            if hold.state != state
                || !is_frontal(delta, [-forward[0], 0., -forward[2]])
                || depth * depth + side * side > BODY_RADIUS * BODY_RADIUS
            {
                *slot = None;
                return;
            }
            holding = true;
            let inward = (-dot(*allowed, forward)).max(0.) / dot(forward, forward);
            *allowed = advance(*allowed, forward, inward);
        });
        self.holding = holding;
    }
}

fn dot(a: Point, b: Point) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn sub(a: Point, b: Point) -> Point {
    core::array::from_fn(|i| a[i] - b[i])
}

fn advance(a: Point, v: Point, t: f32) -> Point {
    core::array::from_fn(|i| a[i] + v[i] * t)
}

fn is_frontal(direction: Point, forward: Point) -> bool {
    let front = dot(direction, forward);
    front > 0. && front * front >= 0.5 * dot(direction, direction)
}

pub(super) fn arm_impact(hunter: u32) {
    IMPACT_OWNER.store(hunter, Relaxed);
}

unsafe fn play_impact(hunter: u32) {
    if IMPACT_OWNER.compare_exchange(hunter, 0, Relaxed, Relaxed).is_ok() {
        unsafe { fx::guard_body_impact(hunter) };
    }
}

#[repr(C, align(16))]
#[derive(Default)]
struct BodyShape {
    start: Point,
    _pad0: f32,
    end: Point,
    _pad1: f32,
    radius: f32,
    _pad2: [f32; 3],
}

struct Contact {
    safe: f32,
    normal: Point,
    centered: bool,
    guard_index: usize,
}

impl BodyShape {
    fn sweep(
        &self,
        guard_index: usize,
        barrier: &Barrier,
        delta: Point,
        steps: u32,
        hit: &mut Option<Contact>,
    ) {
        let Barrier {
            center, forward, ..
        } = *barrier;
        let axis = sub(self.end, self.start);
        let axis_len = dot(axis, axis);
        let radius = BODY_RADIUS + self.radius;
        let across = [forward[2], 0., -forward[0]];
        // Three overlapping GU-sized spheres widen only the sides.
        for offset in [0., -EXTRA_HALF_WIDTH, EXTRA_HALF_WIDTH] {
            let probe = advance(center, across, offset);
            let overlap = |t| {
                let relative = sub(advance(probe, delta, -t), self.start);
                let along = if axis_len > 0. {
                    (dot(relative, axis) / axis_len).clamp(0., 1.)
                } else {
                    0.
                };
                let closest = advance(self.start, axis, along);
                let normal = sub(advance(closest, delta, t), probe);
                let horizontal = [normal[0], 0., normal[2]];
                (dot(normal, normal) <= radius * radius
                    && dot(horizontal, forward) > 0.
                    && dot(horizontal, delta) < 0.)
                    .then_some(horizontal)
            };

            let mut safe = 0.;
            for step in 0..=steps {
                if hit.as_ref().is_some_and(|earlier| safe > earlier.safe) {
                    break;
                }
                let mut contact = step as f32 / steps as f32;
                let Some(mut normal) = overlap(contact) else {
                    safe = contact;
                    continue;
                };
                for _ in 0..5 {
                    let mid = (safe + contact) * 0.5;
                    if let Some(at_mid) = overlap(mid) {
                        contact = mid;
                        normal = at_mid;
                    } else {
                        safe = mid;
                    }
                }
                // The side probes find contact earlier. Judge whether it is
                // head-on relative to the hunter, regardless of which probe hit.
                let from_center = advance(normal, across, offset);
                let centered = is_frontal(from_center, forward)
                    && is_frontal(delta, [-forward[0], 0., -forward[2]]);
                if hit.as_ref().is_none_or(|earlier| {
                    safe < earlier.safe || (safe == earlier.safe && centered && !earlier.centered)
                }) {
                    *hit = Some(Contact {
                        safe,
                        normal,
                        centered,
                        guard_index,
                    });
                }
                break;
            }
        }
    }
}

unsafe fn has_ground_body(state: u32) -> bool {
    unsafe {
        state != 0 && read::<u8>(state, 442) == 0
            && read::<u8>(state, 1320) == 0 // GU 17C9D0 body collision enabled
            && read::<u8>(state, 2328) == 0
    }
}

unsafe fn local_guard_active(actor: u32) -> bool {
    unsafe {
        style::selected(0) == Some(STYLE_ID)
            && hunter::equipped_weapon(actor) == Some(0)
            && hunter::is_quest(actor)
            && guard_body_active(actor)
    }
}

unsafe fn facing(hunter: u32) -> Point {
    let (sin, cos) = Angle::from_units(unsafe { read(hunter, 4076) }).sin_cos();
    [sin, 0., cos]
}

unsafe fn barriers(monster: u32, start: Point) -> [Option<Barrier>; PLAYER_COUNT + 1] {
    unsafe {
        let visual_offset = sub(read(monster, POSITION), start);
        let area = read::<u8>(monster, AREA);
        let make = |actor, peer| {
            let position = read(actor, POSITION);
            let mut center = advance(position, visual_offset, 1.);
            center[1] += BODY_RADIUS;
            Barrier {
                hunter: actor,
                peer,
                position,
                center,
                forward: facing(actor),
                holding: false,
            }
        };
        let mut guards = core::array::from_fn(|_| None);
        if let Some(local) = hunter::local()
            && read::<u8>(local, AREA) == area
            && local_guard_active(local)
        {
            guards[0] = Some(make(local, None));
        }
        if let Some(roster) = Roster::current() {
            for index in 0..PLAYER_COUNT {
                if let Some(peer) = roster.peer(index)
                    && guarding_peer_slot(peer, &roster).is_some()
                    && read::<u8>(peer, AREA) == area
                {
                    guards[index + 1] = Some(make(peer, Some(index)));
                }
            }
        }
        guards
    }
}

unsafe fn body_contact(monster: u32, guards: &[Option<Barrier>], delta: Point) -> Option<Contact> {
    unsafe {
        let base = forge::mem::text_addr();
        // 17CF40 iterates these same body rows and excludes disabled parts.
        let rows: unsafe extern "C" fn(u32) -> u32 = core::mem::transmute(base + 0x710b0);
        let mut row = rows(monster);
        if row == 0 {
            return None;
        }
        let manager = read::<u32>(base, 0x1857830);
        let part_disabled: unsafe extern "C" fn(u32, u32) -> u32 =
            core::mem::transmute(read::<u32>(read(monster, 0), 880));
        let steps = (delta[0].abs().max(delta[2].abs()) / 10.) as u32 + 1;
        let mut hit = None;
        while read::<u16>(row, 0) != 0xffff {
            if read::<u16>(row, 10) & 0x80d2 == 0
                && part_disabled(monster, read::<u16>(row, 12).into()) != 1
            {
                let mut body = BodyShape::default();
                match gs_resolve_body_shape(manager, monster, row, &mut body, 1, base + 0x16b9c0) {
                    1 => body.end = body.start,
                    2 | 3 => {}
                    _ => {
                        row += 44;
                        continue;
                    }
                }
                if body.radius > 0. && body.radius.is_finite() {
                    for (index, guard) in guards.iter().enumerate() {
                        if let Some(guard) = guard.as_ref().filter(|guard| !guard.holding) {
                            body.sweep(index, guard, delta, steps, &mut hit);
                        }
                    }
                }
            }
            row += 44;
        }
        hit
    }
}

// GU 17D0AC loads the body resolver's scale argument into s0. The
// Forge target is soft-float, so supply the native register explicitly.
core::arch::global_asm!(
    ".syntax unified",
    ".arm",
    ".fpu vfpv3",
    ".text",
    ".align 2",
    ".global gs_resolve_body_shape",
    ".hidden gs_resolve_body_shape",
    ".type gs_resolve_body_shape,%function",
    "gs_resolve_body_shape:",
    "ldr ip,[sp,#4]",
    "vmov.f32 s0,#1.0",
    "bx ip",
);

unsafe extern "C" {
    fn gs_resolve_body_shape(
        manager: u32,
        monster: u32,
        row: u32,
        shape: *mut BodyShape,
        mode: u32,
        entry: u32,
    ) -> u32;
}

// A6084 publishes the proposed root and delta; the caller next resolves
// terrain and then publishes the corrected position to the model.
#[forge::hook(offset = 0xa6084)]
unsafe fn monster_step(monster: u32, posture: u32) -> u32 {
    let area = unsafe { read::<u8>(monster, AREA) };
    let result = original!(monster, posture);
    unsafe {
        let state = read::<u32>(monster, STATE);
        // GU marks a remote-master monster with +46583. Only its master
        // clips the root movement that GU later publishes to the other clients.
        if posture != 0
            || area != read::<u8>(monster, AREA)
            || read::<u8>(monster, 46583) != 0
            || !has_ground_body(state)
        {
            return result;
        }
        let start = read::<Point>(state, 0);
        let mut position = read::<Point>(state, POSITION);
        let delta = [position[0] - start[0], 0., position[2] - start[2]];
        let length = dot(delta, delta);
        if !(0.0..=MAX_STEP * MAX_STEP).contains(&length) || length == 0. {
            return result;
        }
        // The body shapes use the published pose. Translate the guards into
        // that coordinate frame before sweeping the proposed root movement.
        let mut guards = barriers(monster, start);
        let mut allowed = delta;
        for guard in guards.iter_mut().flatten() {
            guard.hold_step(monster, state, start, delta, &mut allowed);
        }
        if guards.iter().flatten().any(|guard| guard.holding) {
            allowed = allowed.map(|component| component * HELD_LATERAL_RATE);
        }
        if dot(allowed, allowed) > 0.
            && guards.iter().flatten().any(|guard| !guard.holding)
            && let Some(hit) = body_contact(monster, &guards, allowed)
        {
            let step = allowed;
            allowed = advance([0.; 3], step, hit.safe);
            if hit.centered {
                let guard = guards[hit.guard_index].as_ref().unwrap();
                guard.edit_hold(|slot| {
                    slot.get_or_insert(Hold {
                        monster,
                        state,
                        relative_root: sub(advance(start, allowed, 1.), guard.position),
                        forward: guard.forward,
                    });
                });
                play_impact(guard.hunter);
            } else {
                let slide = advance(
                    step,
                    hit.normal,
                    -dot(step, hit.normal) / dot(hit.normal, hit.normal),
                );
                allowed = advance(allowed, slide, 1. - hit.safe);
            }
        }
        if allowed != delta {
            position[0] = start[0] + allowed[0];
            position[2] = start[2] + allowed[2];
            write(state, POSITION, position);
            write(state, 800, position[0] - start[0]);
            write(state, 808, position[2] - start[2]);
        }
    }
    result
}

// The contact can move around as the monster animates. Its combined native
// push vector need not face forward after the guard has caught that monster.
#[forge::hook(offset = 0x17f188)]
unsafe fn body_push(
    manager: u32,
    hunter: u32,
    hunter_push: *const [f32; 4],
    monster: u32,
    monster_push: *const [f32; 4],
) -> u32 {
    unsafe {
        let state = read::<u32>(monster, STATE);
        let local = hunter::local() == Some(hunter);
        let peer = if local {
            None
        } else {
            Roster::current().and_then(|roster| guarding_peer_slot(hunter, &roster))
        };
        let guarding = if local {
            local_guard_active(hunter)
        } else {
            peer.is_some()
        };
        if guarding
            && has_ground_body(state)
            && read::<u8>(hunter, AREA) == read::<u8>(monster, AREA)
        {
            let mut push = hunter_push.read();
            let [x, _, z, _] = monster_push.read();
            let separation = [x, 0., z];
            let away_from_hunter = [-push[0], 0., -push[2]];
            let forward = facing(hunter);
            let front = dot(separation, forward) > 0. || dot(away_from_hunter, forward) > 0.;
            // A replica gets contact from the native push and leaves the
            // monster's position to its owner.
            let replica_contact = read::<u8>(monster, 46583) != 0
                && (push[0] != 0. || push[2] != 0.)
                && (is_frontal(separation, forward) || is_frontal(away_from_hunter, forward));
            let relative_root = sub(read(state, POSITION), read(hunter, POSITION));
            let mut held = false;
            edit_guard_hold(hunter, peer, |slot| {
                if let Some(hold) = slot.as_ref().filter(|hold| hold.monster == monster) {
                    let [dx, _, dz] = sub(relative_root, hold.relative_root);
                    held = hold.state == state && dx * dx + dz * dz <= BODY_RADIUS * BODY_RADIUS;
                    if !held {
                        *slot = None;
                    }
                }
                if replica_contact {
                    slot.get_or_insert(Hold {
                        monster,
                        state,
                        relative_root,
                        forward,
                    });
                }
            });
            if held || front {
                if replica_contact {
                    play_impact(hunter);
                }
                push[0] = 0.;
                push[2] = 0.;
                return original!(manager, hunter, &push, monster, monster_push);
            }
        }
    }
    original!(manager, hunter, hunter_push, monster, monster_push)
}

pub(super) fn install() {
    let base = forge::mem::text_addr();
    forge::install_hook!(base, monster_step);
    forge::install_hook!(base, body_push);
}
