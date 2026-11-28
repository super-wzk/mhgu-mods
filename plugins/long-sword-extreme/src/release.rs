//! 解放連撃. The blade and the delayed part-attached strike have separate GU contacts.
use crate::peer::{COUNT, Slot};
use crate::{collision, fx, motion, native, read, sound, write};
use core::cell::{Cell, UnsafeCell};
use mhgu_sdk::combat::collision::{Contact, ContactPool};
use mhgu_sdk::weapon::style::action;

// All accesses run on GU's game thread. Copy values out before calling GU,
// which can reenter the style from a collision or action transition.
struct State {
    armed: Cell<u8>,
    mark: Cell<Option<Mark>>,
    active: Cell<Option<FollowUp>>,
    pool: UnsafeCell<Option<ContactPool>>,
}
unsafe impl Sync for State {}
static STATES: [State; COUNT] = [const {
    State {
        armed: Cell::new(0),
        mark: Cell::new(None),
        active: Cell::new(None),
        pool: UnsafeCell::new(None),
    }
}; COUNT];

const POWERS: [u16; 3] = [10, 60, 180];

#[derive(Clone, Copy)]
struct Mark {
    owner: u32,
    area: u8,
    target: u32,
    slot: u8,
    bone: u16,
    offset: [f32; 3],
    tier: usize,
}

impl Mark {
    unsafe fn target_is_live(self, actor: u32) -> bool {
        unsafe {
            self.owner == actor
                && read::<u8>(actor, 4180) == self.area
                && target(self.slot) == self.target
        }
    }

    unsafe fn world_position(self) -> Option<[f32; 3]> {
        let matrix = unsafe { matrix(self.target, self.bone) }?;
        let offset = transform(&matrix, self.offset);
        Some(core::array::from_fn(|i| matrix[12 + i] + offset[i]))
    }
}

#[derive(Clone, Copy)]
struct FollowUp {
    mark: Mark,
    shape_row: u32,
}

impl FollowUp {
    unsafe fn position_shape(self, actor: u32) -> bool {
        unsafe {
            if !self.mark.target_is_live(actor) {
                return false;
            }
            let Some(world) = self.mark.world_position() else {
                return false;
            };
            let Some(owner) = matrix(actor, 0) else {
                return false;
            };
            write(self.shape_row, 16, local(&owner, world));
            true
        }
    }
}

unsafe fn target(slot: u8) -> u32 {
    unsafe {
        let manager = read::<u32>(forge::mem::text_addr(), 0x1851fe0);
        if manager == 0 || slot >= 10 {
            return 0;
        }
        native!(0x5ceb4, manager, u32::from(slot))
    }
}

unsafe fn matrix(actor: u32, bone: u16) -> Option<[f32; 16]> {
    unsafe {
        let (at, index) = if bone == 255 {
            (84, u32::MAX)
        } else {
            (208, bone as i16 as u32)
        };
        let get: unsafe extern "C" fn(u32, u32) -> u32 =
            core::mem::transmute(read::<u32>(read(actor, 0), at));
        let p = get(actor, index);
        (p != 0).then(|| read(p, 0))
    }
}

fn transform(m: &[f32; 16], p: [f32; 3]) -> [f32; 3] {
    core::array::from_fn(|i| m[i] * p[0] + m[i + 4] * p[1] + m[i + 8] * p[2])
}

unsafe fn local(m: &[f32; 16], p: [f32; 3]) -> [f32; 3] {
    let mut inverse = [0.; 16];
    unsafe {
        native!(0x2332ac, inverse.as_mut_ptr() as u32, m.as_ptr() as u32);
    }
    transform(&inverse, core::array::from_fn(|i| p[i] - m[12 + i]))
}

pub(super) fn arm(slot: Slot, state: u8) {
    STATES[slot.0].armed.set(state);
}
pub(super) fn disarm(slot: Slot) {
    STATES[slot.0].armed.set(0);
}
pub(super) fn reset(slot: Slot) {
    disarm(slot);
    STATES[slot.0].mark.set(None);
    STATES[slot.0].active.set(None);
    if let Some(pool) = unsafe { &*STATES[slot.0].pool.get() } {
        for contact in pool.iter() {
            contact.disable();
        }
    }
}

pub(super) unsafe fn mark(slot: Slot, actor: u32, hit: u32, monster: u32) -> bool {
    let state = STATES[slot.0].armed.get();
    let tier = match state {
        76 => 0,
        83 => 1,
        84 => 2,
        _ => return false,
    };
    unsafe {
        if action::current(actor) != (action::GROUP, action::slot(state)) {
            return false;
        }
        let header = read::<u32>(monster, 5160);
        let part = read::<u32>(hit, 172);
        if header == 0 || part == 0 {
            return false;
        }
        let target_slot = read::<u8>(header, 437);
        if target(target_slot) != monster {
            return false;
        }
        // GU 16DE70 also expands hurt volumes using special transforms
        // (e.g. 1010). Keep their impact relative to the monster root.
        let joint: u16 = read(part, 0);
        let attachment = (joint < 1000)
            .then(|| matrix(monster, joint))
            .flatten()
            .map(|matrix| (joint, matrix))
            .or_else(|| matrix(monster, 255).map(|matrix| (255, matrix)));
        let Some((bone, m)) = attachment else {
            return false;
        };
        let position = read(hit, 32);
        let offset = local(&m, position);
        STATES[slot.0].mark.set(Some(Mark {
            owner: actor,
            area: read(actor, 4180),
            target: monster,
            slot: target_slot,
            bone,
            offset,
            tier,
        }));
        disarm(slot);
        fx::mark(actor, monster, bone, offset, position, 26 + 2 * tier);
        true
    }
}

unsafe fn launch(slot: Slot, actor: u32, hit: Contact<'_>, mark: Mark) {
    unsafe {
        let Some(template) = collision::blade_template(actor) else {
            return;
        };
        let Some(shape) = collision::RELEASE.load() else {
            return;
        };
        let Some(rows) = shape.rows_in_first_group(1) else {
            return;
        };
        let follow_up = FollowUp {
            mark,
            shape_row: read(rows, 0),
        };
        if follow_up.shape_row == 0 || !follow_up.position_shape(actor) {
            return;
        }
        collision::setup(
            actor,
            hit,
            template,
            shape.as_raw(),
            [0, 60],
            POWERS[mark.tier],
            0,
        );
        // 169878 checks the designated target for all native hit lists.
        let address = hit.as_raw();
        write(address, 140, mark.target);
        write(address, 48, read::<u32>(address, 48) | 0x0080_0000);
        STATES[slot.0].active.set(Some(follow_up));
        STATES[slot.0].mark.set(None);
        if let Some(position) = mark.world_position() {
            fx::world(actor, position, 25 + 2 * mark.tier);
            sound::release(actor, mark.tier as u8, position);
        }
    }
}

pub(super) unsafe fn collision_pass(slot: Slot, manager: u32) {
    unsafe {
        if STATES[slot.0].mark.get().is_none() && STATES[slot.0].active.get().is_none() {
            return;
        }
        let Some(actor) = slot.live() else {
            reset(slot);
            return;
        };
        if read::<i16>(actor, 5078) <= 0 {
            reset(slot);
            return;
        }
        let pool = (&mut *STATES[slot.0].pool.get()).get_or_insert_with(|| ContactPool::new(1));
        let hit = pool.iter().next().unwrap();
        if let Some(mark) = STATES[slot.0].mark.get() {
            // FZ 8115AC5E discards a pending mark on a hunter damage reaction
            // (source groups 2/7). GU dispatches hunter damage through group 64.
            if !mark.target_is_live(actor) || action::current(actor).0 == 64 {
                STATES[slot.0].mark.set(None);
            } else if motion::source_clip(actor, read(actor, 1204)) != Some(1448)
                || read::<f32>(actor, 1280) >= 148.
            {
                launch(slot, actor, hit, mark);
            }
        }
        if let Some(follow_up) = STATES[slot.0].active.get() {
            if follow_up.mark.owner == actor && hit.enabled() && follow_up.position_shape(actor) {
                hit.enqueue(manager);
            } else {
                hit.disable();
                STATES[slot.0].active.set(None);
            }
        }
    }
}

// GU shares the release HTS across hunters. Restore this hit's coordinates
// immediately before 1696FC expands its row into the collision manager.
pub(super) unsafe fn position_for_expansion(slot: Slot, owner: u32, hit: u32) {
    let state = &STATES[slot.0];
    if let Some(follow_up) = state.active.get()
        && follow_up.mark.owner == owner
        && unsafe { &*state.pool.get() }
            .as_ref()
            .is_some_and(|p| p.iter().any(|c| c.as_raw() == hit))
    {
        unsafe { follow_up.position_shape(owner) };
    }
}
