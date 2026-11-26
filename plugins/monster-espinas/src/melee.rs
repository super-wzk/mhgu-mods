//! Supplementary melee contacts. GU keeps their addresses until its collision pass.
use super::{
    Actors, byte,
    package::{Batch, Package, row_id},
};
use mhgu_sdk::{
    combat::collision::{Contact, ContactPool, world},
    monster::{Monster, MonsterState, attack::AttackTables, motion::MotionId},
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
static CONTACTS: Actors<Hits> = Actors::new();

pub(super) fn forget(actor: u32) {
    CONTACTS.remove(actor);
}

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

#[forge::hook(offset = 0x7dc2c)]
unsafe fn arm(actor: u32, slot: u32, id: i32, flags: u32) -> i32 {
    let result = original!(actor, slot, id, flags);
    if result != 1 || slot != 0 {
        return result;
    }
    unsafe {
        let Some((monster, tables, batch)) = attack_batch(actor, id, flags) else {
            CONTACTS.with(|actors| {
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
        CONTACTS.with(|actors| {
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
            let shift = if crate::package::Directory::read(monster).is_some() {
                // GU adds the remaining blend to its native delay. Our clock
                // already holds during that blend; measure only source frames.
                let shift = -monster.motion_frame();
                let start = f32::from(first_row.start()) + shift;
                anchor.set_window(
                    start.max(0.),
                    (f32::from(first_row.duration()) + start.min(0.)).max(0.),
                );
                shift
            } else {
                anchor.delay() + anchor.duration()
                    - f32::from(first_row.start())
                    - f32::from(first_row.duration())
            };
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
    if let Some(contact) = unsafe { Contact::from_raw(hit) }
        && let Some(monster) = clock_owner(contact)
        && crate::motion::advance(monster) == 0.
    {
        return 0;
    }
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
    CONTACTS.with(|actors| {
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
    let owner = unsafe { Contact::from_raw(hit) }.and_then(clock_owner);
    let result = if let Some(monster) = owner {
        let fraction = crate::motion::advance(monster);
        if fraction == 0. {
            return 0;
        }
        monster.with_motion_clock_rate(monster.motion_rate() * fraction, || original!(hit))
    } else {
        original!(hit)
    };
    let Some(contact) = (unsafe { Contact::from_raw(hit) }) else {
        return result;
    };
    if contact.enabled() && contact.delay() <= 0. {
        let actor = contact.owner_address();
        CONTACTS.with(|actors| {
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

fn clock_owner(contact: Contact<'_>) -> Option<Monster<'_>> {
    CONTACTS.with(|actors| {
        let hits = actors.get(&contact.owner_address())?;
        // Only the contacts installed for a known monster participate. Its
        // projectiles, native secondary slot and interrupted clips keep GU time.
        let monster = unsafe { Monster::from_raw(contact.owner_address()) }?;
        if hits.motion != Some(monster.motion()) {
            return None;
        }
        let primary = monster.state()?.primary_contact().as_raw();
        (contact.as_raw() == primary
            || hits
                .active_contacts()
                .any(|extra| extra.as_raw() == contact.as_raw()))
        .then_some(monster)
    })
}

// 1696FC expands HTS into manager world volumes. Joint 127 in FZ effect 54
// is the position at spawn; do not reevaluate it from the skeleton each pass.
struct WorldContact;

impl world::Handler for WorldContact {
    fn sphere(contact: world::ContactKey) -> Option<world::Sphere> {
        CONTACTS.with(|actors| {
            let world = actors.get(&contact.owner)?.world.as_ref()?;
            if world.hit != contact.address {
                return None;
            }
            Some(world::Sphere {
                center: world.center?,
                radius: world.radius,
            })
        })
    }
}

pub(super) fn bind_world_contact() -> bool {
    world::bind::<WorldContact>()
}

pub(super) fn install(base: u32) {
    forge::install_hook!(base, arm);
    forge::install_hook!(base, submit);
    forge::install_hook!(base, advance);
}
