//! Family 85. 811F6164 and 811F6170 spawn two projectiles at the same position.
use crate::peer::{COUNT, Slot};
use crate::{collision, fx, native, program, read, sound};
use core::cell::UnsafeCell;
use mhgu_sdk::{
    combat::collision::{Contact, ContactPool},
    geometry::Angle,
};

struct Wave {
    actor: u32,
    area: u8,
    origin: [f32; 3],
    direction: [f32; 2],
    age: f32,
    limit: f32,
    travel: f32,
    contacts: ContactPool,
    spirit: u8,
}
impl Wave {
    fn stop(&mut self) {
        self.actor = 0;
        for contact in self.contacts.iter() {
            contact.disable();
        }
    }

    fn contact(&self, actor: u32, hit: u32) -> Option<Contact<'_>> {
        if self.actor != actor {
            return None;
        }
        self.contacts.iter().find(|contact| contact.as_raw() == hit)
    }
}

// GU borrows contact addresses past the expansion pass. Keep both slots at
// stable addresses across moves and scene changes, including after a reset.
struct State(UnsafeCell<Option<Wave>>);
unsafe impl Sync for State {}
static STATES: [State; COUNT] = [const { State(UnsafeCell::new(None)) }; COUNT];

pub fn reset(slot: Slot) {
    unsafe {
        if let Some(w) = (&mut *STATES[slot.0].0.get()).as_mut() {
            w.stop();
        }
    }
}

pub unsafe fn hit_gain(slot: Slot, actor: u32, hit: u32) -> u8 {
    unsafe {
        (&*STATES[slot.0].0.get())
            .as_ref()
            .filter(|w| w.contact(actor, hit).is_some())
            .map_or(0, |w| w.spirit)
    }
}

unsafe fn launch(slot: Slot, actor: u32, tier: usize, yaw: u16) {
    unsafe {
        let Some(template) = collision::blade_template(actor) else {
            return;
        };
        let Some(shape) = collision::DISCHARGE.load() else {
            return;
        };
        let Some(rows) = shape.rows_in_first_group(4) else {
            return;
        };
        if read::<[u32; 4]>(rows, 0).contains(&0) {
            return;
        }
        let row = program::wave(tier);
        let w = (&mut *STATES[slot.0].0.get()).get_or_insert_with(|| Wave {
            contacts: ContactPool::new(2),
            actor: 0,
            area: 0,
            origin: [0.; 3],
            direction: [0.; 2],
            age: 0.,
            limit: 0.,
            travel: 0.,
            spirit: 0,
        });
        w.actor = actor;
        w.area = read(actor, 4180);
        w.origin = read(actor, 64);
        let (sin, cos) = Angle::from_units(yaw).sin_cos();
        w.direction = [sin, cos];
        w.age = 0.;
        w.limit = f32::from(row.start + row.duration);
        w.travel = 50.;
        w.spirit = row.spirit;
        for c in w.contacts.iter() {
            collision::setup(
                actor,
                c,
                template,
                shape.as_raw(),
                [row.start, row.duration],
                row.power,
                row.source_hitstop,
            );
        }
    }
}

// GU 16ABAC expands the entire attack list once before resolving damage.
// FZ 81E9445A steps 50 units per 30 Hz tick, including the first tick.
pub(super) unsafe fn submit(slot: Slot, manager: u32) {
    unsafe {
        let Some(w) = (&mut *STATES[slot.0].0.get()).as_mut() else {
            return;
        };
        if w.actor == 0 {
            return;
        }
        if slot.live() != Some(w.actor)
            || read::<u8>(w.actor, 4180) != w.area
            || read::<i16>(w.actor, 5078) <= 0
            || w.age >= w.limit
        {
            w.stop();
            return;
        }
        let clock = read::<u32>(forge::mem::text_addr(), 0x2126764);
        if clock == 0 {
            return;
        }
        let dt = read::<f32>(clock, 104);
        if !(dt > 0.) {
            return;
        }
        w.travel = 50. * (1. + (w.age / 2.) as u32 as f32);
        for c in w.contacts.iter() {
            c.enqueue(manager);
        }
        w.age += dt;
    }
}

pub(super) unsafe fn expand(slot: Slot, owner: u32, hit: u32) -> u32 {
    unsafe {
        let Some(w) = (&*STATES[slot.0].0.get()).as_ref() else {
            return 0;
        };
        let Some(contact) = w.contact(owner, hit) else {
            return 0;
        };
        let [sin, cos] = w.direction;
        let position = |height, forward| {
            let distance = w.travel + forward;
            [
                w.origin[0] + sin * distance,
                w.origin[1] + height,
                w.origin[2] + cos * distance,
            ]
        };
        // FZ 82C7CFA4[0], four capsules per projectile and one contact serial.
        let ends = core::array::from_fn::<_, 4, _>(|i| {
            [
                position(0., if i == 0 { 0. } else { 40. }),
                position(200., 20. * i as f32),
            ]
        });
        contact.submit_world_capsules(&ends, 40.)
    }
}

pub unsafe fn frame(slot: Slot, actor: u32, state: u8, last: f32, now: f32, yaw: u16) {
    unsafe {
        let chain = (state - 54) / 3;
        let tier = if state == 65 { 3 } else { (state - 54) % 3 } as usize;
        let crossed = |at| last < at && at <= now;
        for at in [24., 54., 84.] {
            if crossed(at) {
                // 811F5D2E / 811F60B0 / 811F6026 attach to FZ node 1 (motion root).
                for layer in (0..chain).rev() {
                    fx::joint(actor, -1, 8 + layer as usize);
                }
                if at == 24. && chain > 0 {
                    sound::blade(actor, u8::from(chain == 3));
                }
            }
        }
        if crossed(128.) {
            if crate::world_contact_ready() {
                launch(slot, actor, tier, yaw);
            }
            fx::discharge(actor, tier, yaw);
            sound::blade(actor, if tier == 3 { 3 } else { 2 });
            if tier == 3 {
                sound::common(actor, 235);
            }
        }
        if crossed(180.) {
            native!(0x2a2560, actor, 0);
        }
    }
}
