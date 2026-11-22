//! FZ family 108 row 4 records the swept path independently of blade row 260.
use crate::peer::{COUNT, Slot};
use crate::{collision, program, read};
use core::cell::UnsafeCell;
use mhgu_sdk::{
    combat::collision::{Contact, ContactPool},
    style::action,
};

pub(super) const ACTION: u8 = 77;
const SPAWN: f32 = 26.;
const STRIKE: f32 = 46.;

struct Path {
    actor: u32,
    area: u8,
    frame: f32,
    previous: [f32; 3],
    ends: [[f32; 3]; 2],
}

impl Path {
    fn sample(&mut self, frame: f32, position: [f32; 3]) {
        // 8115B0A2 updates each endpoint only while below its own threshold.
        // The converter maps FZ node 0 to the actor's world root. GU joint 0 is the hips.
        for (end, until) in self
            .ends
            .iter_mut()
            .zip([program::SWEEP_START, program::SWEEP_END])
        {
            // The final 30 Hz source samples before 32 and 44 are frames 30 and 42.
            let final_sample = until - 2.;
            if frame >= SPAWN && frame <= final_sample {
                *end = position;
            } else if self.frame < final_sample && frame > final_sample {
                // If a GU update skipped source ticks, recover the last sampled
                // position before the boundary from the actual terrain-clipped travel.
                let t = (final_sample - self.frame) / (frame - self.frame);
                *end = core::array::from_fn(|i| {
                    self.previous[i] + t * (position[i] - self.previous[i])
                });
            }
        }
        self.previous = position;
        self.frame = frame;
    }
}

struct Sweep {
    path: Option<Path>,
    // GU keeps intrusive list pointers until the end of its collision pass.
    pool: Option<ContactPool>,
}

impl Sweep {
    fn contact(&self) -> Option<Contact<'_>> {
        self.pool.as_ref()?.iter().next()
    }

    fn stop(&mut self) {
        self.path = None;
        if let Some(contact) = self.contact() {
            contact.disable();
        }
    }

    fn active(&self, owner: u32, hit: u32) -> Option<(Contact<'_>, &Path)> {
        let path = self.path.as_ref()?;
        let contact = self.contact()?;
        (path.frame >= STRIKE && path.actor == owner && contact.as_raw() == hit)
            .then_some((contact, path))
    }
}

struct State(UnsafeCell<Sweep>);
unsafe impl Sync for State {}
static STATES: [State; COUNT] = [const {
    State(UnsafeCell::new(Sweep {
        path: None,
        pool: None,
    }))
}; COUNT];

pub(super) fn reset(slot: Slot) {
    unsafe { (*STATES[slot.0].0.get()).stop() }
}

pub(super) unsafe fn begin(slot: Slot, actor: u32) {
    if crate::world_contact_ready() {
        unsafe {
            let origin = read(actor, 64);
            (*STATES[slot.0].0.get()).path = Some(Path {
                actor,
                area: read(actor, 4180),
                frame: 0.,
                previous: origin,
                ends: [origin; 2],
            });
        }
    }
}

pub(super) unsafe fn collision_pass(slot: Slot, manager: u32) {
    unsafe {
        let state = &mut *STATES[slot.0].0.get();
        let Some(path) = &mut state.path else { return };
        let actor = path.actor;
        if slot.live() != Some(actor)
            || action::current(actor) != (action::GROUP, action::slot(ACTION))
            || read::<u8>(actor, 4180) != path.area
            || read::<i16>(actor, 5078) <= 0
        {
            state.stop();
            return;
        }
        if path.frame < STRIKE {
            let frame = read::<f32>(actor, 1280);
            path.sample(frame, read(actor, 64));
            if frame < STRIKE {
                return;
            }
            let row = program::instant();
            let remaining = f32::from(row.duration) - (frame - STRIKE);
            let Some(template) = collision::blade_template(actor) else {
                state.stop();
                return;
            };
            if remaining <= 0. {
                state.stop();
                return;
            }
            let shape = collision::INSTANT.load();
            let Some(shape) = shape.filter(|s| s.rows_in_first_group(1).is_some()) else {
                state.stop();
                return;
            };
            let pool = state.pool.get_or_insert_with(|| ContactPool::new(1));
            let contact = pool.iter().next().unwrap();
            collision::setup(
                actor,
                contact,
                template,
                shape.as_raw(),
                [row.start, row.duration],
                row.power,
                row.source_hitstop,
            );
            contact.set_window(f32::from(row.start), remaining);
        }
        if let Some(contact) = state.contact().filter(|c| c.enabled()) {
            contact.enqueue(manager);
        } else {
            state.stop();
        }
    }
}

pub(super) unsafe fn expand(slot: Slot, owner: u32, hit: u32) -> u32 {
    unsafe {
        let Some((contact, path)) = (&*STATES[slot.0].0.get()).active(owner, hit) else {
            return 0;
        };
        u32::from(contact.submit_world_capsule(path.ends, program::SWEEP_RADIUS))
    }
}

pub(super) fn hit_gain(slot: Slot, owner: u32, hit: u32) -> u8 {
    if unsafe { (&*STATES[slot.0].0.get()).active(owner, hit).is_some() } {
        program::instant().spirit
    } else {
        0
    }
}
