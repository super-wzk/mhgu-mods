//! GU 1.4: independent melee contacts for the FZ Espinas action package.
//! The game's collision list links live objects by address until its collision pass.
#![no_std]
extern crate alloc;
use alloc::collections::BTreeMap;
use core::{
    cell::UnsafeCell,
    sync::atomic::{AtomicBool, Ordering},
};
use mhgu_sdk::{
    combat::collision::{Contact, ContactPool},
    geometry::{Angle, Bearing},
    monster::{
        Monster, MonsterState, MonsterUpdate, anger::AngerMode, attack::AttackTables,
        motion::MotionId, movement::Posture,
    },
};

struct Hits {
    contacts: ContactPool,
    cuts: u32,
    first: u32,
    active: usize,
    motion: Option<MotionId>,
    world: Option<World>,
}
struct World {
    hit: u32,
    radius: f32,
    center: Option<[f32; 3]>,
}
impl Hits {
    fn active_contacts(&self) -> impl Iterator<Item = Contact<'_>> {
        self.contacts.iter().take(self.active)
    }
    fn cancel(&mut self) {
        // The native collision list can still hold these addresses this pass.
        for hit in self.active_contacts() {
            hit.disable();
        }
        self.world = None;
        self.motion = None;
        self.active = 0;
    }
}
struct Actors<T> {
    locked: AtomicBool,
    value: UnsafeCell<BTreeMap<u32, T>>,
}
unsafe impl<T: Send> Sync for Actors<T> {}
impl<T> Actors<T> {
    const fn new() -> Self {
        Self {
            locked: AtomicBool::new(false),
            value: UnsafeCell::new(BTreeMap::new()),
        }
    }
    fn with<R>(&self, f: impl FnOnce(&mut BTreeMap<u32, T>) -> R) -> R {
        while self
            .locked
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            core::hint::spin_loop();
        }
        let r = f(unsafe { &mut *self.value.get() });
        self.locked.store(false, Ordering::Release);
        r
    }
}
static ACTORS: Actors<Hits> = Actors::new();
unsafe fn u32at(p: u32, off: u32) -> u32 {
    unsafe { ((p + off) as *const u32).read_unaligned() }
}
unsafe fn u16at(p: u32, off: u32) -> u16 {
    unsafe { ((p + off) as *const u16).read_unaligned() }
}
unsafe fn byte(p: u32, off: u32) -> u8 {
    unsafe { *((p + off) as *const u8) }
}
mod air;
mod ground;
mod hunt;
mod package;
mod physiology;
mod rathian;
mod reaction;

use package::{Batch, Directory, Package, row_id};
unsafe fn attack_batch<'a>(
    actor: u32,
    id: i32,
    flags: u32,
) -> Option<(Monster<'a>, AttackTables<'a>, Batch)> {
    if flags != 0 {
        return None;
    }
    let id = u32::try_from(id).ok()?;
    let monster = unsafe { Monster::from_raw(actor)? };
    let package = Package::read(monster)?;
    let batch = unsafe { package.batch(monster.motion(), id)? };
    Some((monster, package.tables, batch))
}

#[forge::hook(offset = 0xcf5f64)]
unsafe fn tick(actor: u32, flags: u32) -> u32 {
    let outcome = unsafe { MonsterUpdate::from_callback(actor) }
        .map_or(hunt::ActionOutcome::UseOriginal, hunt::update_action);
    match outcome {
        hunt::ActionOutcome::UseOriginal => original!(actor, flags),
        hunt::ActionOutcome::Handled => 0,
    }
}

#[forge::hook(offset = 0x7dc2c)]
unsafe fn arm(actor: u32, slot: u32, id: i32, flags: u32) -> i32 {
    let result = original!(actor, slot, id, flags);
    if result != 1 || slot != 0 {
        return result;
    }
    unsafe {
        let Some((monster, tables, batch)) = attack_batch(actor, id, flags) else {
            ACTORS.with(|actors| {
                if let Some(hits) = actors.get_mut(&actor) {
                    hits.cancel();
                }
            });
            return result;
        };
        let Batch {
            first,
            count,
            capacity,
            cuts,
            world,
            radius,
        } = batch;
        let Some(state) = monster.state() else {
            return result;
        };
        let anchor = state.primary_contact();
        let Some(first_row) = tables.row(row_id(first)) else {
            return result;
        };
        ACTORS.with(|actors| {
            let boxes = actors.entry(actor).or_insert_with(|| {
                let contacts = ContactPool::new((capacity - 1) as usize);
                Hits {
                    contacts,
                    cuts,
                    first,
                    active: 0,
                    motion: None,
                    world: None,
                }
            });
            boxes.cuts = cuts;
            boxes.first = first;
            boxes.motion = Some(monster.motion());
            boxes.active = (count - 1) as usize;
            let world_hit = match world {
                0 => None,
                1 => Some(anchor),
                n => boxes.active_contacts().nth(usize::from(n) - 2),
            };
            boxes.world = world_hit.map(|hit| World {
                hit: hit.as_raw(),
                radius,
                center: None,
            });
            // A loop may rearm after frame zero; align extra windows to the native primary clock.
            let shift = anchor.delay() + anchor.duration()
                - f32::from(first_row.start())
                - f32::from(first_row.duration());
            for hit in boxes.contacts.iter().skip(boxes.active) {
                hit.disable();
            }
            for (i, hit) in boxes.active_contacts().enumerate() {
                let Some(row) = tables.row(row_id(first + 1 + i as u32)) else {
                    hit.disable();
                    continue;
                };
                if !row.configure_melee(hit) {
                    hit.disable();
                    continue;
                }
                let start = f32::from(row.start()) + shift;
                let duration = f32::from(row.duration()) + start.min(0.);
                hit.set_window(start.max(0.), duration.max(0.));
                if duration <= 0. {
                    hit.disable();
                }
            }
        });
    }
    result
}

#[forge::hook(offset = 0x1696e4)]
unsafe fn submit(hit: u32) -> u32 {
    let r = original!(hit);
    // Object identity, rather than just its actor pointer, prevents double submission
    // when the native secondary slot or a projectile from the same owner is visited.
    let Some(contact) = (unsafe { Contact::from_raw(hit) }) else {
        return r;
    };
    let actor = contact.owner_address();
    if actor == 0 {
        return r;
    }
    ACTORS.with(|actors| {
        let Some(boxes) = actors.get_mut(&actor) else {
            return;
        };
        // Only owners inserted by arm() are known monster pointers.
        let Some(monster) = (unsafe { Monster::from_raw(actor) }) else {
            return;
        };
        let Some(state) = monster.state() else {
            return;
        };
        if hit != state.primary_contact().as_raw() {
            return;
        }
        if boxes.motion != Some(monster.motion()) {
            boxes.cancel();
            return;
        }
        let Some(tables) = monster.attack_tables() else {
            boxes.cancel();
            return;
        };
        let cut = state.tail_severed();
        let groups = tables.shape_groups();
        for (i, hit) in core::iter::once(contact)
            .chain(boxes.active_contacts())
            .enumerate()
        {
            let index = boxes.first + i as u32;
            let group = groups.and_then(|groups| {
                if cut {
                    groups.get(unsafe { byte(boxes.cuts, index) })
                } else {
                    tables.row(row_id(index))?.shape_group_in(groups)
                }
            });
            if !group.is_some_and(|g| hit.set_shape_group(g)) {
                hit.disable();
            } else if i != 0 {
                // GU already linked the primary; only append supplementary contacts.
                unsafe {
                    original!(hit.as_raw());
                }
            }
        }
    });
    r
}

// The collision manager advances the native clock before testing eligibility.
// GU leaves the enable byte set for the pass that consumes the last duration
// unit. FZ replay windows end at the authored boundary (also at a phase cut).
#[forge::hook(offset = 0x16919c)]
unsafe fn advance(hit: u32) -> u32 {
    let result = original!(hit);
    let Some(contact) = (unsafe { Contact::from_raw(hit) }) else {
        return result;
    };
    if contact.enabled() && contact.delay() <= 0. {
        let actor = contact.owner_address();
        ACTORS.with(|actors| {
            let Some(hits) = actors.get_mut(&actor) else {
                return;
            };
            let Some(monster) = (unsafe { Monster::from_raw(actor) }) else {
                return;
            };
            if let Some(world) = &mut hits.world
                && world.hit == hit
                && world.center.is_none()
            {
                world.center = monster.state().map(MonsterState::position);
            }
            if contact.duration() <= 0.
                && hits.motion == Some(monster.motion())
                && (monster
                    .state()
                    .is_some_and(|state| hit == state.primary_contact().as_raw())
                    || hits.active_contacts().any(|extra| extra.as_raw() == hit))
            {
                contact.disable();
            }
        });
    }
    result
}

// 1696FC expands HTS into manager world volumes. Joint 127 in FZ effect 54
// is the position at spawn; do not reevaluate it from the skeleton each pass.
#[forge::hook(offset = 0x1696fc)]
unsafe fn expand(hit: u32) -> u32 {
    unsafe {
        let Some(contact) = Contact::from_raw(hit) else {
            return original!(hit);
        };
        let info = ACTORS.with(|actors| {
            actors
                .get(&contact.owner_address())
                .and_then(|h| h.world.as_ref())
                .filter(|w| w.hit == hit)
                .and_then(|w| w.center.map(|pos| (pos, w.radius)))
        });
        let Some((pos, radius)) = info else {
            return original!(hit);
        };
        u32::from(contact.submit_world_sphere(pos, radius))
    }
}

#[forge::hook(offset = 0x6ed40)]
unsafe fn release(actor: u32) -> u32 {
    hunt::forget(actor);
    reaction::forget(actor);
    ACTORS.with(|actors| {
        actors.remove(&actor);
    });
    original!(actor)
}
// Forge 1.5 generates a safe extern handler that dereferences its ABI argument.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
mod entry {
    use super::*;
    #[forge::entry]
    fn main() {
        let base = forge::mem::text_addr();
        use super::physiology::{add_anger, hitzone, mode};
        use super::reaction::flinch;
        forge::install_hook!(base, tick);
        forge::install_hook!(base, arm);
        forge::install_hook!(base, submit);
        forge::install_hook!(base, advance);
        forge::install_hook!(base, expand);
        forge::install_hook!(base, release);
        forge::install_hook!(base, hitzone);
        forge::install_hook!(base, flinch);
        forge::install_hook!(base, add_anger);
        forge::install_hook!(base, mode);
    }
}
